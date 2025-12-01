use acropolis_common::BlockInfo;
use anyhow::Result;
use caryatid_sdk::async_trait;
use pallas::ledger::traverse::MultiEraTx;

#[async_trait]
pub trait ManagedIndex: Send + Sync + 'static {
    fn name(&self) -> String;

    async fn handle_onchain_tx(&mut self, info: &BlockInfo, tx: &MultiEraTx<'_>) -> Result<()> {
        let _ = (info, tx);
        Ok(())
    }

    async fn handle_rollback(&mut self, info: &BlockInfo) -> Result<()> {
        let _ = info;
        Ok(())
    }
}
