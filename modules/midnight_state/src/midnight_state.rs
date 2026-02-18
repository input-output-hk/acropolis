//! Acropolis Midnight state module for Caryatid
//! Indexes data required by `midnight-node`
use acropolis_common::{
    BlockInfo, BlockStatus, Era,
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

#[derive(Default)]
struct EpochTotals {
    compact_blocks: usize,
    extended_blocks: usize,
    delta_count: usize,
    created_utxos: usize,
    spent_utxos: usize,
}

impl EpochTotals {
    fn observe(&mut self, deltas: &AddressDeltasMessage) {
        match deltas {
            AddressDeltasMessage::Deltas(deltas) => {
                self.compact_blocks += 1;
                self.delta_count += deltas.len();
                self.created_utxos +=
                    deltas.iter().map(|delta| delta.created_utxos.len()).sum::<usize>();
                self.spent_utxos +=
                    deltas.iter().map(|delta| delta.spent_utxos.len()).sum::<usize>();
            }
            AddressDeltasMessage::ExtendedDeltas(deltas) => {
                self.extended_blocks += 1;
                self.delta_count += deltas.len();
                self.created_utxos +=
                    deltas.iter().map(|delta| delta.created_utxos.len()).sum::<usize>();
                self.spent_utxos +=
                    deltas.iter().map(|delta| delta.spent_utxos.len()).sum::<usize>();
            }
        }
    }
}

struct EpochCheckpoint {
    epoch: u64,
    block_number: u64,
    status: BlockStatus,
    era: Era,
}

impl EpochCheckpoint {
    fn from_block(block: &BlockInfo) -> Self {
        Self {
            epoch: block.epoch,
            block_number: block.number,
            status: block.status.clone(),
            era: block.era,
        }
    }
}

impl MidnightState {
    fn log_epoch_summary(checkpoint: &EpochCheckpoint, totals: &EpochTotals) {
        info!(
            epoch = checkpoint.epoch,
            block_number = checkpoint.block_number,
            era = ?checkpoint.era,
            status = ?checkpoint.status,
            compact_blocks = totals.compact_blocks,
            extended_blocks = totals.extended_blocks,
            delta_count = totals.delta_count,
            created_utxos = totals.created_utxos,
            spent_utxos = totals.spent_utxos,
            "epoch checkpoint"
        );

        if totals.compact_blocks > 0 {
            warn!(
                epoch = checkpoint.epoch,
                compact_blocks = totals.compact_blocks,
                "received compact deltas; expected extended mode for midnight"
            );
        }
    }

    async fn run(mut address_deltas_reader: AddressDeltasReader) -> Result<()> {
        let mut current_epoch: Option<u64> = None;
        let mut totals = EpochTotals::default();
        let mut checkpoint: Option<EpochCheckpoint> = None;

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
                            if let Some(cp) = checkpoint.as_ref() {
                                Self::log_epoch_summary(cp, &totals);
                            }
                            totals = EpochTotals::default();
                        }
                    }

                    current_epoch = Some(blk_info.epoch);
                    checkpoint = Some(EpochCheckpoint::from_block(blk_info.as_ref()));
                    totals.observe(deltas.as_ref());
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
