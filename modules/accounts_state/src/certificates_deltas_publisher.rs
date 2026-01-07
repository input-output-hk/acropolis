use acropolis_common::caryatid::RollbackAwarePublisher;
use acropolis_common::messages::{CardanoMessage, Message, StakeCertificatesDeltasMessage};
use acropolis_common::{BlockInfo, StakeCertificateDelta};
use caryatid_sdk::Context;
use std::sync::Arc;

/// Message publisher for Stake Certificates Deltas
pub struct StakeCertificatesDeltasPublisher(RollbackAwarePublisher<Message>);

impl StakeCertificatesDeltasPublisher {
    /// Construct with context and topic to publish on
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self(RollbackAwarePublisher::new(context, topic))
    }

    /// Publish the Stake Certificate Deltas
    pub async fn publish(
        &mut self,
        block: &BlockInfo,
        stake_certificate_deltas: Vec<StakeCertificateDelta>,
    ) -> anyhow::Result<()> {
        self.0
            .publish(Arc::new(Message::Cardano((
                block.clone(),
                CardanoMessage::StakeCertificatesDeltas(StakeCertificatesDeltasMessage {
                    deltas: stake_certificate_deltas,
                }),
            ))))
            .await
    }

    /// Publish a rollback message, if we have anything to roll back
    pub async fn publish_rollback(&mut self, message: Arc<Message>) -> anyhow::Result<()> {
        self.0.publish(message).await
    }
}
