//! Acropolis Stake Delta Filter: State storage

use std::{collections::HashMap, fs, io::Write, sync::Arc};
use acropolis_common::{
    messages::{AddressDeltasMessage, Message, CardanoMessage,
               StakeAddressDeltasMessage, TxCertificatesMessage},
    Address, BlockInfo, ShelleyAddressPointer, StakeAddress,
    StakeAddressPayload, StakeCredential, TxCertificate
};
use anyhow::Result;
use serde_with::serde_as;
use tracing::info;
use crate::StakeDeltaFilterParams;
use crate::{PointerCache, Tracker, process_message};

#[serde_as]
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct PointerOccurrence {
    /// List of occurrences of the pointer in the blockchain
    #[serde_as(as = "Vec<(_, _)>")]
    pub occurrence: HashMap<ShelleyAddressPointer, Vec<(Option<Address>, BlockInfo, Address)>>
}

pub struct DeltaPublisher {
    pub params: Arc<StakeDeltaFilterParams>
}

impl DeltaPublisher {
    pub fn new (params: Arc<StakeDeltaFilterParams>) -> Self { Self { params } }

    pub async fn publish(&self, block: &BlockInfo, message: StakeAddressDeltasMessage)
                         -> Result<()> {
        let packed_message = Arc::new(Message::Cardano((
            block.clone(),
            CardanoMessage::StakeAddressDeltas(message)
        )));
        let params = self.params.clone();

        tokio::spawn(async move {
            params.context.message_bus
                .publish(&params.stake_address_delta_topic, packed_message).await
                .unwrap_or_else(|e| tracing::error!("Failed to publish: {e}")); 
        });
        Ok(())
    }
}

pub struct State {
    pub pointer_cache: PointerCache,

    pub params: Arc<StakeDeltaFilterParams>,
    pub delta_publisher: DeltaPublisher,

    pub tracker: Tracker
}

impl State {

    pub async fn handle_deltas(&mut self, block: &BlockInfo,
                               delta: &AddressDeltasMessage) -> Result<()> {

        let msg = process_message(&self.pointer_cache, delta, block, Some(&mut self.tracker));
        self.delta_publisher.publish(block, msg).await?;
        Ok(())
    }

    pub async fn handle_certs(&mut self, block: &BlockInfo, msg: &TxCertificatesMessage)
                              -> Result<()> {
        for cert in msg.certificates.iter() {
            match cert {
                TxCertificate::StakeRegistration(reg) => {
                    let ptr = ShelleyAddressPointer {
                        slot: block.slot,
                        tx_index: reg.tx_index,
                        cert_index: reg.cert_index,
                    };

                    let stake_address = StakeAddress{
                        network: self.params.network.clone(),
                        payload: match &reg.stake_credential {
                            StakeCredential::ScriptHash(h) => 
                                StakeAddressPayload::ScriptHash(h.clone()),
                            StakeCredential::AddrKeyHash(k) => 
                                StakeAddressPayload::StakeKeyHash(k.clone())
                        }
                    };

                    self.pointer_cache.set_pointer(ptr, stake_address, block.slot);
                },
                _ => ()
            }
        }
        Ok(())
    }

    pub fn new(params: Arc<StakeDeltaFilterParams>) -> Self { Self {
        pointer_cache: PointerCache::new(),
        params: params.clone(),
        delta_publisher: DeltaPublisher::new(params.clone()),
        tracker: Tracker::new()
    }}

    pub fn info(&self) {
        info!("pointer cache size: {}, max slot: {}",
            self.pointer_cache.pointer_map.len(), self.pointer_cache.max_slot
        );
        self.tracker.info();
    }

    pub fn save(&mut self) -> Result<()> {
        let used_pointers = self.tracker.get_used_pointers();

        if self.params.write_full_cache {
            self.pointer_cache.try_save(&self.params.get_cache_file_name(".json")?)?;
        }
        else {
            self.pointer_cache.try_save_filtered(&self.params.get_cache_file_name("")?, &used_pointers)?;
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
