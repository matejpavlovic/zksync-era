use jsonrpsee::types::{ErrorCode, ErrorObject};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Context, Result};
use clap::Parser;
use jsonrpsee::server::RpcModule;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tokio::signal;
use tokio::sync::oneshot;
use zksync_core_leftovers::temp_config_store::{load_database_secrets, load_general_config};
use zksync_env_config::object_store::ProverObjectStoreConfig;
use zksync_object_store::ObjectStore;
use zksync_object_store::ObjectStoreFactory;
use zksync_prover_dal::{ConnectionPool, Prover};
use zksync_prover_fri_utils::fetch_next_circuit;
use zksync_types::{
    basic_fri_types::CircuitIdRoundTuple,
    protocol_version::ProtocolSemanticVersion,
};
use zksync_prover_fri::cpu_prover_utils::verify_client_proof;
use zksync_prover_fri::utils::{ProverArtifacts, save_proof};
use zksync_prover_fri_types::{ProverJob, PROVER_PROTOCOL_SEMANTIC_VERSION};
use zksync_config::configs::FriProverConfig;


#[derive(Debug, Parser)]
#[command(author = "Matter Labs", version)]
pub(crate) struct Cli {
    #[arg(long)]
    pub(crate) config_path: Option<std::path::PathBuf>,
    #[arg(long)]
    pub(crate) secrets_path: Option<std::path::PathBuf>,
}

struct Server {
    server_addr: SocketAddr,
    max_size: u32,
    jobs: Arc<RwLock<HashMap<u32, (ProverJob, Instant)>>>,
    request_id: Arc<AtomicUsize>,
    prover_config: FriProverConfig,
    object_store: Arc<dyn ObjectStore>,
    prover_connection_pool: ConnectionPool<Prover>,
    protocol_version: ProtocolSemanticVersion,
    blob_store: Arc<dyn ObjectStore>,
    public_blob_store: Option<Arc<dyn ObjectStore>>,
}

impl Server {
    pub async fn new(server_addr: SocketAddr, max_size: u32) -> Result<Self> {
        let opt = Cli::parse();
        let general_config = load_general_config(opt.config_path).context("general config")?;
        let prover_config = general_config.prover_config.context("fri_prover config")?;
        let database_secrets =
            load_database_secrets(opt.secrets_path).context("database secrets")?;

        let prover_connection_pool =
            ConnectionPool::<Prover>::singleton(database_secrets.prover_url()?)
                .build()
                .await
                .context("failed to build a prover_connection_pool")?;
        let object_store_config = ProverObjectStoreConfig(
            prover_config
                .clone()
                .prover_object_store
                .context("object store")?,
        );
        let object_store = ObjectStoreFactory::new(object_store_config.0.clone())
            .create_store()
            .await?;
        let store_factory = ObjectStoreFactory::new(object_store_config.0);
        let public_object_store_config = prover_config
            .public_object_store
            .clone()
            .context("public object store config")?;

        let public_blob_store = match prover_config.shall_save_to_public_bucket {
            false => None,
            true => Some(
                ObjectStoreFactory::new(public_object_store_config)
                    .create_store()
                    .await?,
            ),
        };

        Ok(Self {
            server_addr,
            max_size,
            jobs: Arc::new(RwLock::new(HashMap::new())),
            request_id: Arc::new(AtomicUsize::new(0)),
            prover_config,
            object_store,
            prover_connection_pool,
            protocol_version: PROVER_PROTOCOL_SEMANTIC_VERSION,
            blob_store: store_factory.create_store().await?,
            public_blob_store
        })
    }

    async fn register_methods(self: Arc<Self>, module: &mut RpcModule<()>) -> anyhow::Result<()> {

        let server = self.clone();
        module.register_async_method("get_job", move |_params, _,_| {
            let server = server.clone();
            async move {
                let circuit_ids_for_round_to_be_proven: Vec<CircuitIdRoundTuple> = _params.one()?;
                let _req_id = server.request_id.fetch_add(1, Ordering::SeqCst) as u32;
                let proof_job_option = server.get_next_job(_req_id, circuit_ids_for_round_to_be_proven)
                    .await
                    .map_err(|_e| ErrorObject::from(ErrorCode::InternalError))?;

                if let Some(proof_job) = proof_job_option {
                    // Insert the job in the hash table
                    let mut jobs = server.jobs.write().await;
                    let started_job_at = Instant::now();
                    jobs.insert(_req_id, (proof_job.clone(), started_job_at));
                    println!("Job {} with request id {} inserted.", proof_job.job_id, _req_id);
                    Ok(proof_job)
                } else {
                    println!("No job is available.");
                    Err(ErrorObject::from(ErrorCode::InternalError))
                }
            }
        })?;

        module.register_async_method("submit_result", move |_params, _, _| {
            let server = self.clone();
            async move {
                let proof_artifact: ProverArtifacts = _params.one()?;
                let mut jobs = server.jobs.write().await;
                if let Some((job, started_job_at)) = jobs.remove(&proof_artifact.request_id) {
                    println!("Received proof artifact for job {} with request id {}.", job.job_id, job.request_id);
                    let server_clone = server.clone();

                    // Respond to the client immediately
                    tokio::spawn(async move {
                        let job_id = job.job_id.clone();
                        if verify_client_proof(proof_artifact.clone(), job).await {
                            let _ = server_clone.save_proof_to_db(job_id, proof_artifact, started_job_at).await;
                        }
                    });

                    // Respond with success
                    Ok(())
                } else {
                    println!("There is no current job with request id {}.", proof_artifact.request_id);
                    Err(ErrorObject::from(ErrorCode::InternalError))
                }
            }
        })?;

        Ok(())
    }

    async fn get_next_job(&self, _req_id: u32, circuit_ids_for_round_to_be_proven: Vec<CircuitIdRoundTuple>) -> anyhow::Result<Option<ProverJob>> {
        let mut storage = self.prover_connection_pool.connection().await.unwrap();
        let Some(job) = fetch_next_circuit(
            &mut storage,
            &*self.object_store,
            &circuit_ids_for_round_to_be_proven,
            &self.protocol_version,
            _req_id,
        )
            .await
        else {
            return Ok(None);
        };
        Ok(Some(job))
    }

    async fn save_proof_to_db(
        &self,
        job_id: u32,
        artifacts: ProverArtifacts,
        started_at: Instant,
    ) -> anyhow::Result<()> {
        // Error handling when getting connection
        let mut storage_processor = match self.prover_connection_pool.connection().await {
            Ok(conn) => conn,
            Err(e) => {
                println!("Failed to get connection for job {}: {:?}", job_id, e);
                return Err(anyhow::anyhow!("Failed to get connection for job {}: {:?}", job_id, e));
            }
        };

        save_proof(
            job_id,
            started_at,
            artifacts,
            &*self.blob_store,
            self.public_blob_store.as_deref(),
            self.prover_config.shall_save_to_public_bucket,
            &mut storage_processor,
            self.protocol_version,
        ).await;
        println!("Wrote to DB that job {} has been successfully completed.", job_id);
        Ok(())
    }

    async fn run(self: Arc<Self>) -> anyhow::Result<()> {
        let server = jsonrpsee::server::Server::builder()
            .max_request_body_size(self.max_size)
            .build(self.server_addr)
            .await?;

        let mut module = RpcModule::new(());
        self.clone().register_methods(&mut module).await?;

        let server_handle = server.start(module);

        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            signal::ctrl_c()
                .await
                .expect("failed to install Ctrl+C handler");
            tx.send(()).unwrap();
        });

        rx.await.unwrap();
        let _ = server_handle.stop();

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let server_addr: SocketAddr = "0.0.0.0:3030".parse()?;
    let max_size = 20 * 1024 * 1024;
    let server = Arc::new(Server::new(server_addr, max_size).await?);
    server.run().await
}