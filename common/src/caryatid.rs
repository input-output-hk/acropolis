use std::sync::Arc;

use crate::messages::{CardanoMessage, Message, StateTransitionMessage};
use crate::types::BlockInfo;
use crate::validation::ValidationOutcomes;
use anyhow::{anyhow, bail, Result};
use caryatid_sdk::{async_trait, Context, MessageBounds, Subscription};
use tracing::error;

#[async_trait]
pub trait SubscriptionExt<M: MessageBounds> {
    async fn read_ignoring_rollbacks(&mut self) -> Result<(String, Arc<M>)>;
}

#[async_trait]
impl SubscriptionExt<Message> for Box<dyn Subscription<Message>> {
    async fn read_ignoring_rollbacks(&mut self) -> Result<(String, Arc<Message>)> {
        loop {
            let (stream, message) = self.read().await?;
            if matches!(
                message.as_ref(),
                Message::Cardano((
                    _,
                    CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_))
                ))
            ) {
                continue;
            }
            break Ok((stream, message));
        }
    }
}

/// A utility to publish messages, which will only publish rollback messages if some work has been rolled back
pub struct RollbackAwarePublisher<M: MessageBounds> {
    /// Module context
    context: Arc<Context<M>>,

    /// Topic to publish on
    topic: String,

    // At which slot did we publish our last non-rollback message
    last_activity_at: Option<u64>,
}

impl RollbackAwarePublisher<Message> {
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self {
            context,
            topic,
            last_activity_at: None,
        }
    }

    pub async fn publish(&mut self, message: Arc<Message>) -> Result<()> {
        match message.as_ref() {
            Message::Cardano((
                block,
                CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_)),
            )) => {
                if self.last_activity_at.is_some_and(|slot| slot >= block.slot) {
                    self.last_activity_at = None;
                    self.context.publish(&self.topic, message).await?;
                }
                Ok(())
            }
            Message::Cardano((block, _)) => {
                self.last_activity_at = Some(block.slot);
                self.context.publish(&self.topic, message).await
            }
            _ => self.context.publish(&self.topic, message).await,
        }
    }
}

#[macro_export]
macro_rules! declare_cardano_reader {
    ($name:ident, $msg_constructor:ident, $msg_type:ty) => {
        async fn $name(s: &mut Box<dyn Subscription<Message>>) -> Result<(BlockInfo, $msg_type)> {
            match s.read_ignoring_rollbacks().await?.1.as_ref() {
                Message::Cardano((blk, CardanoMessage::$msg_constructor(body))) => {
                    Ok((blk.clone(), body.clone()))
                }
                msg => Err(anyhow!(
                    "Unexpected message {msg:?} for {} topic",
                    stringify!($msg_constructor)
                )),
            }
        }
    };
}

#[derive(Debug)]
pub enum RollbackWrapper<T> {
    Rollback(Arc<Message>),
    Normal((Arc<BlockInfo>, Arc<T>)),
}

#[macro_export]
macro_rules! declare_cardano_inner {
    ($reader_name:ident, $param:expr, $msg_constructor:ident, $msg_type:ty) => {
        pub struct $reader_name {
            sub: Box<dyn Subscription<Message>>,
        }

        impl $reader_name {
            pub async fn read_rb(&mut self) -> Result<RollbackWrapper<$msg_type>> {
                let res = self.sub.read().await?.1;
                match res.as_ref() {
                    Message::Cardano((blk, CardanoMessage::$msg_constructor(body))) => Ok(
                        RollbackWrapper::Normal((Arc::new(blk.clone()), Arc::new(body.clone()))),
                    ),
                    Message::Cardano((
                        _blk,
                        CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_)),
                    )) => Ok(RollbackWrapper::Rollback(res.clone())),
                    msg => bail!("Unexpected message {msg:?} for {}", $param),
                }
            }

            pub async fn read(&mut self) -> Result<(Arc<BlockInfo>, Arc<$msg_type>)> {
                loop {
                    match self.read_rb().await? {
                        RollbackWrapper::Normal(blk) => return Ok(blk),
                        RollbackWrapper::Rollback(_) => continue,
                    }
                }
            }
        }
    };
}

#[macro_export]
macro_rules! declare_cardano_rdr {
    ($reader_name:ident, $param:expr, default $def_topic:expr, $msg_constructor:ident, $msg_type:ty) => {
        declare_cardano_inner!($reader_name, $param, $msg_constructor, $msg_type);

        impl $reader_name {
            pub async fn new(ctx: &Context<Message>, cfg: &Arc<Config>) -> Result<Self> {
                let topic_name = cfg.get($param).unwrap_or($def_topic);

                tracing::info!("Creating subscriber on '{topic_name}' for '{}'", $param);
                Ok(Self {
                    sub: ctx.subscribe(&topic_name).await?,
                })
            }

            pub async fn new_opt(
                do_create: bool,
                ctx: &Context<Message>,
                cfg: &Arc<Config>,
            ) -> Result<Option<Self>> {
                if do_create {
                    Ok(Some(Self::new(ctx, cfg).await?))
                } else {
                    Ok(None)
                }
            }
        }
    };
    ($reader_name:ident, $param:expr, $msg_constructor:ident, $msg_type:ty) => {
        declare_cardano_inner!($reader_name, $param, $msg_constructor, $msg_type);

        impl $reader_name {
            pub async fn new_no_default(
                ctx: &Context<Message>,
                cfg: &Arc<Config>,
            ) -> Result<Option<Self>> {
                let Ok(topic_name) = cfg.get_string($param) else {
                    tracing::info!("No topic specified for '{}'", $param);
                    return Ok(None);
                };

                tracing::info!("Creating subscriber on '{topic_name}' for '{}'", $param);
                Ok(Some(Self {
                    sub: ctx.subscribe(&topic_name).await?,
                }))
            }
        }
    };
}

pub struct ValidationContext {
    context: Arc<Context<Message>>,
    current_block: Option<Arc<BlockInfo>>,
    validation: ValidationOutcomes,
    validation_topic: String,
}

impl ValidationContext {
    pub fn new(context: &Arc<Context<Message>>, validation_topic: &str) -> Self {
        Self {
            validation: ValidationOutcomes::new(),
            current_block: None,
            context: context.clone(),
            validation_topic: validation_topic.to_owned(),
        }
    }

    pub fn get_block_info(&self) -> Result<Arc<BlockInfo>> {
        self.current_block.as_ref().ok_or_else(|| anyhow!("Current block missing")).cloned()
    }

    pub fn get_current_block_opt(&self) -> Option<Arc<BlockInfo>> {
        self.current_block.clone()
    }

    fn handling_error(&mut self, handler: &str, error: &anyhow::Error) {
        self.validation.push_anyhow(anyhow!("Error handling {handler}: {error:#}"));
    }

    /// Adds given `outcome` to the validation outcomes list.
    /// If the `outcome` is 'Err', then the error is added to the outcomes list instead,
    /// annotated with `handler` string.
    pub fn merge(&mut self, handler: &str, outcome: Result<ValidationOutcomes>) {
        match outcome {
            Err(e) => self.handling_error(handler, &e),
            Ok(mut outcome) => self.validation.merge(&mut outcome),
        }
    }

    /// Adds outcome to the validation outcomes list. Similar to `merge`, but passes
    /// value of arbitrary type `T` instead of merging validation outcomes.
    /// Main intention for `T` is unit (`()`), but the implementation made more general.
    /// * Checks errors (and adds them to the validation outcomes, if result is Err)
    /// * Passes argument to the outcome (or replaces it with default)
    ///   `handler` annotation string for errors
    ///   `result` result of validation to be checked and passed
    pub fn handle<T: Default>(&mut self, handler: &str, result: Result<T>) -> T {
        match result {
            Ok(outcome) => outcome,
            Err(e) => {
                self.handling_error(handler, &e);
                T::default()
            }
        }
    }

    /// Analyzes message retrieved from a subscriber used for synchronization:
    /// * checks errors (adds them to validation outcome);
    /// * sets current_block (in case the message is not empty).
    pub fn consume_sync<T>(
        &mut self,
        inp: Result<RollbackWrapper<T>>,
    ) -> Result<RollbackWrapper<T>> {
        match &inp {
            Ok(RollbackWrapper::Normal((blk, _msg))) => {
                self.current_block = Some(blk.clone());
            }
            Ok(RollbackWrapper::Rollback(_)) => {
                self.current_block = None;
            }
            Err(e) => {
                self.current_block = None;
                bail!("Error handling sync block: {e}");
            }
        }
        inp
    }

    /// Analyzes message retrieved from a subscriber:
    /// * checks errors (adds them to validation outcome);
    /// * checks block info (error is generated if current_block field set and different from
    ///   block info of the message from the topic. That is, all blocks received by the module
    ///   must have the same block info
    pub fn consume<T>(
        &mut self,
        handler: &str,
        inp: Result<(Arc<BlockInfo>, Arc<T>)>,
    ) -> Option<(Arc<BlockInfo>, Arc<T>)> {
        match inp {
            Ok(ref msg @ (ref blk_info, _)) => {
                self.check_sync(handler, blk_info);
                Some(msg.clone())
            }
            Err(e) => {
                self.validation.push_anyhow(e);
                None
            }
        }
    }

    pub async fn publish(&mut self) {
        if let Some(blk) = &self.current_block {
            if let Err(e) =
                self.validation.publish(&self.context, &self.validation_topic, blk).await
            {
                error!("Publish failed: {:?}", e);
            }
        } else {
            self.validation.print_errors(None);
        }
    }

    /// Check for synchronisation
    fn check_sync(&mut self, handler: &str, actual: &BlockInfo) {
        if let Some(ref block) = self.current_block {
            if block.number != actual.number {
                self.validation.push_anyhow(anyhow!(
                    "Messages out of sync: expected {:?}, actual ({handler}) {:?}",
                    block,
                    actual
                ));
            }
        }
    }
}
