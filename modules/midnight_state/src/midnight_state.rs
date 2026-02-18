//! Acropolis Midnight state module for Caryatid
//! Indexes data required by `midnight-node`
use acropolis_common::{
    caryatid::RollbackWrapper,
    declare_cardano_reader,
    messages::{AddressDeltasMessage, CardanoMessage, Message, StateTransitionMessage},
    state_history::{StateHistory, StateHistoryStore},
    AssetName, BlockInfo, BlockStatus, PolicyId,
};
use anyhow::{anyhow, bail, Context as _, Result};
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use std::{str::FromStr, sync::Arc};
use tokio::sync::Mutex;
use tracing::{error, info, warn};
mod state;
use state::State;
mod types;

declare_cardano_reader!(
    AddressDeltasReader,
    "address-deltas-topic",
    "cardano.address.deltas.extended",
    AddressDeltas,
    AddressDeltasMessage
);

/// Midnight State module
#[module(
    message_type(Message),
    name = "midnight-state",
    description = "Midnight State Indexer"
)]

pub struct MidnightState;

impl MidnightState {
    fn required_string(config: &Config, key: &str) -> Result<String> {
        config.get_string(key).map_err(|_| anyhow!("Missing required config key '{key}'"))
    }

    fn parse_cnight_asset_name(raw: &str) -> Result<AssetName> {
        let normalized = raw.strip_prefix("0x").unwrap_or(raw);
        let bytes = match hex::decode(normalized) {
            Ok(bytes) => bytes,
            Err(_) => raw.as_bytes().to_vec(),
        };

        AssetName::new(&bytes)
            .ok_or_else(|| anyhow!("Invalid cnight-asset-name '{raw}': must decode to <= 32 bytes"))
    }

    fn parse_cnight_config(config: &Config) -> Result<(PolicyId, AssetName)> {
        let cnight_policy_id_raw = Self::required_string(config, "cnight-policy-id")?;
        let cnight_asset_name_raw = Self::required_string(config, "cnight-asset-name")?;

        let cnight_policy_id = PolicyId::from_str(&cnight_policy_id_raw)
            .with_context(|| format!("Invalid cnight-policy-id '{cnight_policy_id_raw}'"))?;
        let cnight_asset_name = Self::parse_cnight_asset_name(&cnight_asset_name_raw)?;

        Ok((cnight_policy_id, cnight_asset_name))
    }

    async fn run(
        history: Arc<Mutex<StateHistory<State>>>,
        mut address_deltas_reader: AddressDeltasReader,
        cnight_policy_id: PolicyId,
        cnight_asset_name: AssetName,
    ) -> Result<()> {
        loop {
            // Get a mutable state
            let mut state = {
                let mut h = history.lock().await;
                h.get_or_init_with(|| State::new(cnight_policy_id, cnight_asset_name))
            };

            match address_deltas_reader.read_with_rollbacks().await? {
                RollbackWrapper::Normal((blk_info, deltas)) => {
                    let (delta_kind, delta_count) = match deltas.as_ref() {
                        AddressDeltasMessage::Deltas(deltas) => ("compact", deltas.len()),
                        AddressDeltasMessage::ExtendedDeltas(deltas) => ("extended", deltas.len()),
                    };

                    if blk_info.status == BlockStatus::RolledBack {
                        warn!(
                            block_number = blk_info.number,
                            block_hash = %blk_info.hash,
                            "applying rollback"
                        );
                        state = history.lock().await.get_rolled_back_state(blk_info.number);
                    }

                    if blk_info.new_epoch {
                        info!(
                            epoch = blk_info.epoch,
                            block_number = blk_info.number,
                            era = ?blk_info.era,
                            status = ?blk_info.status,
                            delta_kind,
                            delta_count,
                            cnight_creates = state.epoch_cnight_create_count(),
                            cnight_spends = state.epoch_cnight_spend_count(),
                            tracked_utxos = state.tracked_utxo_count(),
                            "epoch checkpoint"
                        );
                        state.handle_new_epoch()?;
                    }

                    if matches!(deltas.as_ref(), AddressDeltasMessage::Deltas(_)) {
                        if blk_info.new_epoch {
                            warn!(
                                epoch = blk_info.epoch,
                                block_number = blk_info.number,
                                "received compact deltas; expected extended topic"
                            );
                        }
                        history.lock().await.commit(blk_info.number, state);
                        continue;
                    }

                    if let Err(e) = state.handle_address_deltas(&blk_info, deltas.as_ref()) {
                        error!(
                            block_number = blk_info.number,
                            block_hash = %blk_info.hash,
                            "skipped block due to delta processing error: {e}"
                        );
                        continue;
                    }

                    history.lock().await.commit(blk_info.number, state);
                }
                RollbackWrapper::Rollback(point) => {
                    warn!(
                        rollback_point = ?point,
                        "received rollback wrapper message"
                    );
                }
            };
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let (cnight_policy_id, cnight_asset_name) = Self::parse_cnight_config(&config)?;
        info!(
            cnight_policy_id = %cnight_policy_id,
            cnight_asset_name = %hex::encode(cnight_asset_name.as_slice()),
            "configured cNight filter"
        );

        // Subscribe to the `AddressDeltasMessage` publisher
        let address_deltas_reader = AddressDeltasReader::new(&context, &config).await?;

        // Initalize unbounded state history
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "midnight_state",
            StateHistoryStore::Unbounded,
        )));

        // Start the run task
        context.run(async move {
            Self::run(
                history,
                address_deltas_reader,
                cnight_policy_id,
                cnight_asset_name,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::MidnightState;
    use config::Config;

    #[test]
    fn parse_cnight_config_fails_when_keys_are_missing() {
        let config = Config::builder().build().unwrap();
        let err = MidnightState::parse_cnight_config(&config).expect_err("missing keys must fail");
        assert!(err.to_string().contains("cnight-policy-id"));
    }

    #[test]
    fn parse_cnight_config_fails_when_asset_name_is_missing() {
        let config = Config::builder()
            .set_override(
                "cnight-policy-id",
                "00000000000000000000000000000000000000000000000000000000",
            )
            .unwrap()
            .build()
            .unwrap();
        let err = MidnightState::parse_cnight_config(&config).expect_err("missing keys must fail");
        assert!(err.to_string().contains("cnight-asset-name"));
    }
}
