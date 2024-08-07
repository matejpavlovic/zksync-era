//#![feature(generic_const_exprs)]
use std::{collections::HashMap, sync::Arc, time::Instant};
use anyhow::Context as _;
use circuit_definitions::boojum::cs::implementations::verifier::VerificationKey;
use zkevm_test_harness::prover_utils::{prove_base_layer_circuit, prove_recursion_layer_circuit};
use zksync_config::configs::{fri_prover_group::FriProverGroupConfig, FriProverConfig};
use zksync_env_config::FromEnv;
use zksync_prover_fri_types::{circuit_definitions::{
    base_layer_proof_config,
    boojum::{cs::implementations::pow::NoPow, worker::Worker},
    circuit_definitions::{
        base_layer::{ZkSyncBaseLayerCircuit, ZkSyncBaseLayerProof},
        recursion_layer::{ZkSyncRecursionLayerProof, ZkSyncRecursiveLayerCircuit},
    },
    recursion_layer_proof_config,
}, CircuitWrapper, FriProofWrapper, ProverJob, ProverServiceDataKey};
use zksync_vk_setup_data_server_fri::{keystore::Keystore, GoldilocksProverSetupData};

use zksync_core_leftovers::temp_config_store::load_general_config;

use crate::{metrics::{CircuitLabels, Layer, METRICS}, utils::{setup_metadata_to_setup_data_key, get_setup_data_key, verify_proof, ProverArtifacts}};

#[derive(Clone)]
pub enum SetupLoadMode {
    FromMemory(HashMap<ProverServiceDataKey, Arc<GoldilocksProverSetupData>>),
    FromDisk,
}

pub struct Prover {
    pub config: Arc<FriProverConfig>,
    pub setup_load_mode: SetupLoadMode,
}

impl Prover {
    #[allow(dead_code)]
    pub fn new(
        config_path: core::option::Option<std::path::PathBuf>,
    ) -> anyhow::Result<Self> {

        let general_config = load_general_config(config_path).context("general config")?;
        let prover_config = general_config.prover_config.context("fri_prover config")?;
        let setup_load_mode = load_setup_data_cache(&prover_config).context("load_setup_data_cache()")?;

        Ok(Prover {
            config: Arc::new(prover_config),
            setup_load_mode,
        })
    }

    pub fn prove(&self,
                 job: ProverJob,
    ) -> ProverArtifacts {

        let setup_data = get_setup_data(self.setup_load_mode.clone(), job.setup_data_key.clone()).context("get_setup_data()").unwrap();
        println!("Proving.");
        let started_at = Instant::now();

        let proof_wrapper = match job.circuit_wrapper {
            CircuitWrapper::Base(base_circuit) => {
                Self::prove_base_layer(job.job_id, base_circuit, self.config.clone(), setup_data, job.request_id)
            }
            CircuitWrapper::Recursive(recursive_circuit) => {
                Self::prove_recursive_layer(job.job_id, recursive_circuit, self.config.clone(), setup_data, job.request_id)
            }
        };

        println!("Finished proving, took: {:?}", started_at.elapsed());
        ProverArtifacts::new(job.block_number, proof_wrapper, job.request_id)
    }

    fn prove_recursive_layer(
        job_id: u32,
        circuit: ZkSyncRecursiveLayerCircuit,
        _config: Arc<FriProverConfig>,
        artifact: Arc<GoldilocksProverSetupData>,
        request_id: u32,
    ) -> FriProofWrapper {
        let worker = Worker::new();
        let circuit_id = circuit.numeric_circuit_type();
        //let started_at = Instant::now();
        let proof = prove_recursion_layer_circuit::<NoPow>(
            circuit.clone(),
            &worker,
            recursion_layer_proof_config(),
            &artifact.setup_base,
            &artifact.setup,
            &artifact.setup_tree,
            &artifact.vk,
            &artifact.vars_hint,
            &artifact.wits_hint,
            &artifact.finalization_hint,
        );

        /*let label = CircuitLabels {
            circuit_type: circuit_id,
            layer: Layer::Recursive,
        };
        METRICS.proof_generation_time[&label].observe(started_at.elapsed());*/

        verify_proof(&CircuitWrapper::Recursive(circuit), &proof, &artifact.vk, job_id, request_id);
        FriProofWrapper::Recursive(ZkSyncRecursionLayerProof::from_inner(circuit_id, proof))

    }

    fn prove_base_layer(
        job_id: u32,
        circuit: ZkSyncBaseLayerCircuit,
        _config: Arc<FriProverConfig>,
        artifact: Arc<GoldilocksProverSetupData>,
        request_id: u32,
    ) -> FriProofWrapper {
        let worker = Worker::new();
        let circuit_id = circuit.numeric_circuit_type();
        let started_at = Instant::now();
        let proof = prove_base_layer_circuit::<NoPow>(
            circuit.clone(),
            &worker,
            base_layer_proof_config(),
            &artifact.setup_base,
            &artifact.setup,
            &artifact.setup_tree,
            &artifact.vk,
            &artifact.vars_hint,
            &artifact.wits_hint,
            &artifact.finalization_hint,
        );

        let label = CircuitLabels {
            circuit_type: circuit_id,
            layer: Layer::Base,
        };
        METRICS.proof_generation_time[&label].observe(started_at.elapsed());

        verify_proof(&CircuitWrapper::Base(circuit), &proof, &artifact.vk, job_id, request_id);
        FriProofWrapper::Base(ZkSyncBaseLayerProof::from_inner(circuit_id, proof))
    }

}

/*pub fn verify_and_save_proof(proof_artifact: ProverArtifacts, job: ProverJob, vk: &VerificationKey<F, H>){
    match (proof_artifact.proof_wrapper, job.circuit_wrapper) {

        (FriProofWrapper::Base(proof), CircuitWrapper::Base(base_circuit)) => {
            verify_proof(&CircuitWrapper::Base(base_circuit), &proof.into_inner(), &vk, job.job_id, proof_artifact.request_id);
        }

        (FriProofWrapper::Recursive(proof), CircuitWrapper::Recursive(recursive_circuit)) => {
            verify_proof(&CircuitWrapper::Recursive(recursive_circuit), &proof.into_inner(), &vk, job.job_id, proof_artifact.request_id);
        }
        _ => {}
    };

}*/

#[allow(dead_code)]
pub fn load_setup_data_cache(config: &FriProverConfig) -> anyhow::Result<SetupLoadMode> {
    Ok(match config.setup_load_mode {
        zksync_config::configs::fri_prover::SetupLoadMode::FromDisk => SetupLoadMode::FromDisk,
        zksync_config::configs::fri_prover::SetupLoadMode::FromMemory => {
            let mut cache = HashMap::new();
            tracing::info!(
        "Loading setup data cache for group {}",
        &config.specialized_group_id
    );
            let prover_setup_metadata_list = FriProverGroupConfig::from_env()
                .context("FriProverGroupConfig::from_env()")?
                .get_circuit_ids_for_group_id(config.specialized_group_id)
                .expect(
                    "At least one circuit should be configured for group when running in FromMemory mode",
                );
            tracing::info!(
        "for group {} configured setup metadata are {:?}",
        &config.specialized_group_id,
        prover_setup_metadata_list
    );
            let keystore = Keystore::default();
            for prover_setup_metadata in prover_setup_metadata_list {
                let key = setup_metadata_to_setup_data_key(&prover_setup_metadata);
                let setup_data = keystore
                    .load_cpu_setup_data_for_circuit_type(key.clone())
                    .context("get_cpu_setup_data_for_circuit_type()")?;
                cache.insert(key, Arc::new(setup_data));
            }
            SetupLoadMode::FromMemory(cache)
        }
    })
}

pub fn get_setup_data(
    setup_load_mode: SetupLoadMode,
    key: ProverServiceDataKey,
) -> anyhow::Result<Arc<GoldilocksProverSetupData>> {
    let key = get_setup_data_key(key);
    Ok(match setup_load_mode {
        SetupLoadMode::FromMemory(cache) => cache
            .get(&key)
            .context("Setup data not found in cache")?
            .clone(),
        SetupLoadMode::FromDisk => {
            let started_at = Instant::now();
            let keystore = Keystore::default();
            let artifact: GoldilocksProverSetupData = keystore
                .load_cpu_setup_data_for_circuit_type(key.clone())
                .context("get_cpu_setup_data_for_circuit_type()")?;
            //METRICS.gpu_setup_data_load_time[&key.circuit_id.to_string()].observe(started_at.elapsed());
            println!("Setup data load time, took: {:?}", started_at.elapsed());

            Arc::new(artifact)
        }
    })
}