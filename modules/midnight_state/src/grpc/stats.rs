use std::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Default)]
pub struct RequestStats {
    pub utxo_events: AtomicU64,
    pub bridge_utxos: AtomicU64,
    pub council_datum: AtomicU64,
    pub technical_committee_datum: AtomicU64,
    pub ariadne_parameters: AtomicU64,
    pub block_by_hash: AtomicU64,
    pub epoch_nonce: AtomicU64,
    pub epoch_candidates: AtomicU64,
    pub latest_stable_block: AtomicU64,
    pub stable_block_by_hash: AtomicU64,
}

#[derive(Debug)]
pub struct RequestStatsSnapshot {
    pub utxo_events: u64,
    pub bridge_utxos: u64,
    pub council_datum: u64,
    pub technical_committee_datum: u64,
    pub ariadne_parameters: u64,
    pub block_by_hash: u64,
    pub epoch_nonce: u64,
    pub epoch_candidates: u64,
    pub latest_stable_block: u64,
    pub stable_block_by_hash: u64,
}

impl fmt::Display for RequestStatsSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "utxo_events={} bridge_utxos={} council_datum={} technical_committee_datum={} \
             ariadne_parameters={} block_by_hash={} epoch_nonce={} epoch_candidates={} \
             latest_stable_block={} stable_block_by_hash={}",
            self.utxo_events,
            self.bridge_utxos,
            self.council_datum,
            self.technical_committee_datum,
            self.ariadne_parameters,
            self.block_by_hash,
            self.epoch_nonce,
            self.epoch_candidates,
            self.latest_stable_block,
            self.stable_block_by_hash
        )
    }
}

impl RequestStats {
    pub fn snapshot(&self) -> RequestStatsSnapshot {
        RequestStatsSnapshot {
            utxo_events: self.utxo_events.load(Ordering::Relaxed),
            bridge_utxos: self.bridge_utxos.load(Ordering::Relaxed),
            council_datum: self.council_datum.load(Ordering::Relaxed),
            technical_committee_datum: self.technical_committee_datum.load(Ordering::Relaxed),
            ariadne_parameters: self.ariadne_parameters.load(Ordering::Relaxed),
            block_by_hash: self.block_by_hash.load(Ordering::Relaxed),
            epoch_nonce: self.epoch_nonce.load(Ordering::Relaxed),
            epoch_candidates: self.epoch_candidates.load(Ordering::Relaxed),
            latest_stable_block: self.latest_stable_block.load(Ordering::Relaxed),
            stable_block_by_hash: self.stable_block_by_hash.load(Ordering::Relaxed),
        }
    }
}
