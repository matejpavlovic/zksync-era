#![feature(generic_const_exprs)]
use std::sync::Arc;
use anyhow::Context as _;
use clap::Parser;
use jsonrpsee::http_client::HttpClientBuilder;
use jsonrpsee::core::client::ClientT;
use jsonrpsee::types::params::ParamsSer;
use tokio;
use zksync_prover_fri_types::PROVER_PROTOCOL_SEMANTIC_VERSION;
use zksync_types::basic_fri_types::CircuitIdRoundTuple;
use zksync_core_leftovers::temp_config_store::load_general_config;
use zksync_prover_fri::cpu_prover_utils::*;
use tokio::signal;
use std::time::Instant;

#[derive(Debug, Parser)]
#[command(author = "Matter Labs", version)]
pub(crate) struct Cli {
    #[arg(long)]
    pub(crate) config_path: Option<std::path::PathBuf>,
    #[arg(long)]
    pub(crate) secrets_path: Option<std::path::PathBuf>,
    #[arg(long)]
    pub(crate) server_url: String, // New argument for server URL
}

struct Client {
    server_url: String,
    max_size: u32,
    client: jsonrpsee::http_client::HttpClient,
    client_prover: Prover,
    setup_load_mode: SetupLoadMode,
}

impl Client {
    pub async fn new(server_url: &str, max_size: u32) -> anyhow::Result<Self> {
        let client = HttpClientBuilder::default().max_request_body_size(max_size).build(server_url)?;
        let opt = Cli::parse();
        let general_config = load_general_config(opt.config_path).context("general config")?;
        let prover_config = general_config.prover_config.context("fri_prover config")?;
        let setup_load_mode = load_setup_data_cache(&prover_config).context("load_setup_data_cache()")?;
        let circuit_ids_for_round_to_be_proven = vec![CircuitIdRoundTuple::new(4, 0)];
        let client_prover = Prover::new(prover_config.clone(), setup_load_mode.clone(), circuit_ids_for_round_to_be_proven, PROVER_PROTOCOL_SEMANTIC_VERSION);

        Ok(Self {
            server_url: server_url.to_string(),
            max_size,
            client,
            client_prover,
            setup_load_mode,
        })
    }

    pub async fn poll_for_job(&self) -> anyhow::Result<()> {

        //let config = Arc::clone(&self.client_prover.config);

        loop {
            let response: Result<Job, _> = self.client.request("get_job", None).await;

            match response {
                Ok(job) => {
                    let proof_job = job.proof_job;
                    let result = format!(
                        "Have to execute job {}, with block number {}, and request id {}.",
                        proof_job.job_id, proof_job.block_number, job.request_id
                    );
                    println!("{}", result);

                    let setup_data = get_setup_data(self.setup_load_mode.clone(), proof_job.setup_data_key.clone()).context("get_setup_data()").unwrap();
                    let started_at = Instant::now();
                    //let proof_artifact = self.client_prover.prove(proof_job, self.client_prover.config.clone(), setup_data, job.request_id);
                    let proof_artifact = self.client_prover.prove(proof_job, setup_data, job.request_id);
                    println!("Finished proving, took: {:?}", started_at.elapsed());
                    let job_result = JobResult::new(job.request_id, proof_artifact);

                    let result_json = serde_json::to_value(job_result)?;
                    let params = ParamsSer::Array(vec![result_json]);
                    let submit_response: Result<(), _> = self.client.request("submit_result", Some(params)).await;

                    match submit_response {
                        Ok(_) => println!("Proof submitted and verified successfully."),
                        Err(e) => eprintln!("Failed to submit proof: {}.", e),
                    }
                }

            Err(e) => eprintln!("Error: {}.", e),
            }
            //Ok(())
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = Cli::parse();
    let server_url = &opt.server_url;
    let max_size: u32 = 20 * 1024 * 1024;
    let client = Client::new(server_url, max_size).await?;
    client.poll_for_job().await
}