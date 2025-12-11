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
    /// Most indexes override this
    async fn handle_onchain_tx(&mut self, info: &BlockInfo, tx: &MultiEraTx<'_>) -> Result<()> {
        let _ = (info, tx);
        Ok(())
    }

    /// Low-level raw-bytes handler.
    ///
    /// Default behavior:
    ///   - decode the tx using Pallas
    ///   - call the high-level handler
    ///
    /// Indexes that want raw bytes override this and bypass decoding entirely.
    async fn handle_onchain_tx_bytes(&mut self, info: &BlockInfo, raw_tx: &[u8]) -> Result<()> {
        let tx = MultiEraTx::decode(raw_tx)?;
        self.handle_onchain_tx(info, &tx).await
    }

    /// Called when the chain rolls back to a point.
    ///
    /// Implementations must remove or revert any state derived from slots
    /// greater than `point`. Failing to do so will corrupt index state.
    async fn handle_rollback(&mut self, point: &Point) -> Result<()> {
        let _ = point;
        Ok(())
    }

    /// Resets the index to a known chain point.
    ///
    /// Most implementations return `start` unchanged. However, more advanced
    /// indexes may choose a different reset point based on internal state.
    async fn reset(&mut self, start: &Point) -> Result<Point>;
}
