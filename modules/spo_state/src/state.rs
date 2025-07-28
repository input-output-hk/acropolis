//! Acropolis SPOState: State storage

use acropolis_common::{
    ledger_state::SPOState,
    messages::{CardanoMessage, Message, SPOStateMessage, TxCertificatesMessage},
    params::{SECURITY_PARAMETER_K, TECHNICAL_PARAMETER_POOL_RETIRE_MAX_EPOCH},
    serialization::SerializeMapAs,
    BlockInfo, PoolRegistration, PoolRetirement, TxCertificate,
};
use anyhow::Result;
use imbl::HashMap;
use serde_with::{hex::Hex, serde_as};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use tracing::{error, info};

#[serde_as]
#[derive(Debug, Clone, serde::Serialize)]
pub struct BlockState {
    block: u64,

    epoch: u64,

    #[serde_as(as = "SerializeMapAs<Hex, _>")]
    spos: HashMap<Vec<u8>, PoolRegistration>,

    #[serde_as(as = "SerializeMapAs<_, Vec<Hex>>")]
    pending_deregistrations: HashMap<u64, Vec<Vec<u8>>>,
}

impl BlockState {
    pub fn new(
        block: u64,
        epoch: u64,
        spos: HashMap<Vec<u8>, PoolRegistration>,
        pending_deregistrations: HashMap<u64, Vec<Vec<u8>>>,
    ) -> Self {
        Self {
            block,
            epoch,
            spos,
            pending_deregistrations,
        }
    }
}

impl From<SPOState> for BlockState {
    fn from(value: SPOState) -> Self {
        Self {
            block: 0,
            epoch: 0,
            spos: value.pools.into(),
            pending_deregistrations: value.retiring.into_iter().fold(
                HashMap::new(),
                |mut acc, (key_hash, epoch)| {
                    acc.entry(epoch).or_insert_with(Vec::new).push(key_hash);
                    acc
                },
            ),
        }
    }
}

// TODO: cleanup clones and into_iter, if possible
// It's not the end of the world here, as this is only used in testing, for now.
impl From<&BlockState> for SPOState {
    fn from(value: &BlockState) -> Self {
        Self {
            pools: value.spos.clone().into_iter().fold(BTreeMap::new(), |mut acc, (key, value)| {
                acc.insert(key, value);
                acc
            }),
            retiring: value.pending_deregistrations.clone().into_iter().fold(
                BTreeMap::new(),
                |mut acc, (epoch, key_hashes)| {
                    key_hashes.into_iter().for_each(|key_hash| {
                        acc.insert(key_hash, epoch);
                    });

                    acc
                },
            ),
        }
    }
}

/// Overall module state
pub struct State {
    /// Volatile states, one per volatile block
    history: VecDeque<BlockState>,
}

impl State {
    // Construct with optional publisher
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

    pub fn list_pools_with_info(&self) -> Option<Vec<(&Vec<u8>, &PoolRegistration)>> {
        self.current().map(|state| state.spos.iter().collect())
    }

    /// Get pools that will be retired in the upcoming epochs
    pub fn get_retiring_pools(&self) -> Vec<PoolRetirement> {
        self.current().map_or(Vec::new(), |state: &BlockState| {
            let current_epoch = state.epoch;
            state
                .pending_deregistrations
                .iter()
                .filter(|(&epoch, _)| epoch > current_epoch)
                .flat_map(|(&epoch, retiring_operators)| {
                    retiring_operators.iter().map(move |operator| PoolRetirement {
                        operator: operator.clone(),
                        epoch,
                    })
                })
                .collect()
        })
    }

    async fn log_stats(&self) {
        if let Some(current) = self.current() {
            info!(
                num_spos = current.spos.keys().len(),
                num_pending_deregistrations =
                    current.pending_deregistrations.values().map(|d| d.len()).sum::<usize>(),
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
                Some(state) => {
                    if state.block >= block_number {
                        info!("Rolling back state for block {}", state.block);
                        self.history.pop_back();
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
        if let Some(current) = self.history.back() {
            current.clone()
        } else {
            BlockState::new(0, 0, HashMap::new(), HashMap::new())
        }
    }

    /// Returns a reference to the block state at a specified height, if applicable
    pub fn inspect_previous_state(&self, block_height: u64) -> Option<&BlockState> {
        for state in self.history.iter().rev() {
            if state.block == block_height {
                return Some(state);
            }
        }
        None
    }

    // Handle end of epoch, returns message to be published
    pub fn end_epoch(&mut self, block: &BlockInfo) -> Arc<Message> {
        let current = self.get_previous_state(block.number);
        info!(
            epoch = block.epoch - 1,
            spos = current.spos.len(),
            "End of epoch"
        );

        // Flatten into vector of registrations
        let spos = current.spos.values().cloned().collect();

        let message = Arc::new(Message::Cardano((
            block.clone(),
            CardanoMessage::SPOState(SPOStateMessage {
                epoch: block.epoch - 1,
                spos,
            }),
        )));

        message
    }

    /// Handle TxCertificates with SPO registrations / de-registrations
    pub fn handle_tx_certs(
        &mut self,
        block: &BlockInfo,
        tx_certs_msg: &TxCertificatesMessage,
    ) -> Result<()> {
        let mut current = self.get_previous_state(block.number);
        current.block = block.number;

        // Handle end of epoch
        if block.epoch > current.epoch {
            current.epoch = block.epoch;

            // Deregister any pending
            let deregistrations = current.pending_deregistrations.remove(&current.epoch);
            match deregistrations {
                Some(deregistrations) => {
                    for dr in deregistrations {
                        match current.spos.remove(&dr) {
                            None => error!(
                                "Retirement requested for unregistered SPO {}",
                                hex::encode(&dr)
                            ),
                            _ => (),
                        };
                    }
                }
                None => (),
            };
        }

        // Handle certificates
        for tx_cert in tx_certs_msg.certificates.iter() {
            match tx_cert {
                TxCertificate::PoolRegistration(reg) => {
                    current.spos.insert(reg.operator.clone(), reg.clone());
                }
                TxCertificate::PoolRetirement(ret) => {
                    if ret.epoch <= current.epoch {
                        error!(
                            "SPO retirement received for current or past epoch {} for SPO {}",
                            ret.epoch,
                            hex::encode(&ret.operator)
                        );
                    } else if ret.epoch > current.epoch + TECHNICAL_PARAMETER_POOL_RETIRE_MAX_EPOCH
                    {
                        error!("SPO retirement received for epoch {} that exceeds future limit for SPO {}", ret.epoch, hex::encode(&ret.operator));
                    } else {
                        // Replace any existing queued deregistrations
                        for (epoch, deregistrations) in
                            &mut current.pending_deregistrations.iter_mut()
                        {
                            deregistrations.retain(|d| *d != ret.operator);
                            if deregistrations.len() != deregistrations.len() {
                                info!(
                                    "Removed pending deregistration of SPO {} from epoch {}",
                                    hex::encode(&ret.operator),
                                    epoch
                                );
                            }
                        }
                        current
                            .pending_deregistrations
                            .entry(ret.epoch)
                            .or_default()
                            .push(ret.operator.clone());
                    }
                }
                _ => (),
            }
        }

        // Prune and add to state history
        if self.history.len() >= SECURITY_PARAMETER_K as usize {
            self.history.pop_front();
        }
        self.history.push_back(current);

        Ok(())
    }

    pub fn bootstrap(&mut self, state: SPOState) {
        self.history.clear();
        self.history.push_back(state.into());
    }

    pub fn dump(&self, block_height: u64) -> Option<SPOState> {
        self.inspect_previous_state(block_height).map(SPOState::from)
    }
}

// -- Tests --
#[cfg(test)]
pub mod tests {
    use super::*;
    use acropolis_common::{BlockInfo, BlockStatus, Era, PoolRetirement, Ratio, TxCertificate};

    #[tokio::test]
    async fn new_state_is_empty() {
        let state = State::new();
        assert_eq!(0, state.history.len());
    }

    #[tokio::test]
    async fn current_on_new_state_returns_none() {
        let state = State::new();
        assert!(state.current().is_none());
    }

    fn new_msg() -> TxCertificatesMessage {
        TxCertificatesMessage {
            certificates: Vec::<TxCertificate>::new(),
        }
    }

    pub fn new_block() -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Immutable,
            slot: 0,
            number: 0,
            hash: Vec::<u8>::new(),
            epoch: 0,
            new_epoch: true,
            era: Era::Byron,
        }
    }

    #[tokio::test]
    async fn state_is_not_empty_after_handle() {
        let mut state = State::new();
        let msg = new_msg();
        let block = new_block();
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        assert_eq!(1, state.history.len());
    }

    #[tokio::test]
    async fn spo_gets_registered() {
        let mut state = State::new();
        let mut msg = new_msg();
        msg.certificates.push(TxCertificate::PoolRegistration(PoolRegistration {
            operator: vec![0],
            vrf_key_hash: vec![0],
            pledge: 0,
            cost: 0,
            margin: Ratio {
                numerator: 0,
                denominator: 0,
            },
            reward_account: vec![0],
            pool_owners: vec![vec![0]],
            relays: vec![],
            pool_metadata: None,
        }));
        let block = new_block();
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let current = state.current();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert_eq!(1, current.spos.len());
            let spo = current.spos.get(&vec![0u8]);
            assert!(!spo.is_none());
        };
    }

    #[tokio::test]
    async fn pending_deregistration_gets_queued() {
        let mut state = State::new();
        let mut msg = new_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 1,
        }));
        let block = new_block();
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let current = state.current();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert_eq!(1, current.pending_deregistrations.len());
            let drs = current.pending_deregistrations.get(&1);
            assert!(!drs.is_none());
            if let Some(drs) = drs {
                assert_eq!(1, drs.len());
                assert!(drs.contains(&vec![0u8]));
            }
        };
    }

    #[tokio::test]
    async fn second_pending_deregistration_gets_queued() {
        let mut state = State::new();
        let mut msg = new_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 2,
        }));
        let mut block = new_block();
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let mut msg = new_msg();
        block.number = 1;
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![1],
            epoch: 2,
        }));
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let current = state.current();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert_eq!(1, current.pending_deregistrations.len());
            let drs = current.pending_deregistrations.get(&2);
            assert!(!drs.is_none());
            if let Some(drs) = drs {
                assert_eq!(2, drs.len());
                assert!(drs.contains(&vec![0u8]));
                assert!(drs.contains(&vec![1u8]));
            }
        };
    }

    #[tokio::test]
    async fn rollback_removes_second_pending_deregistration() {
        let mut state = State::new();
        let mut msg = new_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 2,
        }));
        let mut block = new_block();
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let mut msg = new_msg();
        block.number = 1;
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![1],
            epoch: 2,
        }));
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let msg = new_msg();
        block.number = 1;
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let current = state.current();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert_eq!(1, current.pending_deregistrations.len());
            let drs = current.pending_deregistrations.get(&2);
            assert!(!drs.is_none());
            if let Some(drs) = drs {
                assert_eq!(1, drs.len());
                assert!(drs.contains(&vec![0u8]));
            }
        };
    }

    #[tokio::test]
    async fn spo_gets_deregistered() {
        let mut state = State::new();
        let mut msg = new_msg();
        msg.certificates.push(TxCertificate::PoolRegistration(PoolRegistration {
            operator: vec![0],
            vrf_key_hash: vec![0],
            pledge: 0,
            cost: 0,
            margin: Ratio {
                numerator: 0,
                denominator: 0,
            },
            reward_account: vec![0],
            pool_owners: vec![vec![0]],
            relays: vec![],
            pool_metadata: None,
        }));
        let mut block = new_block();
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let current = state.current();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert_eq!(1, current.spos.len());
            let spo = current.spos.get(&vec![0u8]);
            assert!(!spo.is_none());
        };
        let mut msg = new_msg();
        block.number = 1;
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 1,
        }));
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let msg = new_msg();
        block.number = 2;
        block.epoch = 1;
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let current = state.current();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert!(current.spos.is_empty());
        };
    }

    #[tokio::test]
    async fn spo_gets_restored_on_rollback() {
        let mut state = State::new();
        let mut msg = new_msg();
        msg.certificates.push(TxCertificate::PoolRegistration(PoolRegistration {
            operator: vec![0],
            vrf_key_hash: vec![0],
            pledge: 0,
            cost: 0,
            margin: Ratio {
                numerator: 0,
                denominator: 0,
            },
            reward_account: vec![0],
            pool_owners: vec![vec![0]],
            relays: vec![],
            pool_metadata: None,
        }));
        let mut block = new_block();
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        println!("{}", serde_json::to_string_pretty(&state.history).unwrap());
        let current = state.current();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert_eq!(1, current.spos.len());
            let spo = current.spos.get(&vec![0u8]);
            assert!(!spo.is_none());
        };
        let mut msg = new_msg();
        block.number = 1;
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 1,
        }));
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        println!("{}", serde_json::to_string_pretty(&state.history).unwrap());
        let msg = new_msg();
        block.number = 2;
        block.epoch = 1;
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        println!("{}", serde_json::to_string_pretty(&state.history).unwrap());
        let current = state.current();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert!(current.spos.is_empty());
        };
        let msg = new_msg();
        block.number = 2;
        block.epoch = 0;
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        println!("{}", serde_json::to_string_pretty(&state.history).unwrap());
        let current = state.current();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert_eq!(1, current.spos.len());
            let spo = current.spos.get(&vec![0u8]);
            assert!(!spo.is_none());
        };
    }

    #[tokio::test]
    async fn get_retiring_pools_returns_empty_when_state_is_new() {
        let state = State::new();
        assert!(state.get_retiring_pools().is_empty());
    }

    #[tokio::test]
    async fn get_retiring_pools_returns_pools() {
        let mut state = State::new();
        let mut msg = new_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 2,
        }));
        let mut block = new_block();
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let mut msg = new_msg();
        block.number = 1;
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![1],
            epoch: 3,
        }));
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let current = state.current();
        assert!(!current.is_none());
        let mut retiring_pools = state.get_retiring_pools();
        retiring_pools.sort_by_key(|p| p.epoch);
        assert_eq!(2, retiring_pools.len());
        assert_eq!(vec![0], retiring_pools[0].operator);
        assert_eq!(2, retiring_pools[0].epoch);
        assert_eq!(vec![1], retiring_pools[1].operator);
        assert_eq!(3, retiring_pools[1].epoch);
    }
}
