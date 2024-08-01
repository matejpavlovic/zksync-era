use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicUsize, Ordering};
use anyhow::{Context, Result};
use clap::Parser;
use jsonrpsee::http_server::{HttpServerBuilder, RpcModule};
use tokio::signal;
use tokio::sync::oneshot;
use zksync_core_leftovers::temp_config_store::load_general_config;
use zksync_object_store::StoredObject;
use zksync_prover_fri::cpu_prover_utils::{get_setup_data, Job, JobResult, load_setup_data_cache, SetupLoadMode, verify_proof_artifact};
use zksync_prover_fri_types::{CircuitWrapper, ProverJob, ProverServiceDataKey};
use zksync_types::basic_fri_types::AggregationRound;
use zksync_types::L1BatchNumber;

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
}

impl Server {
    pub async fn new(server_addr: SocketAddr, max_size: u32) -> Result<Self> {
        let opt = Cli::parse();
        let general_config = load_general_config(opt.config_path).context("general config")?;
        let prover_config = general_config.prover_config.context("fri_prover config")?;

        Ok(Self {
            server_addr,
            max_size,
            jobs: Arc::new(RwLock::new(HashMap::new())),
            request_id: Arc::new(AtomicUsize::new(0)),
            setup_load_mode: load_setup_data_cache(&prover_config).context("load_setup_data_cache()")?,
        })
    }

    fn register_methods(&self, module: &mut RpcModule<()>) -> Result<()> {
        let request_id_clone = Arc::clone(&self.request_id);
        let jobs_clone = Arc::clone(&self.jobs);

        module.register_method("get_job", move |_params, _| {
            let req_id = request_id_clone.fetch_add(1, Ordering::SeqCst) as u32;
            let path = "/home/johnstephan/1_0_4_BasicCircuits_0.bin";
            let mut file = File::open(path)?;
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)?;

            let circuit_wrapper = <CircuitWrapper as StoredObject>::deserialize(buffer).expect("Deserialization of circuit wrapper.");
            let setup_data_key = ProverServiceDataKey::new(4, AggregationRound::BasicCircuits);
            let proof_job = ProverJob::new(L1BatchNumber(1), 10, circuit_wrapper, setup_data_key);
            let mut jobs = jobs_clone.write().unwrap();
            jobs.insert(req_id, proof_job.clone());
            println!("Job {} with request id {} inserted.", proof_job.job_id, req_id);

            Ok(Job { request_id: req_id, proof_job })
        })?;

        let jobs_clone = Arc::clone(&self.jobs);
        let setup_load_mode_clone = self.setup_load_mode.clone();
        module.register_method("submit_result", move |params, _| {
            let job_result: JobResult = params.one()?;
            let mut jobs = jobs_clone.write().unwrap();
            if let Some(job) = jobs.remove(&job_result.request_id) {
                println!("Received proof artifact for job {} with request id {}.", job.job_id, job_result.request_id);
                let setup_data = get_setup_data(setup_load_mode_clone.clone(), job.setup_data_key.clone()).context("get_setup_data()").unwrap();
                verify_proof_artifact(job_result, job, &setup_data.vk);
                Ok(())
            } else {
                Err(jsonrpsee::core::Error::Custom("Job not found".into()))
            }
        })?;

        Ok(())
    }

    async fn run(&self) -> Result<()> {
        let server = HttpServerBuilder::default()
            .max_request_body_size(self.max_size)
            .max_response_body_size(self.max_size)
            .build(self.server_addr)
            .await?;

        let mut module = RpcModule::new(());
        self.register_methods(&mut module)?;

        let server_handle = server.start(module)?;

        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
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
    let server = Server::new(server_addr, max_size).await?;
    server.run().await
}