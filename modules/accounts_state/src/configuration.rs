use std::sync::Arc;

use acropolis_common::{
    configuration::{get_string_flag, StartupMode},
    messages::Message,
    queries::accounts::DEFAULT_ACCOUNTS_QUERY_TOPIC,
};
use caryatid_sdk::Context;
use config::Config;

use crate::spo_default_vote_publisher::SPODefaultVotePublisher;
use crate::{
    drep_distribution_publisher::DRepDistributionPublisher, pots_publisher::PotsPublisher,
    registration_updates_publisher::StakeRegistrationUpdatesPublisher,
    spo_distribution_publisher::SPODistributionPublisher,
    spo_rewards_publisher::SPORewardsPublisher,
    stake_reward_deltas_publisher::StakeRewardDeltasPublisher, verifier::Verifier,
    AccountsPublishers, AccountsReaders, CertsReader, EpochActivityReader, GenesisReader,
    GovOutcomesReader, GovProceduresReader, ParamsReader, SPOReader, StakeDeltasReader,
    WithdrawalsReader,
};

// Publishers
const DEFAULT_DREP_DISTRIBUTION_TOPIC: (&str, &str) = (
    "publish-drep-distribution-topic",
    "cardano.drep.distribution",
);
const DEFAULT_SPO_DISTRIBUTION_TOPIC: (&str, &str) =
    ("publish-spo-distribution-topic", "cardano.spo.distribution");
const DEFAULT_SPO_DEFAULT_VOTE_TOPIC: (&str, &str) =
    ("publish-spo-default-vote-topic", "cardano.spo.default-vote");
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
const DEFAULT_POTS_TOPIC: (&str, &str) = ("publish-pots-topic", "cardano.pots");
const DEFAULT_VALIDATION_OUTCOMES_TOPIC: (&str, &str) =
    ("validation-outcomes-topic", "cardano.validation.accounts");

/// Topic for receiving bootstrap data when starting from a CBOR dump snapshot
const DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC: (&str, &str) =
    ("snapshot-subscribe-topic", "cardano.snapshot");

pub struct AccountsConfig {
    pub readers: AccountsReaders,
    pub publishers: AccountsPublishers,
    pub validation_outcomes_topic: String,
    pub verifier: Verifier,
    pub accounts_query_topic: String,
}

impl AccountsConfig {
    pub async fn load(
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
            verifier.set_spdd_template(&verify_spdd_files)?;
        }

        let snapshot = match StartupMode::from_config(config.as_ref()).is_snapshot() {
            true => Some(
                context
                    .subscribe(&get_string_flag(config, DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC))
                    .await?,
            ),
            false => None,
        };

        Ok(AccountsConfig {
            readers: AccountsReaders {
                genesis: GenesisReader::new(&context, config).await?,
                certs: CertsReader::new(&context, config).await?,
                withdrawals: WithdrawalsReader::new(&context, config).await?,
                stake_deltas: StakeDeltasReader::new(&context, config).await?,
                gov_procedures: GovProceduresReader::new(&context, config).await?,
                params: ParamsReader::new(&context, config).await?,
                spos: SPOReader::new(&context, config).await?,
                epoch_activity: EpochActivityReader::new(&context, config).await?,
                gov_outcomes: GovOutcomesReader::new(&context, config).await?,
                snapshot,
            },
            publishers: AccountsPublishers {
                drep_distribution: DRepDistributionPublisher::new(
                    context.clone(),
                    get_string_flag(config, DEFAULT_DREP_DISTRIBUTION_TOPIC),
                ),
                spo_distribution: SPODistributionPublisher::new(
                    context.clone(),
                    get_string_flag(config, DEFAULT_SPO_DISTRIBUTION_TOPIC),
                ),
                spo_default_vote: SPODefaultVotePublisher::new(
                    context.clone(),
                    get_string_flag(config, DEFAULT_SPO_DEFAULT_VOTE_TOPIC),
                ),
                spo_rewards: SPORewardsPublisher::new(
                    context.clone(),
                    get_string_flag(config, DEFAULT_SPO_REWARDS_TOPIC),
                ),
                stake_reward_deltas: StakeRewardDeltasPublisher::new(
                    context.clone(),
                    get_string_flag(config, DEFAULT_STAKE_REWARD_DELTAS_TOPIC),
                ),
                registration_updates: StakeRegistrationUpdatesPublisher::new(
                    context.clone(),
                    get_string_flag(config, DEFAULT_STAKE_REGISTRATION_UPDATES_TOPIC),
                ),
                pots: PotsPublisher::new(
                    context.clone(),
                    get_string_flag(config, DEFAULT_POTS_TOPIC),
                ),
            },
            validation_outcomes_topic: get_string_flag(config, DEFAULT_VALIDATION_OUTCOMES_TOPIC),
            verifier,
            accounts_query_topic: get_string_flag(config, DEFAULT_ACCOUNTS_QUERY_TOPIC),
        })
    }
}
