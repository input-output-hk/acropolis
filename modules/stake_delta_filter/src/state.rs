//! Acropolis Stake Delta Filter: State storage

use std::{collections::{HashMap, VecDeque}, fs, io::Write, sync::Arc};
use acropolis_common::{
    messages::{AddressDeltasMessage, Message, StakeAddressDeltasMessage, TxCertificatesMessage}, 
    Address, BlockInfo, SerialisedHandler, ShelleyAddressPointer, StakeAddress, 
    StakeAddressPayload, StakeCredential, TxCertificate
};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_with::serde_as;
use tracing::info;
use crate::StakeDeltaFilterParams;
use crate::{PointerCache, process_message};

#[serde_as]
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct PointerOccurrence {
    #[serde_as(as = "Vec<(_, _)>")]
    pub occurrence: HashMap<ShelleyAddressPointer, Vec<(Option<Address>, BlockInfo, Address)>>
}

impl PointerOccurrence {
    pub fn info(&self) {
        info!("Len {}", self.occurrence.len());
        self.occurrence.iter().take(10).for_each(
            |(k,v)| {
                info!("{:?} => {}: {:?} ...", k, v.len(), v.iter().take(3))
            }
        );
    }

    pub fn add(&mut self, destination: Option<&Address>, full_address: &Address, block_info: &BlockInfo) -> Result<()> {
        let shelley = full_address.get_pointer().ok_or_else(
            || anyhow!("pointer not present in {:?}", full_address)
        )?;

        self.occurrence.insert(shelley.clone(), vec!((destination.cloned(), block_info.clone(), full_address.clone())));

        //self.occurrence.entry(shelley.clone())
        //    .or_insert(vec![])
        //    .push((block_info.clone(), full_address.clone()));

        Ok(())
    }

    pub fn display_nice(&self) -> Result<String> {
        let mut res = Vec::<String>::new();
        for (ptr,occ) in self.occurrence.iter() {
             let (dst, blk, full) = occ.get(0).ok_or_else(|| anyhow!("Occurrence 0 must present"))?;
             res.push(format!("src: {:?}, dst: {:?}, blk: {:?}, ptr: {},{},{}\n",
                 full, dst, blk, ptr.slot, ptr.tx_index, ptr.cert_index,
             ));
        }
        return Ok(res.into_iter().collect::<String>());
    }
}

pub struct DeltaPublisher {
    pub params: Arc<StakeDeltaFilterParams>
}

impl DeltaPublisher {
    pub fn new (params: Arc<StakeDeltaFilterParams>) -> Self { Self { params } }

    pub async fn publish(&self, message: StakeAddressDeltasMessage) -> Result<()> {
        let packed_message = Arc::new(Message::StakeAddressDeltas(message));
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
    pub correct_ptrs: PointerOccurrence,
    pub incorrect_ptrs: PointerOccurrence,

    pub request_queue: VecDeque<AddressDeltasMessage>,
    pub params: Arc<StakeDeltaFilterParams>,
    pub delta_publisher: DeltaPublisher
}

#[async_trait]
impl SerialisedHandler<AddressDeltasMessage> for State {
    async fn handle(&mut self, _sequence: u64, most_recent_delta: &AddressDeltasMessage) -> Result<()> {
        //info!("New address delta message: {:?}", most_recent_delta);
        if most_recent_delta.block.slot % 10000 == 0 {
            info!("New address delta message: {}", most_recent_delta.block.slot);
        }
        self.request_queue.push_back(most_recent_delta.clone());

        while let Some(delta) = self.request_queue.get(0) {
            match process_message(&self.pointer_cache, delta).await {
                Err(e) => tracing::error!("Cannot decode and convert stake key for {most_recent_delta:?}: {e}"),
                Ok(r) => self.delta_publisher.publish(r).await?
            }
            self.request_queue.pop_front();
        }
        Ok(())
    }
}

//params.context.clone().message_bus.subscribe(&params.clone().tx_certificates_topic, move |message: Arc<Message>| {

#[async_trait]
impl SerialisedHandler<TxCertificatesMessage> for State {
    async fn handle(&mut self, _sequence: u64, msg: &TxCertificatesMessage) -> Result<()> {
        for cert in msg.certificates.iter() {
            match cert {
                TxCertificate::StakeRegistration(reg) => {
                    let ptr = ShelleyAddressPointer {
                        slot: msg.block.slot,
                        tx_index: reg.tx_index,
                        cert_index: reg.cert_index,
                    };
                    let stake_address = StakeAddress{
                        network: self.params.network.clone(),
                        payload: match &reg.stake_credential {
                            StakeCredential::ScriptHash(h) => StakeAddressPayload::ScriptHash(h.clone()),
                            StakeCredential::AddrKeyHash(k) => StakeAddressPayload::StakeKeyHash(k.clone())
                        }
                    };
                    //info!("New pointer {:?}: points to stake {:?}", ptr, stake_address);

                    self.pointer_cache.pointer_map.insert(ptr, Address::Stake(stake_address));
                    self.pointer_cache.update_max_slot(msg.block.slot);
                },
                _ => ()
            }
        }
        Ok(())
    }
}

impl State {
    pub fn new(params: Arc<StakeDeltaFilterParams>) -> Self { Self {
        pointer_cache: PointerCache::new(),
        correct_ptrs: PointerOccurrence::default(),
        incorrect_ptrs: PointerOccurrence::default(),
        request_queue: VecDeque::default(),
        params: params.clone(),
        delta_publisher: DeltaPublisher::new(params.clone())
    }}
/*
    pub fn decode_address(&mut self, block: &BlockInfo, address: &Address) -> Result<Address> {
        match self.pointer_cache.decode_address(address) {
            Ok(decoded) if &decoded != address => {
                self.correct_ptrs.add(Some(&decoded), address, block)?;
                Ok(decoded.clone())
            }
            Ok(_dd) => Ok(address.clone()),
            Err(_e) => {
                self.incorrect_ptrs.add(None, address, block)?;
                Ok(address.clone())
            }
        }
    }
*/
    pub fn info(&self) {
        info!("pointer cache size: {}, max slot: {}",
            self.pointer_cache.pointer_map.len(), self.pointer_cache.max_slot
        );
        self.correct_ptrs.info();
        self.incorrect_ptrs.info();
    }

    pub fn save(&self) -> Result<()> {

        //let mut file = fs::File::create(filename)?;
        //file.write_all(serde_json::to_string(&self.pointer_cache)?.as_bytes())?;
        //file.write_all("\n".as_bytes())?;

        self.pointer_cache.try_save(&self.params.get_cache_file_name("")?)?;

        let mut file = fs::File::create(self.params.get_cache_file_name(".correct")?)?;
        file.write_all(self.correct_ptrs.display_nice()?.as_bytes())?;
        file.write_all(serde_json::to_string(&self.correct_ptrs)?.as_bytes())?;
        file.write_all("\n".to_string().as_bytes())?;

        let mut file = fs::File::create(self.params.get_cache_file_name(".incorrect")?)?;
        file.write_all(serde_json::to_string(&self.incorrect_ptrs)?.as_bytes())?;
        file.write_all("\n".as_bytes())?;

        Ok(())
    }

    pub async fn tick(&self) -> Result<()> {
        self.info();
        self.save()?;
        Ok(())
    }
}
