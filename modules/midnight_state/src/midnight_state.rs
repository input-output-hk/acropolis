//! Acropolis Midnight state module for Caryatid
//! Indexes data required by `midnight-node`
use acropolis_common::{
    BlockInfo, BlockStatus,
    caryatid::RollbackWrapper,
    declare_cardano_reader,
    messages::{AddressDeltasMessage, CardanoMessage, Message, StateTransitionMessage},
};
use anyhow::{Result, bail};
use caryatid_sdk::{Context, Subscription, module};
use config::Config;
use std::sync::Arc;
use tracing::{error, info, warn};

declare_cardano_reader!(
    AddressDeltasReader,
    "address-deltas-topic",
    "cardano.address.deltas",
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
    fn summarise_deltas(deltas: &AddressDeltasMessage) -> (bool, usize, usize, usize) {
        match deltas {
            AddressDeltasMessage::Deltas(deltas) => {
                let created: usize = deltas.iter().map(|delta| delta.created_utxos.len()).sum();
                let spent: usize = deltas.iter().map(|delta| delta.spent_utxos.len()).sum();
                (true, deltas.len(), created, spent)
            }
            AddressDeltasMessage::ExtendedDeltas(deltas) => {
                let created: usize = deltas.iter().map(|delta| delta.created_utxos.len()).sum();
                let spent: usize = deltas.iter().map(|delta| delta.spent_utxos.len()).sum();
                (false, deltas.len(), created, spent)
            }
        }
    }

    fn log_epoch_summary(
        epoch: u64,
        last_block_number: u64,
        last_status: BlockStatus,
        last_era: &str,
        compact_blocks: usize,
        extended_blocks: usize,
        delta_count: usize,
        created_utxos: usize,
        spent_utxos: usize,
    ) {
        info!(
            epoch,
            block_number = last_block_number,
            era = last_era,
            status = ?last_status,
            compact_blocks,
            extended_blocks,
            delta_count,
            created_utxos,
            spent_utxos,
            "epoch checkpoint"
        );

        if compact_blocks > 0 {
            warn!(
                epoch,
                compact_blocks, "received compact deltas; expected extended mode for midnight"
            );
        }
    }

    async fn run(mut address_deltas_reader: AddressDeltasReader) -> Result<()> {
        let mut current_epoch: Option<u64> = None;
        let mut compact_blocks = 0usize;
        let mut extended_blocks = 0usize;
        let mut total_delta_count = 0usize;
        let mut total_created_utxos = 0usize;
        let mut total_spent_utxos = 0usize;
        let mut last_block_number = 0u64;
        let mut last_status = BlockStatus::Bootstrap;
        let mut last_era = "Byron".to_string();

        loop {
            match address_deltas_reader.read_with_rollbacks().await? {
                RollbackWrapper::Normal((blk_info, deltas)) => {
                    if blk_info.status == BlockStatus::RolledBack {
                        warn!(
                            block_number = blk_info.number,
                            block_hash = %blk_info.hash,
                            "applying rollback"
                        );
                    }

                    if let Some(epoch) = current_epoch {
                        if blk_info.epoch != epoch {
                            Self::log_epoch_summary(
                                epoch,
                                last_block_number,
                                last_status,
                                last_era.as_str(),
                                compact_blocks,
                                extended_blocks,
                                total_delta_count,
                                total_created_utxos,
                                total_spent_utxos,
                            );

                            compact_blocks = 0;
                            extended_blocks = 0;
                            total_delta_count = 0;
                            total_created_utxos = 0;
                            total_spent_utxos = 0;
                        }
                    }

                    current_epoch = Some(blk_info.epoch);
                    last_block_number = blk_info.number;
                    last_status = blk_info.status.clone();
                    last_era = format!("{:?}", blk_info.era);

                    let (is_compact, delta_count, created_utxos, spent_utxos) =
                        Self::summarise_deltas(deltas.as_ref());
                    if is_compact {
                        compact_blocks += 1;
                    } else {
                        extended_blocks += 1;
                    }
                    total_delta_count += delta_count;
                    total_created_utxos += created_utxos;
                    total_spent_utxos += spent_utxos;
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
        // Subscribe to the `AddressDeltasMessage` publisher
        let address_deltas_reader = AddressDeltasReader::new(&context, &config).await?;

        // Start the run task
        context.run(async move {
            Self::run(address_deltas_reader).await.unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
