//! Acropolis Stake Delta Filter: State storage

use crate::StakeDeltaFilterParams;
use crate::{process_message, PointerCache, Tracker};
use acropolis_common::{
    messages::{
        AddressDeltasMessage, CardanoMessage, Message, StakeAddressDeltasMessage,
        TxCertificatesMessage,
    },
    Address, BlockInfo, ShelleyAddressPointer, TxCertificate,
};
use anyhow::Result;
use serde_with::serde_as;
use std::collections::HashMap;
use std::{fs, io::Write, sync::Arc};
use tracing::info;

#[allow(dead_code)]
#[serde_as]
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct PointerOccurrence {
    /// List of occurrences of the pointer in the blockchain
    #[serde_as(as = "Vec<(_, _)>")]
    pub occurrence: HashMap<ShelleyAddressPointer, Vec<(Option<Address>, BlockInfo, Address)>>,
}

pub struct DeltaPublisher {
    pub params: Arc<StakeDeltaFilterParams>,
}

impl DeltaPublisher {
    pub fn new(params: Arc<StakeDeltaFilterParams>) -> Self {
        Self { params }
    }

    pub async fn publish(
        &self,
        block: &BlockInfo,
        message: StakeAddressDeltasMessage,
    ) -> Result<()> {
        let packed_message = Arc::new(Message::Cardano((
            block.clone(),
            CardanoMessage::StakeAddressDeltas(message),
        )));
        let params = self.params.clone();

        params
            .context
            .message_bus
            .publish(&params.stake_address_delta_topic, packed_message)
            .await
            .unwrap_or_else(|e| tracing::error!("Failed to publish: {e}"));

        Ok(())
    }
}

pub struct State {
    pub pointer_cache: PointerCache,

    pub params: Arc<StakeDeltaFilterParams>,
    pub delta_publisher: DeltaPublisher,

    pub tracker: Tracker,
}

impl State {
    pub async fn handle_deltas(
        &mut self,
        block: &BlockInfo,
        delta: &AddressDeltasMessage,
    ) -> Result<()> {
        let msg = process_message(&self.pointer_cache, delta, block, Some(&mut self.tracker));

        // Updating block number in pointer cache: looking for Conway epoch start.
        self.pointer_cache.update_block(block);
        self.delta_publisher.publish(block, msg).await?;
        Ok(())
    }

    pub async fn handle_certs(
        &mut self,
        block: &BlockInfo,
        msg: &TxCertificatesMessage,
    ) -> Result<()> {
        for cert in msg.certificates.iter() {
            match cert {
                TxCertificate::StakeRegistration(reg) => {
                    let ptr = ShelleyAddressPointer {
                        slot: block.slot,
                        tx_index: reg.tx_index,
                        cert_index: reg.cert_index,
                    };

                    let stake_address = reg.stake_address.clone();

                    // Sets pointer; updates max processed slot
                    self.pointer_cache.set_pointer(ptr, stake_address, block.slot);
                }
                _ => (),
            }
        }
        Ok(())
    }

    pub fn new(params: Arc<StakeDeltaFilterParams>) -> Self {
        Self {
            pointer_cache: PointerCache::new(),
            params: params.clone(),
            delta_publisher: DeltaPublisher::new(params.clone()),
            tracker: Tracker::new(),
        }
    }

    pub fn info(&self) {
        info!(
            "pointer cache size: {}, max slot: {}",
            self.pointer_cache.pointer_map.len(),
            self.pointer_cache.max_slot
        );
        self.tracker.info();
    }

    pub fn save(&mut self) -> Result<()> {
        let used_pointers = self.tracker.get_used_pointers();

        if self.params.write_full_cache {
            self.pointer_cache.try_save(&self.params.get_cache_file_name(".json")?)?;
        } else {
            self.pointer_cache
                .try_save_filtered(&self.params.get_cache_file_name("")?, &used_pointers)?;
        }

        let mut file = fs::File::create(self.params.get_cache_file_name(".track.log")?)?;
        file.write_all(self.tracker.report().as_bytes())?;

        Ok(())
    }

    pub async fn tick(&mut self) -> Result<()> {
        self.info();
        self.save()?;
        Ok(())
    }
}
