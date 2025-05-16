//! Acropolis SPOState: State storage
use acropolis_common::{
    messages::TxCertificatesMessage,
    PoolRegistration,
    TxCertificate,
    params::{SECURITY_PARAMETER_K, TECHNICAL_PARAMETER_POOL_RETIRE_MAX_EPOCH,},
};
use anyhow::Result;
use imbl::HashMap;
use tracing::{error, info};
use serde::{Serializer, ser::SerializeMap};
use serde_with::{serde_as, hex::Hex, SerializeAs, ser::SerializeAsWrap};
use std::collections::VecDeque;

struct HashMapSerial<KAs, VAs>(std::marker::PhantomData<(KAs, VAs)>);

impl<K, V, KAs, VAs> SerializeAs<HashMap<K, V>> for HashMapSerial<KAs, VAs>
where
    KAs: SerializeAs<K>,
    VAs: SerializeAs<V>,
{
    fn serialize_as<S>(source: &HashMap<K, V>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map_ser = serializer.serialize_map(Some(source.len()))?;
        for (k, v) in source {
            map_ser.serialize_entry(
                &SerializeAsWrap::<K, KAs>::new(k),
                &SerializeAsWrap::<V, VAs>::new(v),
            )?;
        }
        map_ser.end()
    }
}

#[serde_as]
#[derive(Debug, Clone, serde::Serialize)]
pub struct BlockState {
    block: u64,

    epoch: u64,

    #[serde_as(as = "HashMapSerial<Hex, _>")]
    spos: HashMap<Vec::<u8>, PoolRegistration>,

    #[serde_as(as = "HashMapSerial<_, Vec<Hex>>")]
    pending_deregistrations: HashMap<u64, Vec<Vec<u8>>>,
}

impl BlockState {
    pub fn new(block: u64, epoch: u64, spos: HashMap<Vec::<u8>, PoolRegistration>,
               pending_deregistrations: HashMap<u64, Vec<Vec<u8>>>) -> Self {
        Self {
            block,
            epoch,
            spos,
            pending_deregistrations,
        }
    }
}

pub struct State {
    history: VecDeque<BlockState>,
}

impl State {
    pub fn new() -> Self {
        Self {
            history: VecDeque::<BlockState>::new(),
        }
    }

    pub fn current(&self) -> Option<&BlockState> {
        self.history.back()
    }

    pub fn get(&self, operator: &Vec<u8>) -> Option<&PoolRegistration> {
        if let Some(current) = self.current() {
            current.spos.get(operator)
        } else {
            None
        }
    }

    async fn log_stats(&self) {
        if let Some(current) = self.current() {
            info!(
                num_spos = current.spos.keys().len(),
                num_pending_deregistrations = current.pending_deregistrations.values().map(|d| d.len()).sum::<usize>(),
            );
        } else {
            info!(num_spos = 0, num_pending_deregistrations = 0);
        }
    }

    pub async fn tick(&self) -> Result<()> {
        self.log_stats().await;
        Ok(())
    }

    fn get_previous_state(&mut self, block_number: u64) -> BlockState {
        loop {
            match self.history.back() {
                Some(state) => if state.block >= block_number {
                    info!("Rolling back state for block {}", state.block);
                    self.history.pop_back();
                } else {
                    break
                },
                _ => break
            }
        }
        if let Some(current) = self.history.back() {
            current.clone()
        } else {
            BlockState::new(0, 0, HashMap::new(), HashMap::new())
        }
    }

    pub fn handle(&mut self, tx_cert_msg: &TxCertificatesMessage) -> Result<()> {
        let mut current = self.get_previous_state(tx_cert_msg.block.number);
        current.block = tx_cert_msg.block.number;
        if tx_cert_msg.block.epoch > current.epoch {
            current.epoch = tx_cert_msg.block.epoch;
            let deregistrations = current.pending_deregistrations.remove(&current.epoch);
            match deregistrations {
                Some(deregistrations) => {
                    for dr in deregistrations {
                        match current.spos.remove(&dr) {
                        None => error!("Retirement requested for unregistered SPO {}", hex::encode(&dr)),
                        _ => (),
                    };
                    }
                },
                None => (),
            };
        }
        for tx_cert in tx_cert_msg.certificates.iter() {
            match tx_cert {
                TxCertificate::PoolRegistration(reg) => {
                    current.spos.insert(reg.operator.clone(), reg.clone());
                }
                TxCertificate::PoolRetirement(ret) => {
                    if ret.epoch <= current.epoch {
                        error!("SPO retirement received for current or past epoch {} for SPO {}", ret.epoch, hex::encode(&ret.operator));
                    } else if ret.epoch > current.epoch + TECHNICAL_PARAMETER_POOL_RETIRE_MAX_EPOCH {
                        error!("SPO retirement received for epoch {} that exceeds future limit for SPO {}", ret.epoch, hex::encode(&ret.operator));
                    } else {
                        // Replace any existing queued deregistrations
                        for (epoch, deregistrations) in &mut current.pending_deregistrations.iter_mut() {
                            deregistrations.retain(|d| *d != ret.operator);
                            if deregistrations.len() != deregistrations.len() {
                                info!("Removed pending deregistration of SPO {} from epoch {}", hex::encode(&ret.operator), epoch);
                            }
                        }
                        current.pending_deregistrations.entry(ret.epoch).or_default().push(ret.operator.clone());
                    }
                }
                _ => ()
            }
        }
        if self.history.len() >= SECURITY_PARAMETER_K as usize {
            self.history.pop_front();
        }
        self.history.push_back(current);

        Ok(())
    }
}
