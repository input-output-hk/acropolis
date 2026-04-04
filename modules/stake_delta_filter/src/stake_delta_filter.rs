//! Acropolis Stake Delta Filter module
//! Reads address deltas and filters out only stake addresses from it; also resolves pointer addresses.

use acropolis_common::{
    caryatid::{PrimaryRead, RollbackWrapper, ValidationContext},
    configuration::StartupMode,
    declare_cardano_reader,
    messages::{
        AddressDeltasMessage, CardanoMessage, Message, StateQuery, StateQueryResponse,
        StateTransitionMessage, TxCertificatesMessage,
    },
    params::SECURITY_PARAMETER_K,
    queries::{
        errors::QueryError,
        stake_deltas::{
            StakeDeltaQuery, StakeDeltaQueryResponse, DEFAULT_STAKE_DELTAS_QUERY_TOPIC,
        },
    },
    state_history::{StateHistory, StateHistoryStore, DEFAULT_DUMP_INDEX},
    BlockStatus, NetworkId, StakeAddress,
};
use anyhow::{anyhow, bail, Result};
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::Path,
    sync::Arc,
};
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

declare_cardano_reader!(
    AddressDeltasReader,
    "subscription-address-delta-topic",
    "cardano.address.deltas",
    AddressDeltas,
    AddressDeltasMessage
);
declare_cardano_reader!(
    CertsReader,
    "subscription-certificates-topic",
    "cardano.certificates",
    TxCertificates,
    TxCertificatesMessage
);

const DEFAULT_STAKE_ADDRESS_DELTA_TOPIC: (&str, &str) =
    ("publishing-stake-delta-topic", "cardano.stake.deltas");
const DEFAULT_VALIDATION_TOPIC: (&str, &str) = (
    "publishing-validation-topic",
    "cardano.validation.stake.filter",
);

/// Directory to put cached shelley address pointers into. Depending on the address
/// cache mode, these cached pointers can be used instead of tracking current pointer
/// values in blockchain (which can be quite resource-consuming).
const DEFAULT_CACHE_DIR: (&str, &str) = ("cache-dir", "cache");

/// Cache mode: use built-in; always build cache; always read cache (error if missing);
/// build if missing, read otherwise.
const DEFAULT_CACHE_MODE: (&str, CacheMode) = ("cache-mode", CacheMode::Predefined);

/// Cache remembers all stake addresses that could potentially be referenced by pointers. However
/// only a few addressed are actually referenced by pointers in real blockchain.
/// `true` means that all possible addresses should be written to disk (as potential pointers).
/// `false` means that only addresses used in actual pointers should be written to disk.
const DEFAULT_WRITE_FULL_CACHE: (&str, bool) = ("write-full-cache", false);

/// Network: currently only Main/Test. Parameter is necessary to distinguish caches.
const DEFAULT_NETWORK: (&str, NetworkId) = ("network", NetworkId::Mainnet);

/// Stake Delta Filter module
#[module(
    message_type(Message),
    name = "stake-delta-filter",
    description = "Retrieves stake addresses from address deltas"
)]
pub struct StakeDeltaFilter;

mod predefined;
mod state;
mod utils;

use state::{DeltaPublisher, State};
use utils::{process_message, CacheMode, PointerCache, Tracker};

const REPLAY_AUDIT_PUBLISH_LIMIT: usize = 256;
const ROLLBACK_WINDOW_ARTIFACT: &str = "stake_delta_filter.rollback_window";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct StakeDeltaBlockSnapshot {
    block_number: u64,
    status: BlockStatus,
    deltas: Vec<(StakeAddress, i64)>,
}

enum BlockMismatch {
    MissingFromCurrent {
        baseline: StakeDeltaBlockSnapshot,
    },
    MissingFromBaseline {
        current: StakeDeltaBlockSnapshot,
    },
    Different {
        baseline: StakeDeltaBlockSnapshot,
        current: StakeDeltaBlockSnapshot,
    },
}

#[derive(Default)]
struct RollbackValidationWindow {
    dump_index: Option<u64>,
    rolled_back: bool,
    restore_from: Option<u64>,
    recent_blocks: VecDeque<StakeDeltaBlockSnapshot>,
}

#[derive(Default)]
struct StatelessReplayAudit {
    active: bool,
    last_rollback_block: Option<u64>,
    published_blocks: usize,
    replayed_blocks: usize,
    net_deltas: HashMap<StakeAddress, i128>,
}

impl StatelessReplayAudit {
    fn observe_rollback(&mut self, primary: &PrimaryRead<AddressDeltasMessage>) {
        let block_info = primary.block_info();

        if self.active {
            self.log_summary("replaced by newer rollback", Some(block_info.number));
        }

        self.active = true;
        self.last_rollback_block = Some(block_info.number);
        self.published_blocks = 0;
        self.replayed_blocks = 0;
        self.net_deltas.clear();

        info!(
            rollback_block = block_info.number,
            slot = block_info.slot,
            "stake delta filter observed rollback marker"
        );
    }

    fn observe_publish(
        &mut self,
        primary: &PrimaryRead<AddressDeltasMessage>,
        delta_count: usize,
        positive_delta_total: i128,
        negative_delta_total: i128,
        deltas: &[(StakeAddress, i128)],
    ) {
        let block_info = primary.block_info();
        let is_replayed_block = block_info.status == BlockStatus::RolledBack;

        if self.active && self.published_blocks < 10 || is_replayed_block {
            info!(
                rollback_block = self.last_rollback_block,
                block = block_info.number,
                slot = block_info.slot,
                status = ?block_info.status,
                delta_count,
                positive_delta_total,
                negative_delta_total,
                "stake delta filter published stake deltas"
            );
        }

        if !self.active {
            return;
        }

        self.published_blocks += 1;
        if is_replayed_block {
            self.replayed_blocks += 1;
        }

        for (stake_address, delta) in deltas {
            *self.net_deltas.entry(stake_address.clone()).or_default() += *delta;
        }

        if self.published_blocks >= REPLAY_AUDIT_PUBLISH_LIMIT {
            self.log_summary("reached publish limit", Some(block_info.number));
        }
    }

    fn log_summary(&mut self, reason: &str, observed_block: Option<u64>) {
        let positive_delta_total: i128 =
            self.net_deltas.values().copied().filter(|delta| *delta > 0).sum();
        let negative_delta_total: i128 =
            self.net_deltas.values().copied().filter(|delta| *delta < 0).sum();

        info!(
            rollback_block = self.last_rollback_block,
            observed_block,
            published_blocks = self.published_blocks,
            replayed_blocks = self.replayed_blocks,
            differing_addresses = self.net_deltas.len(),
            positive_delta_total,
            negative_delta_total,
            reason,
            "stake delta filter replay audit summary"
        );

        let mut top_deltas: Vec<_> = self
            .net_deltas
            .iter()
            .filter(|(_, delta)| **delta != 0)
            .map(|(stake_address, delta)| (stake_address, *delta))
            .collect();
        top_deltas.sort_by(|(_, left), (_, right)| {
            right.abs().cmp(&left.abs()).then_with(|| right.cmp(left))
        });

        for (rank, (stake_address, replayed_delta)) in top_deltas.into_iter().take(20).enumerate() {
            info!(
                rollback_block = self.last_rollback_block,
                observed_block,
                rank = rank + 1,
                stake_address = %stake_address,
                replayed_delta,
                "stake delta filter replay audit diff"
            );
        }

        self.active = false;
        self.last_rollback_block = None;
        self.published_blocks = 0;
        self.replayed_blocks = 0;
        self.net_deltas.clear();
    }
}

impl RollbackValidationWindow {
    fn new(dump_index: Option<u64>) -> Self {
        Self {
            dump_index,
            rolled_back: false,
            restore_from: None,
            recent_blocks: VecDeque::new(),
        }
    }

    fn observe_rollback(&mut self, rollback_block: u64) {
        self.rolled_back = true;
        self.restore_from = Some(rollback_block.saturating_add(1));
    }

    fn observe_block(
        &mut self,
        primary: &PrimaryRead<AddressDeltasMessage>,
        deltas: &[(StakeAddress, i128)],
    ) {
        let Some(dump_index) = self.dump_index else {
            return;
        };

        let mut sorted_deltas: Vec<_> = deltas
            .iter()
            .map(|(stake_address, delta)| {
                (
                    stake_address.clone(),
                    i64::try_from(*delta).expect("stake delta snapshot value should fit in i64"),
                )
            })
            .collect();
        sorted_deltas.sort_by(|(left_addr, left_delta), (right_addr, right_delta)| {
            left_addr.cmp(right_addr).then_with(|| left_delta.cmp(right_delta))
        });

        self.recent_blocks.push_back(StakeDeltaBlockSnapshot {
            block_number: primary.block_info().number,
            status: primary.block_info().status.clone(),
            deltas: sorted_deltas,
        });
        while self.recent_blocks.len() > SECURITY_PARAMETER_K as usize {
            self.recent_blocks.pop_front();
        }

        if primary.block_info().number != dump_index {
            return;
        }

        if self.rolled_back && Path::new(ROLLBACK_WINDOW_ARTIFACT).exists() {
            self.compare_against_baseline();
        } else {
            if self.rolled_back {
                error!(
                    dump_index,
                    "stake delta filter rollback baseline missing; dumping current rollback window"
                );
            }
            self.dump_baseline();
        }
    }

    fn dump_baseline(&self) {
        match serde_json::to_vec(&self.recent_blocks) {
            Ok(bytes) => {
                if let Err(error) = fs::write(ROLLBACK_WINDOW_ARTIFACT, bytes) {
                    error!(
                        artifact = ROLLBACK_WINDOW_ARTIFACT,
                        "failed to dump stake delta filter rollback window: {error}"
                    );
                } else {
                    info!(
                        artifact = ROLLBACK_WINDOW_ARTIFACT,
                        blocks = self.recent_blocks.len(),
                        "stake delta filter dumped rollback validation window"
                    );
                }
            }
            Err(error) => {
                error!("failed to serialize stake delta filter rollback window: {error}");
            }
        }
    }

    fn compare_against_baseline(&self) {
        let baseline_bytes = match fs::read(ROLLBACK_WINDOW_ARTIFACT) {
            Ok(bytes) => bytes,
            Err(error) => {
                error!(
                    artifact = ROLLBACK_WINDOW_ARTIFACT,
                    "failed to read stake delta filter rollback baseline: {error}"
                );
                return;
            }
        };

        let baseline: VecDeque<StakeDeltaBlockSnapshot> =
            match serde_json::from_slice(&baseline_bytes) {
                Ok(window) => window,
                Err(error) => {
                    error!(
                        artifact = ROLLBACK_WINDOW_ARTIFACT,
                        "failed to decode stake delta filter rollback baseline: {error}"
                    );
                    return;
                }
            };

        let Some(dump_index) = self.dump_index else {
            return;
        };

        let baseline_view = self.filtered_window(&baseline, dump_index);
        let current_view = self.filtered_window(&self.recent_blocks, dump_index);

        if baseline_view == current_view {
            info!("stake delta filter rollback validation success");
            return;
        }

        error!(
            restore_from = self.restore_from,
            dump_index,
            baseline_blocks = baseline_view.len(),
            current_blocks = current_view.len(),
            "stake delta filter rollback validation failed"
        );

        for (rank, mismatch) in
            Self::collect_mismatches(&baseline_view, &current_view).into_iter().take(10).enumerate()
        {
            match mismatch {
                BlockMismatch::MissingFromCurrent { baseline } => {
                    error!(
                        rank = rank + 1,
                        block = baseline.block_number,
                        status = ?baseline.status,
                        baseline_delta_count = baseline.deltas.len(),
                        "stake delta filter rollback mismatch: missing block from current window"
                    );
                }
                BlockMismatch::MissingFromBaseline { current } => {
                    error!(
                        rank = rank + 1,
                        block = current.block_number,
                        status = ?current.status,
                        current_delta_count = current.deltas.len(),
                        "stake delta filter rollback mismatch: missing block from baseline window"
                    );
                }
                BlockMismatch::Different { baseline, current } => {
                    let (positive_baseline, negative_baseline) =
                        Self::delta_totals(&baseline.deltas);
                    let (positive_current, negative_current) = Self::delta_totals(&current.deltas);
                    error!(
                        rank = rank + 1,
                        block = baseline.block_number,
                        baseline_status = ?baseline.status,
                        current_status = ?current.status,
                        baseline_delta_count = baseline.deltas.len(),
                        current_delta_count = current.deltas.len(),
                        positive_baseline,
                        negative_baseline,
                        positive_current,
                        negative_current,
                        "stake delta filter rollback mismatch: block payload differs"
                    );

                    for (diff_rank, (stake_address, baseline_delta, current_delta, delta_diff)) in
                        Self::top_delta_diffs(&baseline.deltas, &current.deltas)
                            .into_iter()
                            .take(10)
                            .enumerate()
                    {
                        error!(
                            block = baseline.block_number,
                            diff_rank = diff_rank + 1,
                            stake_address = %stake_address,
                            baseline_delta,
                            current_delta,
                            delta_diff,
                            "stake delta filter rollback mismatch diff"
                        );
                    }
                }
            }
        }
    }

    fn filtered_window(
        &self,
        blocks: &VecDeque<StakeDeltaBlockSnapshot>,
        dump_index: u64,
    ) -> VecDeque<StakeDeltaBlockSnapshot> {
        let restore_from = self.restore_from.unwrap_or(0);
        blocks
            .iter()
            .filter(|snapshot| {
                snapshot.block_number >= restore_from && snapshot.block_number <= dump_index
            })
            .cloned()
            .collect()
    }

    fn collect_mismatches(
        baseline: &VecDeque<StakeDeltaBlockSnapshot>,
        current: &VecDeque<StakeDeltaBlockSnapshot>,
    ) -> Vec<BlockMismatch> {
        let mut baseline_by_block: HashMap<u64, &StakeDeltaBlockSnapshot> =
            baseline.iter().map(|snapshot| (snapshot.block_number, snapshot)).collect();
        let mut current_by_block: HashMap<u64, &StakeDeltaBlockSnapshot> =
            current.iter().map(|snapshot| (snapshot.block_number, snapshot)).collect();

        let mut block_numbers: Vec<u64> =
            baseline_by_block.keys().chain(current_by_block.keys()).copied().collect();
        block_numbers.sort_unstable();
        block_numbers.dedup();

        let mut mismatches = Vec::new();
        for block_number in block_numbers {
            match (
                baseline_by_block.remove(&block_number),
                current_by_block.remove(&block_number),
            ) {
                (Some(baseline), Some(current)) if baseline != current => {
                    mismatches.push(BlockMismatch::Different {
                        baseline: baseline.clone(),
                        current: current.clone(),
                    });
                }
                (Some(baseline), None) => mismatches.push(BlockMismatch::MissingFromCurrent {
                    baseline: baseline.clone(),
                }),
                (None, Some(current)) => mismatches.push(BlockMismatch::MissingFromBaseline {
                    current: current.clone(),
                }),
                _ => {}
            }
        }

        mismatches
    }

    fn delta_totals(deltas: &[(StakeAddress, i64)]) -> (i128, i128) {
        let positive =
            deltas.iter().map(|(_, delta)| i128::from(*delta)).filter(|delta| *delta > 0).sum();
        let negative =
            deltas.iter().map(|(_, delta)| i128::from(*delta)).filter(|delta| *delta < 0).sum();
        (positive, negative)
    }

    fn top_delta_diffs(
        baseline: &[(StakeAddress, i64)],
        current: &[(StakeAddress, i64)],
    ) -> Vec<(StakeAddress, i64, i64, i128)> {
        let mut merged: HashMap<StakeAddress, (i64, i64)> = HashMap::new();
        for (stake_address, delta) in baseline {
            merged.entry(stake_address.clone()).or_default().0 = *delta;
        }
        for (stake_address, delta) in current {
            merged.entry(stake_address.clone()).or_default().1 = *delta;
        }

        let mut diffs: Vec<_> = merged
            .into_iter()
            .filter_map(|(stake_address, (baseline_delta, current_delta))| {
                let diff = i128::from(current_delta) - i128::from(baseline_delta);
                (diff != 0).then_some((stake_address, baseline_delta, current_delta, diff))
            })
            .collect();
        diffs.sort_by(|(_, _, _, left), (_, _, _, right)| {
            right.abs().cmp(&left.abs()).then_with(|| right.cmp(left))
        });
        diffs
    }
}

#[derive(Clone, Debug, Default)]
struct StakeDeltaFilterParams {
    stake_address_delta_topic: String,
    validation_topic: String,
    network: NetworkId,

    cache_dir: String,
    cache_mode: CacheMode,
    write_full_cache: bool,
    dump_index: Option<u64>,
}

impl StakeDeltaFilterParams {
    fn get_cache_file_name(&self, modifier: &str) -> Result<String> {
        let path = Path::new(&self.cache_dir);
        let full = path.join(format!("{}{}", self.get_network_name(), modifier).to_lowercase());
        let str =
            full.to_str().ok_or_else(|| anyhow!("Cannot produce cache file name".to_string()))?;
        Ok(str.to_string())
    }

    fn get_network_name(&self) -> String {
        format!("{:?}", self.network)
    }

    fn conf(config: &Arc<Config>, keydef: (&str, &str)) -> String {
        config.get_string(keydef.0).unwrap_or(keydef.1.to_string())
    }

    fn conf_enum<'a, T: Deserialize<'a>>(config: &Arc<Config>, keydef: (&str, T)) -> Result<T> {
        if config.get_string(keydef.0).is_ok() {
            config.get::<T>(keydef.0).map_err(|e| anyhow!("cannot parse {} value: {e}", keydef.0))
        } else {
            Ok(keydef.1)
        }
    }

    fn init(cfg: Arc<Config>) -> Result<Arc<Self>> {
        let params = Self {
            stake_address_delta_topic: Self::conf(&cfg, DEFAULT_STAKE_ADDRESS_DELTA_TOPIC),
            validation_topic: Self::conf(&cfg, DEFAULT_VALIDATION_TOPIC),
            cache_dir: Self::conf(&cfg, DEFAULT_CACHE_DIR),
            cache_mode: Self::conf_enum::<CacheMode>(&cfg, DEFAULT_CACHE_MODE)?,
            write_full_cache: Self::conf_enum::<bool>(&cfg, DEFAULT_WRITE_FULL_CACHE)?,
            network: Self::conf_enum::<NetworkId>(&cfg, DEFAULT_NETWORK)?,
            dump_index: cfg.get::<u64>(DEFAULT_DUMP_INDEX).ok(),
        };

        info!("Cache mode {:?}", params.cache_mode);
        if params.cache_mode == CacheMode::Read {
            if !Path::new(&params.cache_dir).try_exists()? {
                return Err(anyhow!(
                    "Pointer cache directory '{}' does not exist.",
                    params.cache_dir
                ));
            }
            info!("Reading (writing) caches from (to) {}", params.cache_dir);
        } else if params.cache_mode != CacheMode::Predefined {
            std::fs::create_dir_all(&params.cache_dir)?;
        }

        Ok(Arc::new(params))
    }
}

impl StakeDeltaFilter {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let address_delta_reader = AddressDeltasReader::new(&context, &config).await?;
        let params = StakeDeltaFilterParams::init(config.clone())?;
        let is_snapshot_mode = StartupMode::from_config(config.as_ref()).is_snapshot();
        let cache_path = params.get_cache_file_name(".json")?;
        let publisher = DeltaPublisher::new(context.clone(), params.clone());

        match params.cache_mode {
            CacheMode::Predefined => {
                Self::stateless_init(
                    PointerCache::try_load_predefined(&params.get_network_name())?,
                    context,
                    publisher,
                    address_delta_reader,
                    is_snapshot_mode,
                    params.dump_index,
                )
                .await
            }

            CacheMode::Read => {
                Self::stateless_init(
                    PointerCache::try_load(&cache_path)?,
                    context,
                    publisher,
                    address_delta_reader,
                    is_snapshot_mode,
                    params.dump_index,
                )
                .await
            }

            CacheMode::WriteIfAbsent => match PointerCache::try_load(&cache_path) {
                Ok(cache) => {
                    Self::stateless_init(
                        cache,
                        context,
                        publisher,
                        address_delta_reader,
                        is_snapshot_mode,
                        params.dump_index,
                    )
                    .await
                }
                Err(e) => {
                    info!("Cannot load cache: {}, building from scratch", e);
                    let certs_reader = CertsReader::new(&context, &config).await?;
                    Self::stateful_init(
                        params,
                        context,
                        certs_reader,
                        address_delta_reader,
                        publisher,
                        is_snapshot_mode,
                    )
                    .await
                }
            },

            CacheMode::Write => {
                let certs_reader = CertsReader::new(&context, &config).await?;
                Self::stateful_init(
                    params,
                    context,
                    certs_reader,
                    address_delta_reader,
                    publisher,
                    is_snapshot_mode,
                )
                .await
            }
        }
    }

    /// Register a query handler that resolves pointer addresses using the given cache.
    fn register_query_handler(
        context: &Arc<Context<Message>>,
        config: &Arc<Config>,
        cache: Arc<PointerCache>,
    ) {
        let query_topic = config
            .get_string(DEFAULT_STAKE_DELTAS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_STAKE_DELTAS_QUERY_TOPIC.1.to_string());
        info!("Registering query handler on '{query_topic}'");

        context.handle(&query_topic, move |message| {
            let cache = cache.clone();
            async move {
                let Message::StateQuery(StateQuery::StakeDeltas(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(
                        StateQueryResponse::StakeDeltas(StakeDeltaQueryResponse::Error(
                            QueryError::internal_error("Invalid message for stake-delta-filter"),
                        )),
                    ));
                };

                let response = match query {
                    StakeDeltaQuery::ResolvePointers { pointers } => {
                        let mut resolved = std::collections::HashMap::new();
                        for ptr in pointers {
                            if let Some(Some(stake_addr)) = cache.decode_pointer(ptr) {
                                resolved.insert(ptr.clone(), stake_addr.clone());
                            }
                        }
                        StakeDeltaQueryResponse::ResolvedPointers(resolved)
                    }
                };

                Arc::new(Message::StateQueryResponse(
                    StateQueryResponse::StakeDeltas(response),
                ))
            }
        });
    }

    async fn stateless_init(
        cache: Arc<PointerCache>,
        context: Arc<Context<Message>>,
        publisher: DeltaPublisher,
        address_delta_reader: AddressDeltasReader,
        is_snapshot_mode: bool,
        dump_index: Option<u64>,
    ) -> Result<()> {
        info!("Stateless init: using stake pointer cache");

        // Register query handler for pointer resolution
        Self::register_query_handler(&context, &context.config, cache.clone());

        context.clone().run(Self::stateless_run(
            cache,
            publisher,
            address_delta_reader,
            is_snapshot_mode,
            dump_index,
        ));

        Ok(())
    }

    async fn stateless_run(
        cache: Arc<PointerCache>,
        mut publisher: DeltaPublisher,
        mut address_delta_reader: AddressDeltasReader,
        is_snapshot_mode: bool,
        dump_index: Option<u64>,
    ) -> Result<()> {
        let mut replay_audit = StatelessReplayAudit::default();
        let mut rollback_validation = RollbackValidationWindow::new(dump_index);

        if !is_snapshot_mode {
            match address_delta_reader.read_with_rollbacks().await? {
                RollbackWrapper::Normal(_) => {}
                RollbackWrapper::Rollback(_) => {
                    bail!("Unexpected rollback while reading initial deltas message");
                }
            }
        }
        loop {
            let primary = PrimaryRead::from_read(address_delta_reader.read_with_rollbacks().await?);

            if let Some(address_deltas) = primary.message() {
                let msg = process_message(&cache, address_deltas, primary.block_info(), None);
                let deltas: Vec<(StakeAddress, i128)> = msg
                    .deltas
                    .iter()
                    .map(|delta| (delta.stake_address.clone(), i128::from(delta.delta)))
                    .collect();
                let positive_delta_total: i128 = msg
                    .deltas
                    .iter()
                    .map(|delta| i128::from(delta.delta))
                    .filter(|delta| *delta > 0)
                    .sum();
                let negative_delta_total: i128 = msg
                    .deltas
                    .iter()
                    .map(|delta| i128::from(delta.delta))
                    .filter(|delta| *delta < 0)
                    .sum();
                replay_audit.observe_publish(
                    &primary,
                    msg.deltas.len(),
                    positive_delta_total,
                    negative_delta_total,
                    &deltas,
                );
                rollback_validation.observe_block(&primary, &deltas);
                publisher
                    .publish(primary.block_info(), msg)
                    .await
                    .unwrap_or_else(|e| error!("Publish error: {e}"))
            } else if let Some(message) = primary.rollback_message() {
                replay_audit.observe_rollback(&primary);
                rollback_validation.observe_rollback(primary.block_info().number);
                // Publish rollbacks downstream
                publisher
                    .publish_message(message.clone())
                    .await
                    .unwrap_or_else(|e| error!("Publish error: {e}"));
            }
        }
    }

    /// Register a query handler for stateful mode, where the cache is behind a Mutex.
    fn register_query_handler_stateful(
        context: &Arc<Context<Message>>,
        config: &Arc<Config>,
        history: Arc<Mutex<StateHistory<State>>>,
    ) {
        let query_topic = config
            .get_string(DEFAULT_STAKE_DELTAS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_STAKE_DELTAS_QUERY_TOPIC.1.to_string());
        info!("Registering stateful query handler on '{query_topic}'");

        context.handle(&query_topic, move |message| {
            let history = history.clone();
            async move {
                let Message::StateQuery(StateQuery::StakeDeltas(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(
                        StateQueryResponse::StakeDeltas(StakeDeltaQueryResponse::Error(
                            QueryError::internal_error("Invalid message for stake-delta-filter"),
                        )),
                    ));
                };

                let locked = history.lock().await;
                let state = match locked.current() {
                    Some(state) => state,
                    None => {
                        return Arc::new(Message::StateQueryResponse(
                            StateQueryResponse::StakeDeltas(StakeDeltaQueryResponse::Error(
                                QueryError::internal_error(
                                    "Invalid message for stake-delta-filter",
                                ),
                            )),
                        ))
                    }
                };

                let response = match query {
                    StakeDeltaQuery::ResolvePointers { pointers } => {
                        let mut resolved = std::collections::HashMap::new();
                        for ptr in pointers {
                            if let Some(Some(stake_addr)) = state.pointer_cache.decode_pointer(ptr)
                            {
                                resolved.insert(ptr.clone(), stake_addr.clone());
                            }
                        }
                        StakeDeltaQueryResponse::ResolvedPointers(resolved)
                    }
                };

                Arc::new(Message::StateQueryResponse(
                    StateQueryResponse::StakeDeltas(response),
                ))
            }
        });
    }

    async fn stateful_init(
        params: Arc<StakeDeltaFilterParams>,
        context: Arc<Context<Message>>,
        certs_reader: CertsReader,
        address_deltas_reader: AddressDeltasReader,
        publisher: DeltaPublisher,
        is_snapshot_mode: bool,
    ) -> Result<()> {
        info!("Stateful init: creating stake pointer cache");

        // State
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "stake_delta_filter",
            StateHistoryStore::default_block_store(),
        )));
        let history_tick = history.clone();

        // Register query handler for pointer resolution (stateful)
        Self::register_query_handler_stateful(&context, &context.config, history.clone());

        let context_run = context.clone();
        context.run(Self::stateful_run(
            history,
            certs_reader,
            address_deltas_reader,
            publisher,
            params,
            context_run,
            is_snapshot_mode,
        ));

        // Ticker to log stats
        let mut subscription = context.subscribe("clock.tick").await?;
        context.run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        let span = info_span!("stake_delta_filter.tick", number = message.number);
                        async {
                            let history = history_tick.lock().await;
                            if let Some(state) = history.current() {
                                state.tick().await.inspect_err(|e| error!("Tick error: {e}")).ok();
                            }
                        }
                        .instrument(span)
                        .await;
                    }
                }
            }
        });

        Ok(())
    }

    async fn stateful_run(
        history: Arc<Mutex<StateHistory<State>>>,
        mut certs_reader: CertsReader,
        mut address_deltas_reader: AddressDeltasReader,
        mut publisher: DeltaPublisher,
        params: Arc<StakeDeltaFilterParams>,
        context: Arc<Context<Message>>,
        is_snapshot_mode: bool,
    ) -> Result<()> {
        if !is_snapshot_mode {
            match address_deltas_reader.read_with_rollbacks().await? {
                RollbackWrapper::Normal(_) => {}
                RollbackWrapper::Rollback(_) => {
                    bail!("Unexpected rollback while reading initial deltas message");
                }
            }
        }
        loop {
            let mut ctx =
                ValidationContext::new(&context, &params.validation_topic, "stake_delta_filter");

            let mut state = history.lock().await.get_or_init_with(|| State::new(params.clone()));

            let primary = PrimaryRead::from_sync(
                &mut ctx,
                "certs",
                certs_reader.read_with_rollbacks().await,
            )?;

            if let Some(tx_cert_msg) = primary.message() {
                state
                    .handle_certs(primary.block_info(), tx_cert_msg)
                    .await
                    .inspect_err(|e| error!("Messaging handling error: {e}"))
                    .ok();
            } else if let Some(message) = primary.rollback_message() {
                // Handle rollbacks on this topic only
                state = history.lock().await.get_rolled_back_state(primary.restore_from_index());

                // Publish rollbacks downstream
                publisher.publish_message(message.clone()).await?;
            }

            match ctx.consume_sync(
                "address deltas",
                address_deltas_reader.read_with_rollbacks().await,
            )? {
                RollbackWrapper::Normal((block_info, deltas)) => {
                    let msg = state.handle_deltas(&block_info, &deltas);
                    publisher.publish(&block_info, msg).await?;
                }
                RollbackWrapper::Rollback(_) => {}
            }

            if primary.message().is_some() {
                let block_info = primary.block_info();
                state.save()?;
                history.lock().await.commit(block_info.number, state);

                if primary.do_validation() {
                    ctx.publish().await;
                }
            }
        }
    }
}
