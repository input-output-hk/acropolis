use std::sync::Arc;

use acropolis_common::{
    configuration::{get_bool_flag, get_string_flag, get_u64_flag, StartupMode},
    messages::Message,
    queries::accounts::DEFAULT_ACCOUNTS_QUERY_TOPIC,
};
use caryatid_sdk::{Context, Subscription};
use config::Config;
use tokio::sync::Mutex;

use crate::{
    drep_distribution_publisher::DRepDistributionPublisher,
    registration_updates_publisher::StakeRegistrationUpdatesPublisher,
    spo_distribution_publisher::SPODistributionPublisher,
    spo_distribution_store::{SPDDStore, SPDDStoreConfig},
    spo_rewards_publisher::SPORewardsPublisher,
    stake_reward_deltas_publisher::StakeRewardDeltasPublisher,
    verifier::Verifier,
    CertsReader, EpochActivityReader, GovOutcomesReader, GovProceduresReader, ParamsReader,
    PotsReader, SPOReader, StakeDeltasReader, WithdrawalsReader,
};

// Publishers
const DEFAULT_DREP_DISTRIBUTION_TOPIC: (&str, &str) = (
    "publish-drep-distribution-topic",
    "cardano.drep.distribution",
);
const DEFAULT_SPO_DISTRIBUTION_TOPIC: (&str, &str) =
    ("publish-spo-distribution-topic", "cardano.spo.distribution");
const DEFAULT_SPO_REWARDS_TOPIC: (&str, &str) =
    ("publish-spo-rewards-topic", "cardano.spo.rewards");
const DEFAULT_STAKE_REWARD_DELTAS_TOPIC: (&str, &str) = (
    "publish-stake-reward-deltas-topic",
    "cardano.stake.reward.deltas",
);
const DEFAULT_STAKE_REGISTRATION_UPDATES_TOPIC: (&str, &str) = (
    "publish-stake-registration-updates-topic",
    "cardano.stake.registration.updates",
);
const DEFAULT_VALIDATION_OUTCOMES_TOPIC: (&str, &str) =
    ("validation-outcomes-topic", "cardano.validation.accounts");

/// Topic for receiving bootstrap data when starting from a CBOR dump snapshot
const DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC: (&str, &str) =
    ("snapshot-subscribe-topic", "cardano.snapshot");

const DEFAULT_SPDD_DB_PATH: (&str, &str) = ("spdd-db-path", "./fjall-spdd");
const DEFAULT_SPDD_RETENTION_EPOCHS: (&str, u64) = ("spdd-retention-epochs", 0);
const DEFAULT_SPDD_CLEAR_ON_START: (&str, bool) = ("spdd-clear-on-start", true);

pub struct AccountsConfig {
    /// Readers
    pub spos_reader: SPOReader,
    pub ea_reader: EpochActivityReader,
    pub certs_reader: CertsReader,
    pub withdrawals_reader: WithdrawalsReader,
    pub pot_deltas_reader: PotsReader,
    pub stake_deltas_reader: StakeDeltasReader,
    pub governance_procedures_reader: GovProceduresReader,
    pub governance_outcomes_reader: GovOutcomesReader,
    pub params_reader: ParamsReader,
    pub snapshot_subscription: Option<Box<dyn Subscription<Message>>>,

    /// Publishers
    pub drep_publisher: DRepDistributionPublisher,
    pub spo_publisher: SPODistributionPublisher,
    pub spo_rewards_publisher: SPORewardsPublisher,
    pub stake_reward_deltas_publisher: StakeRewardDeltasPublisher,
    pub stake_registration_updates_publisher: StakeRegistrationUpdatesPublisher,

    // Miscellaneous
    pub is_snapshot_mode: bool,
    pub validation_outcomes_topic: String,
    pub verifier: Verifier,
    pub accounts_query_topic: String,
    pub spdd_store: Option<Arc<Mutex<SPDDStore>>>,
}

impl AccountsConfig {
    pub async fn init(
        context: Arc<Context<Message>>,
        config: &Arc<Config>,
    ) -> anyhow::Result<Self> {
        let mut verifier = Verifier::new();

        if let Ok(verify_pots_file) = config.get_string("verify-pots-file") {
            verifier.read_pots(&verify_pots_file);
        }
        if let Ok(verify_rewards_files) = config.get_string("verify-rewards-files") {
            verifier.set_rewards_template(&verify_rewards_files);
        }
        if let Ok(verify_spdd_files) = config.get_string("verify-spdd-files") {
            verifier.set_spdd_template(&verify_spdd_files);
        }

        let is_snapshot_mode = StartupMode::from_config(config.as_ref()).is_snapshot();
        let snapshot_subscription = match is_snapshot_mode {
            true => Some(
                context
                    .subscribe(&get_string_flag(config, DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC))
                    .await?,
            ),
            false => None,
        };

        let spdd_store_config = SPDDStoreConfig {
            path: get_string_flag(config, DEFAULT_SPDD_DB_PATH),
            retention_epochs: get_u64_flag(config, DEFAULT_SPDD_RETENTION_EPOCHS),
            clear_on_start: get_bool_flag(config, DEFAULT_SPDD_CLEAR_ON_START),
        };

        let spdd_store = match spdd_store_config.is_enabled() {
            true => Some(Arc::new(Mutex::new(SPDDStore::new(&spdd_store_config)?))),
            false => None,
        };

        Ok(AccountsConfig {
            spos_reader: SPOReader::new(&context, config).await?,
            ea_reader: EpochActivityReader::new(&context, config).await?,
            certs_reader: CertsReader::new(&context, config).await?,
            withdrawals_reader: WithdrawalsReader::new(&context, config).await?,
            pot_deltas_reader: PotsReader::new(&context, config).await?,
            stake_deltas_reader: StakeDeltasReader::new(&context, config).await?,
            governance_procedures_reader: GovProceduresReader::new(&context, config).await?,
            governance_outcomes_reader: GovOutcomesReader::new(&context, config).await?,
            params_reader: ParamsReader::new(&context, config).await?,
            drep_publisher: DRepDistributionPublisher::new(
                context.clone(),
                get_string_flag(config, DEFAULT_DREP_DISTRIBUTION_TOPIC),
            ),
            spo_publisher: SPODistributionPublisher::new(
                context.clone(),
                get_string_flag(config, DEFAULT_SPO_DISTRIBUTION_TOPIC),
            ),
            spo_rewards_publisher: SPORewardsPublisher::new(
                context.clone(),
                get_string_flag(config, DEFAULT_SPO_REWARDS_TOPIC),
            ),
            stake_reward_deltas_publisher: StakeRewardDeltasPublisher::new(
                context.clone(),
                get_string_flag(config, DEFAULT_STAKE_REWARD_DELTAS_TOPIC),
            ),
            stake_registration_updates_publisher: StakeRegistrationUpdatesPublisher::new(
                context.clone(),
                get_string_flag(config, DEFAULT_STAKE_REGISTRATION_UPDATES_TOPIC),
            ),
            is_snapshot_mode,
            validation_outcomes_topic: get_string_flag(config, DEFAULT_VALIDATION_OUTCOMES_TOPIC),
            verifier,
            accounts_query_topic: get_string_flag(config, DEFAULT_ACCOUNTS_QUERY_TOPIC),
            snapshot_subscription,
            spdd_store,
        })
    }
}
