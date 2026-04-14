use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::mem::ManuallyDrop;
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use acropolis_common::validation::UplcMachineError;
use acropolis_common::{
    validation::Phase2ValidationError, CostModels, ReferenceScript, ScriptHash, ScriptLang,
    TxUTxODeltas, UTXOValue, UTxOIdentifier,
};
use rayon::prelude::*;
use rayon::ThreadPool;
use uplc_turbo::data::PlutusData;
use uplc_turbo::{
    arena::Arena,
    binder::DeBruijn,
    bumpalo::Bump,
    constant::Constant,
    machine::{ExBudget, PlutusVersion},
    term::Term,
};

use crate::validations::phase_two::script_context::encode_tx_info;
use crate::validations::phase_two::TxInfo;

use super::script_context::ScriptContext;

// =============================================================================
// Evaluator Thread Pool
// =============================================================================
// Real Plutus scripts (4-12KB) can have deep AST structures that cause stack
// overflow with the default 2MB stack. We use a dedicated thread pool with
// larger stacks (16MB) for script evaluation.

/// Stack size for evaluator threads: 16MB.
const EVALUATOR_STACK_SIZE: usize = 16 * 1024 * 1024;

/// Initial capacity of each arena in the pool: 1MB.
const ARENA_INITIAL_CAPACITY: usize = 1024 * 1024;

fn evaluator_thread_count() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
}

/// Global thread pool with large stacks for script evaluation.
static EVALUATOR_POOL: OnceLock<ThreadPool> = OnceLock::new();

fn evaluator_pool() -> &'static ThreadPool {
    EVALUATOR_POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(evaluator_thread_count())
            .stack_size(EVALUATOR_STACK_SIZE)
            .thread_name(|i| format!("plutus-eval-{i}"))
            .build()
            .expect("Failed to create evaluator thread pool")
    })
}

// =============================================================================
// Arena Pool
// =============================================================================
// Pre-allocated pool of arenas to reduce allocation contention during parallel
// script evaluation. Each arena is reset and returned to the pool after use.

/// A bounded pool of uplc-turbo Arenas.
///
/// All arenas are pre-allocated at creation time. When all arenas are in use,
/// `acquire()` blocks until one becomes available.
#[derive(Clone)]
struct ArenaPool {
    inner: Arc<ArenaPoolInner>,
}

struct ArenaPoolInner {
    arenas: Mutex<VecDeque<Arena>>,
    condvar: Condvar,
}

impl ArenaPool {
    fn new(size: usize, initial_capacity: usize) -> Self {
        let mut arenas = VecDeque::with_capacity(size);
        for _ in 0..size {
            arenas.push_back(Arena::from_bump(Bump::with_capacity(initial_capacity)));
        }
        Self {
            inner: Arc::new(ArenaPoolInner {
                arenas: Mutex::new(arenas),
                condvar: Condvar::new(),
            }),
        }
    }

    fn acquire(&self) -> PooledArena {
        let mut guard = self.inner.arenas.lock().unwrap_or_else(|p| p.into_inner());
        let arena = loop {
            if let Some(arena) = guard.pop_front() {
                break arena;
            }
            guard = self.inner.condvar.wait(guard).unwrap_or_else(|p| p.into_inner());
        };
        PooledArena {
            arena: ManuallyDrop::new(arena),
            pool: self.inner.clone(),
        }
    }
}

/// RAII guard for a pooled arena. Resets and returns the arena on drop.
///
/// Returns the arena to the pool when dropped, after calling `reset()` to
/// clear all allocations. This allows arena reuse without repeated allocations.
struct PooledArena {
    arena: ManuallyDrop<Arena>,
    pool: Arc<ArenaPoolInner>,
}

impl std::ops::Deref for PooledArena {
    type Target = Arena;
    fn deref(&self) -> &Self::Target {
        &self.arena
    }
}

impl Drop for PooledArena {
    fn drop(&mut self) {
        // SAFETY: We only take the arena once, here in drop.
        let mut arena = unsafe { ManuallyDrop::take(&mut self.arena) };
        arena.reset();
        let mut pool = self.pool.arenas.lock().unwrap_or_else(|p| p.into_inner());
        pool.push_back(arena);
        self.pool.condvar.notify_one();
    }
}

/// Global arena pool for script evaluation.
static ARENA_POOL: OnceLock<ArenaPool> = OnceLock::new();

fn arena_pool() -> &'static ArenaPool {
    ARENA_POOL.get_or_init(|| ArenaPool::new(evaluator_thread_count(), ARENA_INITIAL_CAPACITY))
}

// =============================================================================
// Helpers
// =============================================================================

fn from_common_version(v: acropolis_common::PlutusVersion) -> PlutusVersion {
    match v {
        acropolis_common::PlutusVersion::V1 => PlutusVersion::V1,
        acropolis_common::PlutusVersion::V2 => PlutusVersion::V2,
        acropolis_common::PlutusVersion::V3 => PlutusVersion::V3,
    }
}

// =============================================================================
// Raw program evaluation (for calibration and benchmarks)
// =============================================================================

/// Evaluate a raw FLAT-encoded Plutus program without argument application.
///
/// Uses the same evaluator thread pool and arena pool as `evaluate_scripts`.
/// Intended for calibration and benchmarking only.
pub fn evaluate_raw_flat_program(flat_bytes: &[u8]) -> Result<std::time::Duration, String> {
    let flat_bytes = flat_bytes.to_vec();
    evaluator_pool().install(|| {
        let arena = arena_pool().acquire();
        let program: &uplc_turbo::program::Program<DeBruijn> =
            uplc_turbo::flat::decode(&arena, &flat_bytes)
                .map_err(|e| format!("Decode failed: {e:?}"))?;
        let start = std::time::Instant::now();
        let result = program.eval(&arena);
        let elapsed = start.elapsed();
        result.term.map_err(|e| format!("Evaluation failed: {e:?}"))?;
        Ok(elapsed)
    })
}

// =============================================================================
// Script table
// =============================================================================

/// Build a temporary table mapping script hashes to their full `ReferenceScript`.
///
/// Scripts come from two sources:
/// 1. Transaction script witnesses (`tx_deltas.script_witnesses`)
/// 2. Reference scripts from input/reference_input UTXOs, fetched via `lookup_ref_script`
pub fn build_scripts_table(
    tx_deltas: &TxUTxODeltas,
    utxos: &HashMap<UTxOIdentifier, UTXOValue>,
    lookup_ref_script: impl Fn(&ScriptHash) -> Option<Arc<ReferenceScript>>,
) -> HashMap<ScriptHash, Arc<ReferenceScript>> {
    let mut table = HashMap::new();

    // Source 1: script witnesses from the transaction
    if let Some(witnesses) = &tx_deltas.script_witnesses {
        for (hash, script) in witnesses {
            table.insert(*hash, Arc::new(script.clone()));
        }
    }

    // Source 2: reference scripts from input/reference_input UTXOs
    for input in tx_deltas.consumes.iter().chain(tx_deltas.reference_inputs.iter()) {
        if let Some(utxo) = utxos.get(input) {
            if let Some(script_ref) = &utxo.script_ref {
                if table.contains_key(&script_ref.script_hash) {
                    continue;
                }

                if let Some(ref_script) = lookup_ref_script(&script_ref.script_hash) {
                    table.insert(script_ref.script_hash, ref_script);
                }
            }
        }
    }

    table
}

// =============================================================================
// Script evaluation
// =============================================================================

/// Evaluate all Plutus scripts for a transaction in parallel.
///
/// Uses a dedicated thread pool with 16MB stacks and a pre-allocated arena pool
/// to handle deep recursion and reduce allocation overhead. Each script gets its
/// own arena from the pool.
///
/// If `is_valid` is false, scripts are expected to fail (per Alonzo spec).
pub fn evaluate_scripts(
    tx_info: &TxInfo,
    script_contexts: &[ScriptContext<'_>],
    scripts_table: &HashMap<ScriptHash, Arc<ReferenceScript>>,
    cost_models: &CostModels,
    is_valid: bool,
) -> Result<(), Phase2ValidationError> {
    if script_contexts.is_empty() {
        return Ok(());
    }

    let plutus_versions = script_contexts
        .iter()
        .filter_map(|sc| match sc.script_lang {
            ScriptLang::Plutus(v) => Some(v),
            ScriptLang::Native => None,
        })
        .collect::<HashSet<_>>();

    // encode tx info first in parallel
    let mut cached_tx_info_pd = HashMap::new();
    let arena_for_tx_info = Arena::from_bump(Bump::with_capacity(ARENA_INITIAL_CAPACITY));
    plutus_versions.iter().for_each(|common_version| {
        let version = from_common_version(*common_version);
        if let Ok(tx_info_pd) = encode_tx_info(tx_info, &arena_for_tx_info, version) {
            cached_tx_info_pd.insert(*common_version, tx_info_pd);
        }
    });

    // Run all script evaluations in parallel on the evaluator thread pool
    let script_result: Result<(), Phase2ValidationError> = evaluator_pool().install(|| {
        script_contexts.par_iter().try_for_each(|sc| {
            let arena = arena_pool().acquire();
            evaluate_single_script(&arena, sc, &cached_tx_info_pd, scripts_table, cost_models)
        })
    });

    // Handle is_valid flag: if false, expect scripts to fail
    if is_valid {
        script_result
    } else {
        match script_result {
            Ok(()) => Err(Phase2ValidationError::ValidityStateError),
            Err(_) => Ok(()),
        }
    }
}

/// Evaluate a single script execution on the current thread.
fn evaluate_single_script(
    arena: &Arena,
    sc: &ScriptContext<'_>,
    cached_tx_info_pd: &HashMap<acropolis_common::PlutusVersion, &PlutusData<'_>>,
    scripts_table: &HashMap<ScriptHash, Arc<ReferenceScript>>,
    cost_models: &CostModels,
) -> Result<(), Phase2ValidationError> {
    let common_plutus_version = match &sc.script_lang {
        ScriptLang::Plutus(version) => *version,
        ScriptLang::Native => return Ok(()),
    };
    let plutus_version = from_common_version(common_plutus_version);

    // Get cached encoded tx info plutus data if exists
    let tx_info_pd = cached_tx_info_pd.get(&common_plutus_version).copied();

    // 1. Build script arguments
    let args = sc
        .to_script_args(tx_info_pd, arena, plutus_version)
        .map_err(Phase2ValidationError::ScriptContextError)?;

    // 2. Look up script bytes
    let ref_script = scripts_table
        .get(&sc.script_hash)
        .ok_or(Phase2ValidationError::MissingScriptForHash(sc.script_hash))?;

    let cbor_bytes = match ref_script.as_ref() {
        ReferenceScript::PlutusV1(bytes)
        | ReferenceScript::PlutusV2(bytes)
        | ReferenceScript::PlutusV3(bytes) => bytes,
        ReferenceScript::Native(_) => return Ok(()),
    };

    // Script bytes are CBOR-wrapped (a CBOR byte string containing FLAT data).
    let script_bytes: serde_cbor::Value = serde_cbor::from_slice(cbor_bytes).map_err(|e| {
        Phase2ValidationError::UplcMachineError(UplcMachineError::DecodeFailed {
            script_hash: sc.script_hash,
            reason: format!("failed to CBOR-unwrap script bytes: {e}"),
        })
    })?;
    let script_bytes = match script_bytes {
        serde_cbor::Value::Bytes(b) => b,
        _ => {
            return Err(Phase2ValidationError::UplcMachineError(
                UplcMachineError::DecodeFailed {
                    script_hash: sc.script_hash,
                    reason: "script CBOR is not a byte string".into(),
                },
            ));
        }
    };

    // 3. Get cost model for this version
    let cost_model = match plutus_version {
        PlutusVersion::V1 => cost_models.plutus_v1.as_ref().ok_or(
            Phase2ValidationError::MissingCostModel(common_plutus_version),
        )?,
        PlutusVersion::V2 => cost_models.plutus_v2.as_ref().ok_or(
            Phase2ValidationError::MissingCostModel(common_plutus_version),
        )?,
        PlutusVersion::V3 => cost_models.plutus_v3.as_ref().ok_or(
            Phase2ValidationError::MissingCostModel(common_plutus_version),
        )?,
    };

    // 4. Flat-decode the script
    let mut program = uplc_turbo::flat::decode::<DeBruijn>(arena, &script_bytes)
        .map_err(|e| Phase2ValidationError::FlatDecodingError(e.to_string()))?;

    // 5. Apply arguments to the program
    for arg in &args {
        program = program.apply(arena, Term::data(arena, arg));
    }

    // 6. Evaluate with budget
    let budget = ExBudget {
        mem: sc.redeemer.ex_units.mem as i64,
        cpu: sc.redeemer.ex_units.steps as i64,
    };

    let result = program.eval_with_params(arena, plutus_version, cost_model.as_vec(), budget);

    // 7. Check result per version
    match plutus_version {
        PlutusVersion::V1 | PlutusVersion::V2 => match result.term {
            Ok(term) => match term {
                Term::Error => Err((UplcMachineError::ScriptFailed {
                    script_hash: sc.script_hash,
                    message: "Error term evaluated".into(),
                })
                .into()),
                _ => Ok(()),
            },
            Err(e) => Err((UplcMachineError::ScriptFailed {
                script_hash: sc.script_hash,
                message: e.to_string(),
            })
            .into()),
        },
        // Per CIP-117: V3 scripts must evaluate to Constant(Unit)
        PlutusVersion::V3 => match result.term {
            Ok(Term::Constant(Constant::Unit)) => Ok(()),
            Ok(_) => Err((UplcMachineError::ScriptFailed {
                script_hash: sc.script_hash,
                message: "Script evaluated to non-unit term".into(),
            })
            .into()),
            Err(e) => Err((UplcMachineError::ScriptFailed {
                script_hash: sc.script_hash,
                message: e.to_string(),
            })
            .into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::super::script_context::{build_script_contexts, TxInfo};
    use super::*;
    use crate::test_utils::{to_era, to_pallas_era, TestContext};
    use crate::validation_fixture;
    use acropolis_common::{genesis_values::GenesisValues, NetworkId, TxIdentifier};
    use pallas::ledger::traverse::MultiEraTx;
    use test_case::test_case;

    #[test_case(validation_fixture!(
        "alonzo",
        "a95d16e891e51f98a3b1d3fe862ed355ebc8abffb7a7269d86f775553d9e653f"
    ) =>
        matches Ok(());
        "alonzo - valid transaction 1 - with Plutus V1 Script"
    )]
    #[test_case(validation_fixture!(
        "conway",
        "332aac636f8476b1a91c0071a445103d8f55309c23bfddaf242732630efcf0ec"
    ) =>
        matches Ok(());
        "conway - valid transaction 1 - with Plutus V3 Script"
    )]
    #[test_case(validation_fixture!(
        "conway",
        "db3287808e25a301a03661f89104350c2dcb9e4c6afc46cfe92a4bda93d5c063"
    ) =>
        matches Ok(());
        "conway - valid transaction 2 - with minting Plutus V3 Script"
    )]
    #[test_case(validation_fixture!(
        "conway",
        "eed81408c2b2542eb41931bd3c410c28798e2ec52d9432428cdfbb12ffee0554"
    ) =>
        matches Ok(());
        "conway - valid transaction 3 - with 2 Plutus V1 Scripts"
    )]
    #[test_case(validation_fixture!(
        "conway",
        "f7474bdb6de986abb3e983ee17f579503f1449e27e6cd1e052ce7520f64b8976"
    ) =>
        matches Ok(());
        "conway - valid transaction 4 - with 3 Plutus V2 Script"
    )]
    // TODO:
    // make this test case pass
    // #[test_case(validation_fixture!(
    //     "conway",
    //     "74558bb6b317b59612a68cc9d3a4ced4038f26ad9b179d83133480f1a246199d"
    // ) =>
    //     matches Ok(());
    //     "conway - valid transaction 5 - with 2 Plutus V2 Scripts"
    // )]
    #[test_case(validation_fixture!(
        "conway",
        "2c4f36c252265ffadf89c460de2394af258a45e8093bd1c4684bf86b6ab51704"
    ) =>
        matches Ok(());
        "conway - valid transaction 6 - with 2 Plutus V1 Scripts"
    )]
    #[test_case(validation_fixture!(
        "conway",
        "332aac636f8476b1a91c0071a445103d8f55309c23bfddaf242732630efcf0ec",
        "always_fail"
    ) =>
        matches Err(Phase2ValidationError::UplcMachineError(_));
        "conway - invalid transaction - with always failed Plutus V3 Script"
    )]
    #[allow(clippy::result_large_err)]
    fn phase2_evalute_test(
        (ctx, raw_tx, era): (TestContext, Vec<u8>, &str),
    ) -> Result<(), Phase2ValidationError> {
        let tx = MultiEraTx::decode_for_era(to_pallas_era(era), &raw_tx).unwrap();
        let raw_tx = tx.encode();
        let mapped_tx = acropolis_codec::map_transaction(
            &tx,
            &raw_tx,
            TxIdentifier::default(),
            NetworkId::Mainnet,
            to_era(era),
        );
        let tx_error = mapped_tx.error.as_ref();
        assert!(tx_error.is_none());

        let tx_deltas = mapped_tx.convert_to_utxo_deltas(true);

        let lookup_ref_script = |script_hash: &ScriptHash| {
            ctx.reference_scripts
                .iter()
                .find(|(hash, _)| **hash == *script_hash)
                .map(|(_, reference_script)| Arc::new(reference_script.clone()))
        };
        let scripts_table = build_scripts_table(&tx_deltas, &ctx.utxos, lookup_ref_script);

        let genesis_values = GenesisValues::mainnet();
        let tx_info = TxInfo::new(&tx_deltas, &ctx.utxos, &genesis_values).unwrap();
        let scripts_needed = crate::utils::get_scripts_needed(&tx_deltas, &ctx.utxos);
        let scripts_provided = crate::utils::get_scripts_provided(&tx_deltas, &ctx.utxos);
        let script_contexts =
            build_script_contexts(&tx_info, &scripts_needed, &scripts_provided).unwrap();

        let cost_models = ctx.protocol_params.cost_models();

        evaluate_scripts(
            &tx_info,
            &script_contexts,
            &scripts_table,
            &cost_models,
            tx_deltas.is_valid,
        )
    }

    #[test]
    fn evaluate_missing_script_returns_error() {
        let (ctx, raw_tx, era): (TestContext, Vec<u8>, &str) = validation_fixture!(
            "alonzo",
            "a95d16e891e51f98a3b1d3fe862ed355ebc8abffb7a7269d86f775553d9e653f"
        );
        let tx = MultiEraTx::decode_for_era(to_pallas_era(era), &raw_tx).unwrap();
        let raw_tx = tx.encode();
        let mapped_tx = acropolis_codec::map_transaction(
            &tx,
            &raw_tx,
            TxIdentifier::default(),
            NetworkId::Mainnet,
            to_era(era),
        );
        let tx_deltas = mapped_tx.convert_to_utxo_deltas(true);

        let genesis_values = GenesisValues::mainnet();
        let tx_info = TxInfo::new(&tx_deltas, &ctx.utxos, &genesis_values).unwrap();
        let scripts_needed = crate::utils::get_scripts_needed(&tx_deltas, &ctx.utxos);
        let scripts_provided = crate::utils::get_scripts_provided(&tx_deltas, &ctx.utxos);
        let script_contexts =
            build_script_contexts(&tx_info, &scripts_needed, &scripts_provided).unwrap();

        // Empty scripts table - no scripts available
        let empty_table = HashMap::new();
        let cost_models = ctx.protocol_params.cost_models();

        let result = evaluate_scripts(
            &tx_info,
            &script_contexts,
            &empty_table,
            &cost_models,
            tx_deltas.is_valid,
        );

        assert!(
            matches!(result, Err(Phase2ValidationError::MissingScriptForHash(_))),
            "expected MissingScriptForHash, got: {result:?}"
        );
    }
}
