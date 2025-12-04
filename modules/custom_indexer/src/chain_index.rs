use acropolis_common::{BlockInfo, Point};
use anyhow::Result;
use caryatid_sdk::async_trait;
use pallas::ledger::traverse::MultiEraTx;

#[async_trait]
pub trait ChainIndex: Send + Sync + 'static {
    fn name(&self) -> String;

    async fn handle_onchain_tx(&mut self, info: &BlockInfo, tx: &MultiEraTx<'_>) -> Result<()> {
        let _ = (info, tx);
        Ok(())
    }

    async fn handle_rollback(&mut self, point: &Point) -> Result<()> {
        let _ = point;
        Ok(())
    }

    async fn reset(&mut self, start: &Point) -> Result<Point>;
}
