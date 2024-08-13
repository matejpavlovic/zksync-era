use clap::Parser;
use jsonrpsee::core::client::ClientT;
use jsonrpsee::http_client::HttpClientBuilder;
use jsonrpsee::rpc_params;
use tokio;
use zksync_prover_fri::cpu_prover_utils::Prover;
use zksync_prover_fri_types::ProverJob;
use zksync_types::basic_fri_types::CircuitIdRoundTuple;
use zksync_core_leftovers::temp_config_store::load_general_config;
use anyhow::Context as _;

#[derive(Debug, Parser)]
#[command(author = "Matter Labs", version)]
pub(crate) struct Cli {
    #[arg(long)]
    pub(crate) config_path: Option<std::path::PathBuf>,
    #[arg(long)]
    pub(crate) secrets_path: Option<std::path::PathBuf>,
    #[arg(long)]
    pub(crate) server_url: String,
    #[arg(long, default_value = "(1,0)")] // Set default value if not provided
    pub(crate) circuit_ids_rounds: String,
}

struct Client {
    client: jsonrpsee::http_client::HttpClient,
    client_prover: Prover,
}

impl Client {
    pub async fn new(server_url: &str, max_size: u32, circuit_ids_rounds: String) -> anyhow::Result<Self> {
        let client = HttpClientBuilder::default()
            .max_request_size(max_size)
            .max_response_size(max_size)
            .build(server_url)?;

        let general_config = load_general_config(Cli::parse().config_path.clone()).context("general config")?;
        let prover_config = general_config.prover_config.context("fri_prover config")?;

        // Parse the circuit_ids_rounds string into a Vec<CircuitIdRoundTuple>
        let circuit_ids_for_round_to_be_proven: Vec<CircuitIdRoundTuple> = parse_circuit_ids_rounds(&circuit_ids_rounds)?;
        /*let circuit_ids_for_round_to_be_proven = general_config
            .prover_group_config
            .expect("prover_group_config")
            .get_circuit_ids_for_group_id(prover_config.specialized_group_id)
            .unwrap_or_default();
        let circuit_ids_for_round_to_be_proven =
            get_all_circuit_id_round_tuples_for(circuit_ids_for_round_to_be_proven);*/

        let client_prover = Prover::new(prover_config, circuit_ids_for_round_to_be_proven).unwrap();
        Ok(Self {
            client,
            client_prover,
        })
    }

    pub async fn poll_for_job(&self) -> anyhow::Result<()> {
        // Request a job
        let circuit_ids_json = serde_json::to_value(self.client_prover.circuit_ids_for_round_to_be_proven.clone())?;
        let response: Result<ProverJob, _> = self.client.request("get_job", rpc_params![circuit_ids_json]).await;

        match response {
            Ok(job) => {
                println!(
                    "Have to execute job {} with request id {}.",
                    job.job_id, job.request_id
                );
                let proof_artifact = self.client_prover.prove(job);
                let result_json = serde_json::to_value(proof_artifact)?;

                let submit_response: Result<(), _> = self
                    .client
                    .request("submit_result", rpc_params![result_json])
                    .await;

                match submit_response {
                    Ok(_) => println!("Proof submitted and verified successfully."),
                    Err(e) => eprintln!("Failed to submit proof: {}.", e),
                }
            }
            Err(e) => eprintln!("{}", e)
        }
        Ok(())
    }
}

fn parse_circuit_ids_rounds(s: &str) -> Result<Vec<CircuitIdRoundTuple>, anyhow::Error> {
    s.split("),(")
        .map(|pair| {
            let cleaned = pair.trim_matches(|c| c == '(' || c == ')');
            let nums: Vec<&str> = cleaned.split(',').collect();
            if nums.len() != 2 {
                return Err(anyhow::anyhow!("Invalid tuple format: {}", pair));
            }
            let first: u8 = nums[0]
                .trim()
                .parse::<u32>()
                .context("Failed to parse first element of tuple")?
                .try_into()
                .context("Failed to convert first element to u8")?;
            let second: u8 = nums[1]
                .trim()
                .parse::<u32>()
                .context("Failed to parse second element of tuple")?
                .try_into()
                .context("Failed to convert second element to u8")?;
            Ok(CircuitIdRoundTuple::new(first, second))
        })
        .collect()
}


#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = Cli::parse();
    let server_url = &opt.server_url;
    let max_size: u32 = 20 * 1024 * 1024;
    let circuit_ids_rounds = opt.circuit_ids_rounds;
    let client = Client::new(server_url, max_size, circuit_ids_rounds).await?;
    client.poll_for_job().await?;
    Ok(())
}