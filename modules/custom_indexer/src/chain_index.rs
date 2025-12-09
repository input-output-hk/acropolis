use acropolis_common::{BlockInfo, Point};
use anyhow::Result;
use caryatid_sdk::async_trait;
use pallas::ledger::traverse::MultiEraTx;

#[async_trait]
pub trait ChainIndex: Send + Sync + 'static {
    /// A human-readable identifier for the index.
    /// Used for logging, error messages, and cursor store keys.
    fn name(&self) -> String;

    /// High-level transaction handler.
    ///
    /// The indexer runtime calls this when `wants_raw_bytes() == false`.
    /// Implementors receive a fully decoded `MultiEraTx` using Acropolis’s
    /// Pallas dependency. Most indexes should override this unless they
    /// need to control decoding themselves.
    async fn handle_onchain_tx(&mut self, info: &BlockInfo, tx: &MultiEraTx<'_>) -> Result<()> {
        let _ = (info, tx);
        Ok(())
    }

    /// Low-level transaction handler that receives raw CBOR bytes.
    ///
    /// The indexer runtime calls this when `wants_raw_bytes() == true`.
    /// Implementors can parse the bytes using their own Pallas versions,
    /// bypass decoding in the runtime, or operate directly on the CBOR.
    async fn handle_onchain_tx_bytes(&mut self, info: &BlockInfo, raw_tx: &[u8]) -> Result<()> {
        let _ = (info, raw_tx);
        Ok(())
    }

    /// Called when the chain rolls back to a point.
    ///
    /// Implementations must remove or revert any state derived from slots
    /// greater than `point`. Failing to do so will corrupt index state.
    async fn handle_rollback(&mut self, point: &Point) -> Result<()> {
        let _ = point;
        Ok(())
    }

    /// Selects between decoded-transaction mode and raw-bytes mode.
    ///
    /// `false` (default): runtime decodes transactions and calls `handle_onchain_tx`.
    /// `true`: runtime skips decoding and calls `handle_onchain_tx_bytes`.
    fn wants_raw_bytes(&self) -> bool {
        false
    }

    /// Resets the index to a known chain point.
    ///
    /// Most implementations return `start` unchanged. However, more advanced
    /// indexes may choose a different reset point based on internal state.
    async fn reset(&mut self, start: &Point) -> Result<Point>;
}
