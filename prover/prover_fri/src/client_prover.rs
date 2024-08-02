#![feature(generic_const_exprs)]
use clap::Parser;
use jsonrpsee::http_client::HttpClientBuilder;
use jsonrpsee::core::client::ClientT;
use jsonrpsee::types::params::ParamsSer;
use tokio;
use zksync_prover_fri::cpu_prover_utils::Prover;
use zksync_prover_fri_types::ProverJob;

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
    client: jsonrpsee::http_client::HttpClient,
    client_prover: Prover,
}

impl Client {
    pub async fn new(server_url: &str, max_size: u32) -> anyhow::Result<Self> {
        let client = HttpClientBuilder::default().max_request_body_size(max_size).build(server_url)?;
        let client_prover = Prover::new(Cli::parse().config_path).unwrap();
        Ok( Self { client, client_prover } )
    }

    pub async fn poll_for_job(&self) -> anyhow::Result<()> {

        loop {
            // Request a job
            let response: Result<ProverJob, _> = self.client.request("get_job", None).await;

            match response {
                Ok(job) => {
                        println!("Have to execute job {}, with block number {}, and request id {}.", job.job_id, job.block_number, job.request_id);
                        let req_id = job.request_id.clone();
                        let proof_artifact = self.client_prover.prove(job);
                        let result_json = serde_json::to_value(proof_artifact)?;
                        let params = ParamsSer::Array(vec![result_json]);
                        let submit_response: Result<(), _> = self.client.request("submit_result", Some(params)).await;

                        match submit_response {
                            Ok(_) => println!("Proof submitted and verified successfully."),
                            Err(e) => eprintln!("Failed to submit proof: {}.", e),
                        }
                }

            Err(e) => eprintln!("Error: {}.", e),
            }
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