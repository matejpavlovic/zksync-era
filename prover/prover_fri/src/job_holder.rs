use std::collections::HashMap;
use std::fs::File;
use std::io::{Read};
use tokio::signal;
use zksync_object_store::{StoredObject};
use zksync_prover_fri_types::{CircuitWrapper, ProverJob, ProverServiceDataKey};
use zksync_types::basic_fri_types::{AggregationRound};
use zksync_types::L1BatchNumber;
use jsonrpsee::core::server::rpc_module::RpcModule;
use jsonrpsee::http_server::{HttpServerBuilder};
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicUsize, Ordering};
use anyhow::Context;
use clap::Parser;
use tokio::sync::oneshot;
use zksync_core_leftovers::temp_config_store::load_general_config;
use zksync_prover_fri::cpu_prover_utils::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Define the server address
    let server_addr: SocketAddr = "127.0.0.1:3030".parse()?;

    // Build the server
    let max_size = 20 * 1024 * 1024;
    let server = HttpServerBuilder::default().max_request_body_size(max_size).max_response_body_size(max_size).build(server_addr).await?;

    // Create a module to register the methods
    let mut module = RpcModule::new(());

    // Shared state for jobs
    let jobs = Arc::new(RwLock::new(HashMap::new()));

    // Shared atomic request ID
    let request_id = Arc::new(AtomicUsize::new(0));

    // Build an instance of a prover
    let opt = Cli::parse();
    let general_config = load_general_config(opt.config_path).context("general config")?;
    let prover_config = general_config.prover_config.context("fri_prover config")?;
    let setup_load_mode = load_setup_data_cache(&prover_config).context("load_setup_data_cache()")?;

    // Register a method named "get_job"
    {
        let request_id_clone = Arc::clone(&request_id);
        let jobs_clone = Arc::clone(&jobs);
        module.register_method("get_job", move |_params, _| {

            let req_id = request_id_clone.fetch_add(1, Ordering::SeqCst) as u32;

            let path = "/home/johnstephan/1_0_4_BasicCircuits_0.bin";
            let mut file = File::open(path)?;
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)?;
            // Deserialize the bytes into the circuit wrapper
            let circuit_wrapper = <CircuitWrapper as StoredObject>::deserialize(buffer).expect("Deserialization of circuit wrapper.");
            let setup_data_key = ProverServiceDataKey::new(4, AggregationRound::BasicCircuits);
            let proof_job = ProverJob::new(L1BatchNumber(1), 10, circuit_wrapper, setup_data_key);
            let job = Job {
                request_id: req_id,
                proof_job: proof_job.clone(),
            };

            {
                // Store the job in the HashMap
                let mut jobs = jobs_clone.write().unwrap();
                jobs.insert(req_id, proof_job.clone());
                println!("Job {} with request id {} inserted.", proof_job.job_id, req_id);
            }

            Ok(job)
        })?;
    }

    // Register a method named "submit_result"
    {
        let jobs_clone = Arc::clone(&jobs);
        module.register_method("submit_result", move |params, _| {
            let job_result: JobResult = params.one()?;
            let mut jobs = jobs_clone.write().unwrap();
            if let Some(job) = jobs.remove(&job_result.request_id) {
                println!("Received proof artifact for job {} with request id {}.", job.job_id, job_result.request_id);
                // Verify the proof
                let setup_data = get_setup_data(setup_load_mode.clone(), job.setup_data_key.clone()).context("get_setup_data()").unwrap();
                verify_proof_artifact(job_result, job, &setup_data.vk);
                Ok(())
            } else {
                Err(jsonrpsee::core::Error::Custom("Job not found".into()))
            }
        })?;
    }

    // Start the server
    let server_handle = server.start(module)?;

    // Wait for Ctrl+C to stop the server
    let (tx, rx) = oneshot::channel();
    tokio::spawn(async move {
        signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
        tx.send(()).unwrap();
    });

    rx.await.unwrap();
    let _ = server_handle.stop();

    Ok(())
}


#[derive(Debug, Parser)]
#[command(author = "Matter Labs", version)]
pub(crate) struct Cli {
    #[arg(long)]
    pub(crate) config_path: Option<std::path::PathBuf>,
    #[arg(long)]
    pub(crate) secrets_path: Option<std::path::PathBuf>,
}