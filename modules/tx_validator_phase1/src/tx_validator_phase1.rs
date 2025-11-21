//! Acropolis transaction unpacker module for Caryatid
//! Unpacks transaction bodies into UTXO events

mod state;

use acropolis_common::{
    messages::{CardanoMessage, Message, ProtocolParamsMessage, RawTxsMessage},
    *,
};

use acropolis_codec::map_parameters;

use caryatid_sdk::{module, Context, Module, Subscription};
use std::{clone::Clone, sync::Arc};

use crate::state::State;
use acropolis_common::validation::ValidationStatus;
use anyhow::{anyhow, bail, Result};
use config::Config;
use tracing::{error, info};
//mod utxo_registry;
//use crate::utxo_registry::UTxORegistry;

const DEFAULT_TRANSACTIONS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("transactions-subscribe-topic", "cardano.txs");
const DEFAULT_PROTOCOL_PARAMETERS_TOPIC: (&str, &str) =
    ("parameters-topic", "cardano.protocol.parameters");
const DEFAULT_VALIDATION_RESULT_TOPIC: (&str, &str) = (
    "publish-valiadtion-result-topic",
    "cardano.validation.tx-phase-1",
);
const DEFAULT_NETWORK_NAME: (&str, &str) = ("network-name", "mainnet");

//const CIP25_METADATA_LABEL: u64 = 721;

/// Tx unpacker module
/// Parameterised by the outer message enum used on the bus
#[module(
    message_type(Message),
    name = "tx-validator-phase1",
    description = "Transactions validator, Phase 1"
)]
pub struct TxValidatorPhase1;

struct TxValidatorPhase1StateConfig {
    pub context: Arc<Context<Message>>,
    pub transactions_subscribe_topic: String,
    pub genesis_utxos_subscribe_topic: String,
    pub publish_validation_result: String,
    pub params_subscribe_topic: String,
    #[allow(dead_code)]
    pub network_name: String,
}

impl TxValidatorPhase1StateConfig {
    fn conf(config: &Arc<Config>, keydef: (&str, &str)) -> String {
        let actual = config.get_string(keydef.0).unwrap_or(keydef.1.to_string());
        info!("Parameter value '{}' for {}", actual, keydef.0);
        actual
    }

    pub fn new(context: &Arc<Context<Message>>, config: &Arc<Config>) -> Arc<Self> {
        Arc::new(Self {
            context: context.clone(),
            transactions_subscribe_topic: Self::conf(config, DEFAULT_TRANSACTIONS_SUBSCRIBE_TOPIC),
            genesis_utxos_subscribe_topic: Self::conf(config, DEFAULT_PROTOCOL_PARAMETERS_TOPIC),
            params_subscribe_topic: Self::conf(config, DEFAULT_PROTOCOL_PARAMETERS_TOPIC),
            publish_validation_result: Self::conf(config, DEFAULT_VALIDATION_RESULT_TOPIC),
            network_name: Self::conf(config, DEFAULT_NETWORK_NAME),
        })
    }
}

impl TxValidatorPhase1 {
    async fn read_parameters(
        parameters_s: &mut Box<dyn Subscription<Message>>,
    ) -> Result<(BlockInfo, ProtocolParamsMessage)> {
        match parameters_s.read().await?.1.as_ref() {
            Message::Cardano((blk, CardanoMessage::ProtocolParams(params))) => {
                Ok((blk.clone(), params.clone()))
            }
            msg => Err(anyhow!(
                "Unexpected message {msg:?} for protocol parameters topic"
            )),
        }
    }

    async fn read_transactions(
        transaction_s: &mut Box<dyn Subscription<Message>>,
    ) -> Result<(BlockInfo, RawTxsMessage)> {
        match transaction_s.read().await?.1.as_ref() {
            Message::Cardano((blk, CardanoMessage::ReceivedTxs(tx))) => {
                Ok((blk.clone(), tx.clone()))
            }
            msg => Err(anyhow!("Unexpected message {msg:?} for transaction topic")),
        }
    }

    async fn publish_result(
        config: &TxValidatorPhase1StateConfig,
        block: BlockInfo,
        result: ValidationStatus,
    ) -> Result<()> {
        if let ValidationStatus::NoGo(res) = &result {
            error!("Cannot validate transaction: {:?}", res);
        }

        let packed_message = Arc::new(Message::Cardano((
            block.clone(),
            CardanoMessage::BlockValidation(result),
        )));
        let context = config.context.clone();
        let topic = config.publish_validation_result.clone();

        tokio::spawn(async move {
            context
                .publish(&topic, packed_message)
                .await
                .unwrap_or_else(|e| tracing::error!("Failed to publish: {e}"));
        });

        Ok(())
    }

    async fn run(
        state: &mut State,
        mut _gen: Box<dyn Subscription<Message>>,
        mut txs: Box<dyn Subscription<Message>>,
        mut params: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        loop {
            let (trx_b, trx) = Self::read_transactions(&mut txs).await?;
            if trx_b.new_epoch {
                let (prm_b, prm) = Self::read_parameters(&mut params).await?;
                if prm_b != trx_b {
                    bail!("Blocks are out of sync: transaction {trx_b:?} != params {prm_b:?}");
                }
                state.process_params(prm_b, prm).await?;
            }
            let response = state.process_transactions(&trx_b, &trx)?;
            Self::publish_result(&state.config, trx_b, response).await?;
        }
    }

    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let config = TxValidatorPhase1StateConfig::new(&context, &config);

        // Subscribe to genesis and txs topics
        let gen_sub = context.subscribe(&config.genesis_utxos_subscribe_topic).await?;
        let txs_sub = context.subscribe(&config.transactions_subscribe_topic).await?;
        let params_sub = context.subscribe(&config.params_subscribe_topic).await?;

        context.clone().run(async move {
            let mut state = State::new(config.clone());
            TxValidatorPhase1::run(&mut state, gen_sub, txs_sub, params_sub)
                .await
                .unwrap_or_else(|e| error!("TX validator failed: {e}"));
        });

        Ok(())
    }
}
