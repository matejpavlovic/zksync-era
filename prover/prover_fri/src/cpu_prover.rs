#![feature(generic_const_exprs)]
use std::{sync::Arc};
use anyhow::Context as _;
use clap::Parser;
use zksync_prover_fri_types::{PROVER_PROTOCOL_SEMANTIC_VERSION};
use zksync_types::{basic_fri_types::CircuitIdRoundTuple};
use jsonrpsee::http_client::HttpClientBuilder;
use jsonrpsee::core::client::ClientT;
use jsonrpsee::types::params::ParamsSer;
use tokio;
use zksync_core_leftovers::temp_config_store::load_general_config;
use zksync_prover_fri::cpu_prover_utils::*;


#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Define the server address
    let server_url = "http://127.0.0.1:3030";

    // Build the client
    // Set max request body size to 20 GB
    let max_size = 20 * 1024 * 1024;
    let client = HttpClientBuilder::default().max_request_body_size(max_size).build(server_url)?;

    // Build the prover
    let opt = Cli::parse();
    let general_config = load_general_config(opt.config_path).context("general config")?;
    let prover_config = general_config.prover_config.context("fri_prover config")?;
    let setup_load_mode = load_setup_data_cache(&prover_config).context("load_setup_data_cache()")?;
    let circuit_ids_for_round_to_be_proven = vec![CircuitIdRoundTuple::new(4, 0)];
    let client_prover = Prover::new(prover_config.clone(), setup_load_mode.clone(), circuit_ids_for_round_to_be_proven, PROVER_PROTOCOL_SEMANTIC_VERSION);

    // Poll the server for a job
    let response: Result<Job, _> = client.request("get_job", None).await;

    // Handle the response
    match response {
        Ok(job) => {
            let proof_job = job.proof_job;
            let result = format!("Have to execute job {}, with block number {}, and request id {}.", proof_job.job_id, proof_job.block_number, job.request_id);
            println!("{}", result);

            let config = Arc::clone(&client_prover.config);
            let setup_data = get_setup_data(setup_load_mode.clone(), proof_job.setup_data_key.clone());
            let proof_artifact = client_prover.prove(proof_job, config, setup_data.context("get_setup_data()").unwrap());
            let job_result = JobResult::new(job.request_id, proof_artifact);
            // Serialize the job result to JSON
            let result_json = serde_json::to_value(job_result)?;
            // Convert to ParamsSer::Array
            let params = ParamsSer::Array(vec![result_json]);
            let submit_response: Result<(), _> = client.request("submit_result", Some(params)).await;

            match submit_response {
                Ok(_) => println!("Proof submitted and verified successfully."),
                Err(e) => eprintln!("Failed to submit proof: {}.", e),
            }
        }
        Err(e) => eprintln!("Error: {}.", e),
    }

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