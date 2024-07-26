#![feature(generic_const_exprs)]
use std::{collections::HashMap, sync::Arc, time::Instant};
use anyhow::Context as _;
use circuit_definitions::boojum::algebraic_props::round_function::AbsorptionModeOverwrite;
use circuit_definitions::boojum::algebraic_props::sponge::GoldilocksPoseidon2Sponge;
use circuit_definitions::boojum::cs::implementations::proof::Proof;
use circuit_definitions::boojum::cs::implementations::verifier::VerificationKey;
use circuit_definitions::boojum::field::goldilocks::{GoldilocksExt2, GoldilocksField};
use circuit_definitions::circuit_definitions::recursion_layer::ZkSyncRecursionLayerStorageType;
use clap::Parser;
use tokio::task::JoinHandle;
use zkevm_test_harness::prover_utils::{prove_base_layer_circuit, prove_recursion_layer_circuit, verify_base_layer_proof, verify_recursion_layer_proof};
use zksync_config::configs::{fri_prover_group::FriProverGroupConfig, FriProverConfig};
use zksync_env_config::FromEnv;
use zksync_object_store::{bincode, ObjectStore};
use zksync_prover_dal::{ConnectionPool, ProverDal};
use zksync_prover_fri_types::{circuit_definitions::{
    base_layer_proof_config,
    boojum::{cs::implementations::pow::NoPow, worker::Worker},
    circuit_definitions::{
        base_layer::{ZkSyncBaseLayerCircuit, ZkSyncBaseLayerProof},
        recursion_layer::{ZkSyncRecursionLayerProof, ZkSyncRecursiveLayerCircuit},
    },
    recursion_layer_proof_config,
}, CircuitWrapper, FriProofWrapper, PROVER_PROTOCOL_SEMANTIC_VERSION, ProverJob, ProverServiceDataKey};
use zksync_prover_fri_utils::fetch_next_circuit;
use zksync_queued_job_processor::{async_trait, JobProcessor};
use zksync_types::{basic_fri_types::CircuitIdRoundTuple, L1BatchNumber, protocol_version::ProtocolSemanticVersion};
use zksync_vk_setup_data_server_fri::{keystore::Keystore, GoldilocksProverSetupData};

use crate::{metrics::{CircuitLabels, Layer, METRICS}, utils::{setup_metadata_to_setup_data_key, get_setup_data_key, verify_proof, ProverArtifacts}};

use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use serde::{Deserialize, Serialize};

use jsonrpsee::http_client::HttpClientBuilder;
use jsonrpsee::core::client::ClientT;
use jsonrpsee::types::params::ParamsSer;
use tokio;
use zksync_core_leftovers::temp_config_store::load_general_config;
use zksync_types::basic_fri_types::AggregationRound;
use crate::utils::{F, H};

#[derive(Clone)]
pub enum SetupLoadMode {
    FromMemory(HashMap<ProverServiceDataKey, Arc<GoldilocksProverSetupData>>),
    FromDisk,
}

pub struct Prover {
    pub config: Arc<FriProverConfig>,
    setup_load_mode: SetupLoadMode,
    circuit_ids_for_round_to_be_proven: Vec<CircuitIdRoundTuple>,
    protocol_version: ProtocolSemanticVersion,
}

impl Prover {
    #[allow(dead_code)]
    pub fn new(
        config: FriProverConfig,
        setup_load_mode: SetupLoadMode,
        circuit_ids_for_round_to_be_proven: Vec<CircuitIdRoundTuple>,
        protocol_version: ProtocolSemanticVersion,
    ) -> Self {
        Prover {
            config: Arc::new(config),
            setup_load_mode,
            circuit_ids_for_round_to_be_proven,
            protocol_version,
        }
    }

    pub fn prove(&self,
                 job: ProverJob,
                 config: Arc<FriProverConfig>,
                 setup_data: Arc<GoldilocksProverSetupData>,
                 request_id: u32,
    ) -> ProverArtifacts {
        println!("PROVING.");

        let proof_wrapper = match job.circuit_wrapper {
            CircuitWrapper::Base(base_circuit) => {
                Self::prove_base_layer(job.job_id, base_circuit, config, setup_data, request_id)
            }
            CircuitWrapper::Recursive(recursive_circuit) => {
                Self::prove_recursive_layer(job.job_id, recursive_circuit, config, setup_data, request_id)
            }
        };

        println!("Done PROVING.");
        ProverArtifacts::new(job.block_number, proof_wrapper)
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

pub fn verify_proof_artifact(job_result: JobResult, job: ProverJob, vk: &VerificationKey<F, H>){
    match (job_result.proof_artifact.proof_wrapper, job.circuit_wrapper) {
        (FriProofWrapper::Base(proof), CircuitWrapper::Base(base_circuit)) => {
            verify_proof(&CircuitWrapper::Base(base_circuit), &proof.into_inner(), &vk, job.job_id, job_result.request_id);
        }

        (FriProofWrapper::Recursive(proof), CircuitWrapper::Recursive(recursive_circuit)) => {
            verify_proof(&CircuitWrapper::Recursive(recursive_circuit), &proof.into_inner(), &vk, job.job_id, job_result.request_id);
        }
        _ => {}
    };

}

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
            METRICS.gpu_setup_data_load_time[&key.circuit_id.to_string()].observe(started_at.elapsed());

            Arc::new(artifact)
        }
    })
}


#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct Job {
    pub request_id: u32,
    pub proof_job: ProverJob,
}

impl Job {
    pub fn new(
        request_id: u32,
        proof_job: ProverJob,
    ) -> Self {
        Self {
            request_id,
            proof_job,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct JobResult {
    pub request_id: u32,
    pub proof_artifact: ProverArtifacts,
}

impl JobResult {
    pub fn new(
        request_id: u32,
        proof_artifact: ProverArtifacts,
    ) -> Self {
        Self {
            request_id,
            proof_artifact,
        }
    }
}