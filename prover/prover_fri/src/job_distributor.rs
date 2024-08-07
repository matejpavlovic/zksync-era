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
use zksync_prover_fri_types::PROVER_PROTOCOL_SEMANTIC_VERSION;
use zksync_prover_fri_utils::fetch_next_circuit;
use zksync_prover_fri_utils::get_all_circuit_id_round_tuples_for;
use zksync_types::{
    basic_fri_types::CircuitIdRoundTuple,
    protocol_version::ProtocolSemanticVersion,
};
use zksync_prover_fri::cpu_prover_utils::{
    get_setup_data, load_setup_data_cache, SetupLoadMode,
};
use zksync_prover_fri::utils::{ProverArtifacts, save_proof};
use zksync_prover_fri_types::ProverJob;
use zksync_config::configs::FriProverConfig;
use zksync_prover_fri::utils::verify_proof_2;

use zksync_prover_fri_types::{circuit_definitions::{
    base_layer_proof_config,
    circuit_definitions::{
        base_layer::{ZkSyncBaseLayerCircuit, ZkSyncBaseLayerProof},
        recursion_layer::{ZkSyncRecursionLayerProof, ZkSyncRecursiveLayerCircuit},
    },
    recursion_layer_proof_config,
}, CircuitWrapper, FriProofWrapper, ProverServiceDataKey};

use zksync_prover_fri_types::{
    circuit_definitions::{
        boojum::{
            algebraic_props::{
                round_function::AbsorptionModeOverwrite, sponge::GoldilocksPoseidon2Sponge,
            },
            field::goldilocks::{GoldilocksExt2, GoldilocksField},
        },
        circuit_definitions::recursion_layer::{ZkSyncRecursionLayerStorageType,
        },
    },
    queue::FixedSizeQueue, WitnessVectorArtifacts,
};
use zksync_prover_fri::utils::{F, H};
use circuit_definitions::boojum::cs::implementations::verifier::VerificationKey;



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
    jobs: Arc<RwLock<HashMap<u32, ProverJob>>>,
    request_id: Arc<AtomicUsize>,
    setup_load_mode: SetupLoadMode,
    prover_config: FriProverConfig,
    object_store: Arc<dyn ObjectStore>,
    prover_connection_pool: ConnectionPool<Prover>,
    circuit_ids_for_round_to_be_proven: Vec<CircuitIdRoundTuple>,
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
        let protocol_version = PROVER_PROTOCOL_SEMANTIC_VERSION;

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

        let blob_store = store_factory.create_store().await?;
        let public_blob_store = match prover_config.shall_save_to_public_bucket {
            false => None,
            true => Some(
                ObjectStoreFactory::new(public_object_store_config)
                    .create_store()
                    .await?,
            ),
        };

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
            prover_config,
            object_store,
            prover_connection_pool,
            circuit_ids_for_round_to_be_proven,
            protocol_version,
            blob_store,
            public_blob_store
        })
    }

    async fn register_methods(self: Arc<Self>, module: &mut RpcModule<()>) -> anyhow::Result<()> {
        let server = self.clone();
        module.register_async_method("get_job", move |_params, _,_| {
            let server = server.clone();
            async move {
                let _req_id = server.request_id.fetch_add(1, Ordering::SeqCst) as u32;
                let proof_job_option = server.get_next_job(_req_id)
                    .await
                    .map_err(|_e| ErrorObject::from(ErrorCode::InternalError))?;

                if let Some(proof_job) = proof_job_option {
                    // Insert the job in the hash table
                    let mut jobs = server.jobs.write().await;
                    jobs.insert(_req_id, proof_job.clone());
                    println!("Job {} with request id {} inserted.", proof_job.job_id, _req_id);
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
                    let started_at = Instant::now();
                    server.verify_and_save_proof(proof_artifact.clone(), job.clone(), &setup_data.vk, started_at);
                    Ok(())
                } else {
                    println!("There is no current job with request id {}.", proof_artifact.request_id);
                    Err(ErrorObject::from(ErrorCode::InternalError))
                }
            }
        })?;

        Ok(())
    }

    async fn get_next_job(&self, _req_id: u32) -> anyhow::Result<Option<ProverJob>> {
        let mut storage = self.prover_connection_pool.connection().await.unwrap();
        let Some(job) = fetch_next_circuit(
            &mut storage,
            &*self.object_store,
            &self.circuit_ids_for_round_to_be_proven,
            &self.protocol_version,
            _req_id,
        )
            .await
        else {
            return Ok(None);
        };
        Ok(Some(job))
    }

    async fn verify_and_save_proof(&self, proof_artifact: ProverArtifacts, job: ProverJob, vk: &VerificationKey<F, H>, started_at: Instant) {
        let is_valid = match (proof_artifact.proof_wrapper.clone(), job.circuit_wrapper) {
            (FriProofWrapper::Base(proof), CircuitWrapper::Base(base_circuit)) => {
                verify_proof_2(&CircuitWrapper::Base(base_circuit), &proof.into_inner(), vk, job.job_id, proof_artifact.request_id.clone())
            }
            (FriProofWrapper::Recursive(proof), CircuitWrapper::Recursive(recursive_circuit)) => {
                verify_proof_2(&CircuitWrapper::Recursive(recursive_circuit), &proof.into_inner(), vk, job.job_id, proof_artifact.request_id.clone())
            }
            _ => false, // Handle the mismatched case by returning false
        };

        if is_valid {
            let _ = self.save_result(job.job_id, started_at, proof_artifact)
                .await
                .context("save_result()");
            println!("Wrote to DB that job {} has been successfully completed.", job.job_id);
        } else {
            let msg = format!("Proof verification failed for job: {}", job.job_id);
            tracing::error!("{}", msg);
            println!("{}", msg);
        }
    }

    async fn save_result(
        &self,
        job_id: u32,
        started_at: Instant,
        artifacts: ProverArtifacts,
    ) -> anyhow::Result<()> {

        let mut storage_processor = self.prover_connection_pool.connection().await.unwrap();
        save_proof(
            job_id,
            started_at,
            artifacts,
            &*self.blob_store,
            self.public_blob_store.as_deref(),
            self.prover_config.shall_save_to_public_bucket,
            &mut storage_processor,
            self.protocol_version,
        )
            .await;
        Ok(())
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