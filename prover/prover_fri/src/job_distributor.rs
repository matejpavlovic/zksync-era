use jsonrpsee::types::{ErrorCode, ErrorObject};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

use anyhow::{Context, Result};
use clap::Parser;
use jsonrpsee::server::RpcModule;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::signal;
use tokio::sync::oneshot;
use zksync_config::configs::FriProverConfig;
use zksync_core_leftovers::temp_config_store::{load_database_secrets, load_general_config};
use zksync_env_config::object_store::ProverObjectStoreConfig;
use zksync_object_store::ObjectStore;
use zksync_object_store::ObjectStoreFactory;
use zksync_prover_dal::{ConnectionPool, Prover};
use zksync_prover_fri_types::PROVER_PROTOCOL_SEMANTIC_VERSION;
use zksync_prover_fri_utils::fetch_next_circuit;
use zksync_prover_fri_utils::get_all_circuit_id_round_tuples_for;
use zksync_types::{
    basic_fri_types::{AggregationRound, CircuitIdRoundTuple},
    protocol_version::ProtocolSemanticVersion,
};

use zksync_prover_fri::cpu_prover_utils::{
    get_setup_data, load_setup_data_cache, verify_proof_artifact, SetupLoadMode,
};
use zksync_prover_fri::utils::ProverArtifacts;
use zksync_prover_fri_types::{CircuitWrapper, ProverJob, ProverServiceDataKey};
use zksync_types::L1BatchNumber;

#[derive(Debug, Parser)]
#[command(author = "Matter Labs", version)]
pub(crate) struct Cli {
    #[arg(long)]
    pub(crate) config_path: Option<std::path::PathBuf>,
    #[arg(long)]
    pub(crate) secrets_path: Option<std::path::PathBuf>,
}

#[derive(Clone)]
struct Server {
    server_addr: SocketAddr,
    max_size: u32,
    jobs: Arc<RwLock<HashMap<u32, ProverJob>>>,
    request_id: Arc<AtomicUsize>,
    setup_load_mode: SetupLoadMode,
    object_store: Arc<dyn ObjectStore>,
    pool: ConnectionPool<Prover>,
    circuit_ids_for_round_to_be_proven: Vec<CircuitIdRoundTuple>,
    protocol_version: ProtocolSemanticVersion,
}

impl Server {
    pub async fn new(server_addr: SocketAddr, max_size: u32) -> Result<Self> {
        let opt = Cli::parse();
        let general_config = load_general_config(opt.config_path).context("general config")?;
        let prover_config = general_config.prover_config.context("fri_prover config")?;
        let database_secrets =
            load_database_secrets(opt.secrets_path).context("database secrets")?;
        let protocol_version = PROVER_PROTOCOL_SEMANTIC_VERSION;

        let pool = ConnectionPool::singleton(database_secrets.prover_url()?)
            .build()
            .await
            .context("failed to build a connection pool")?;
        let object_store_config = ProverObjectStoreConfig(
            prover_config
                .clone()
                .prover_object_store
                .context("object store")?,
        );
        let object_store = ObjectStoreFactory::new(object_store_config.0)
            .create_store()
            .await?;
        let circuit_ids_for_round_to_be_proven = general_config
            .prover_group_config
            .expect("prover_group_config")
            .get_circuit_ids_for_group_id(prover_config.specialized_group_id)
            .unwrap_or_default();
        let circuit_ids_for_round_to_be_proven =
            get_all_circuit_id_round_tuples_for(circuit_ids_for_round_to_be_proven);

        Ok(Self {
            server_addr,
            max_size,
            jobs: Arc::new(RwLock::new(HashMap::new())),
            request_id: Arc::new(AtomicUsize::new(0)),
            setup_load_mode: load_setup_data_cache(&prover_config)
                .context("load_setup_data_cache()")?,
            object_store,
            pool,
            circuit_ids_for_round_to_be_proven,
            protocol_version,
        })
    }

    async fn register_methods(self: Arc<Self>, module: &mut RpcModule<()>) -> anyhow::Result<()> {
        let server = self.clone();
        module.register_async_method("get_job", move |_params, _,_| {
            let server = server.clone();
            async move {
                let req_id = server.request_id.fetch_add(1, Ordering::SeqCst) as u32;
                let proof_job_option = server.get_next_job().await.map_err(|_e| ErrorObject::from(ErrorCode::InternalError))?;

                if let Some(proof_job) = proof_job_option {
                    let mut jobs = server.jobs.write().await;
                    jobs.insert(req_id, proof_job.clone());
                    println!("Job {} with request id {} inserted.", proof_job.job_id, req_id);
                    Ok(proof_job)
                } else {
                    println!("No job is available.");
                    Err(ErrorObject::from(ErrorCode::InternalError))
                }
            }
        })?;

        let server = self.clone();
        module.register_async_method("submit_result", move |_params, _,_| {
            let server = server.clone();
            async move {
                let proof_artifact: ProverArtifacts = _params.one()?;
                let mut jobs = server.jobs.write().await;
                if let Some(job) = jobs.remove(&proof_artifact.request_id) {
                    println!("Received proof artifact for job {} with request id {}.", job.job_id, job.request_id);
                    let setup_data = get_setup_data(server.setup_load_mode.clone(), job.setup_data_key.clone()).context("get_setup_data()").unwrap();
                    verify_proof_artifact(proof_artifact, job, &setup_data.vk);
                    Ok(())
                } else {
                    println!("There is no current job with request id {}.", proof_artifact.request_id);
                    Err(ErrorObject::from(ErrorCode::InternalError))
                }
            }
        })?;

        Ok(())
    }

    async fn get_next_job(&self) -> anyhow::Result<Option<ProverJob>> {
        let mut storage = self.pool.connection().await.unwrap();
        let Some(job) = fetch_next_circuit(
            &mut storage,
            &*self.object_store,
            &self.circuit_ids_for_round_to_be_proven,
            &self.protocol_version,
        )
            .await
        else {
            return Ok(None);
        };
        Ok(Some(job))
    }

    async fn run(self: Arc<Self>) -> anyhow::Result<()> {
        let server = jsonrpsee::server::Server::builder()
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