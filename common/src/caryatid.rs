use std::sync::Arc;

use crate::messages::{CardanoMessage, Message, StateTransitionMessage};
use crate::types::BlockInfo;
use crate::validation::ValidationOutcomes;
use anyhow::{anyhow, bail, Result};
use caryatid_sdk::{Context, MessageBounds};
use tracing::error;

/// A utility to publish messages on a configured topic.
pub struct RollbackAwarePublisher<M: MessageBounds> {
    /// Module context
    context: Arc<Context<M>>,

    /// Topic to publish on
    topic: String,
}

impl RollbackAwarePublisher<Message> {
    pub fn new(context: Arc<Context<Message>>, topic: String) -> Self {
        Self { context, topic }
    }

    pub async fn publish(&mut self, message: Arc<Message>) -> Result<()> {
        self.context.publish(&self.topic, message).await
    }
}

#[derive(Debug)]
pub enum RollbackWrapper<T> {
    Rollback((Arc<BlockInfo>, Arc<Message>)),
    Normal((Arc<BlockInfo>, Arc<T>)),
}

#[derive(Debug)]
pub enum PrimaryRead<T> {
    Normal {
        block_info: Arc<BlockInfo>,
        message: Arc<T>,
    },
    Rollback {
        block_info: Arc<BlockInfo>,
        rollback_message: Arc<Message>,
    },
}

impl<T> PrimaryRead<T> {
    pub fn from_read(input: RollbackWrapper<T>) -> Self {
        match input {
            RollbackWrapper::Normal((block_info, message)) => Self::Normal {
                block_info,
                message,
            },
            RollbackWrapper::Rollback((block_info, rollback_message)) => Self::Rollback {
                block_info,
                rollback_message,
            },
        }
    }

    pub fn from_sync(
        ctx: &mut ValidationContext,
        handler: &str,
        input: Result<RollbackWrapper<T>>,
    ) -> Result<Self> {
        Ok(Self::from_read(ctx.consume_sync(handler, input)?))
    }

    pub fn block_info(&self) -> &Arc<BlockInfo> {
        match self {
            Self::Normal { block_info, .. } | Self::Rollback { block_info, .. } => block_info,
        }
    }

    pub fn message(&self) -> Option<&Arc<T>> {
        match self {
            Self::Normal { message, .. } => Some(message),
            Self::Rollback { .. } => None,
        }
    }

    pub fn rollback_message(&self) -> Option<&Arc<Message>> {
        match self {
            Self::Normal { .. } => None,
            Self::Rollback {
                rollback_message, ..
            } => Some(rollback_message),
        }
    }

    pub fn is_rollback(&self) -> bool {
        matches!(self, Self::Rollback { .. })
    }

    pub fn do_validation(&self) -> bool {
        !self.is_rollback() && self.block_info().intent.do_validation()
    }

    pub fn should_read_epoch_messages(&self) -> bool {
        self.is_rollback() || self.should_read_epoch_transition_messages()
    }

    pub fn should_read_epoch_transition_messages(&self) -> bool {
        self.epoch().is_some()
    }

    pub fn epoch(&self) -> Option<u64> {
        match self {
            Self::Normal { block_info, .. } if Self::is_epoch_boundary(block_info) => {
                Some(block_info.epoch)
            }
            Self::Normal { .. } | Self::Rollback { .. } => None,
        }
    }

    fn is_epoch_boundary(block_info: &BlockInfo) -> bool {
        block_info.new_epoch && block_info.epoch > 0
    }
}

impl PrimaryRead<Message> {
    pub fn from_cardano_message(message: Arc<Message>) -> Result<Self> {
        match message.as_ref() {
            Message::Cardano((
                block_info,
                CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_)),
            )) => Ok(Self::Rollback {
                block_info: Arc::new(block_info.clone()),
                rollback_message: message,
            }),
            Message::Cardano((block_info, _)) => Ok(Self::Normal {
                block_info: Arc::new(block_info.clone()),
                message,
            }),
            msg => bail!("Unexpected message {msg:?}"),
        }
    }
}

/// Declares locally tailored cardano reader struct, providing a lightweight wrapper around
/// Subscribers from topics. The Main intention is to get rid of boilerplate code, and simplify:
/// (a) topic configuration, config parameters reading and reader initialization;
/// (b) data reading from the topic subscriber. The data are taken from the enum constructor
/// by the functions, provided in the struct being declared, so that the user does not need to
/// manually specify pattern matching code.
#[macro_export]
macro_rules! declare_cardano_reader {
    ($reader_name:ident, $param:expr, $def_topic:expr, $msg_constructor:ident, $msg_type:ty) => {
        pub struct $reader_name {
            sub: Box<dyn Subscription<Message>>,
        }

        impl $reader_name {
            /// Created and initalizes reader, taking topic parameters from config
            pub async fn new(ctx: &Context<Message>, cfg: &Arc<Config>) -> Result<Self> {
                if $def_topic.is_empty() {
                    bail!("No default topic for '{}'", $param);
                }
                let topic_name = cfg.get($param).unwrap_or($def_topic);

                info!("Creating subscriber on '{topic_name}' for '{}'", $param);
                Ok(Self {
                    sub: ctx.subscribe(&topic_name).await?,
                })
            }

            /// Creates and initializes Option<$reader_name>, taking topic parameters from config,
            /// if `do_create` parameter is true, initializes the reader.
            /// if `do_create` parameter is false, initialization is skipped.
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

            /// Creates and initializes Option<$reader_name>, taking topic parameters from config.
            /// If the topic is not specified, default topic name is not used instead, and
            /// None is returned.
            pub async fn new_without_default(
                ctx: &Context<Message>,
                cfg: &Arc<Config>,
            ) -> Result<Option<Self>> {
                match cfg.get::<String>($param) {
                    Ok(topic_name) => {
                        if !$def_topic.is_empty() {
                            bail!(
                                "Default topic for '{}' is '{}', cannot use new_without_default",
                                $param,
                                $def_topic
                            );
                        }
                        info!("Creating subscriber on '{topic_name}' for '{}'", $param);
                        Ok(Some(Self {
                            sub: ctx.subscribe(&topic_name).await?,
                        }))
                    }
                    Err(e) => {
                        info!(
                            "Skipping subscriber creation for '{}': parameter not found, get error '{e}'",
                            $param
                        );
                        Ok(None)
                    }
                }
            }

            /// Reads message, returning rollback messages as well.
            /// Unexpected message (not applicable to the topic and not a rollback)
            /// results in error.
            pub async fn read_with_rollbacks(&mut self) -> Result<RollbackWrapper<$msg_type>> {
                let res = self.sub.read().await?.1;
                match res.as_ref() {
                    Message::Cardano((blk, CardanoMessage::$msg_constructor(body))) => Ok(
                        RollbackWrapper::Normal((Arc::new(blk.clone()), Arc::new(body.clone()))),
                    ),
                    Message::Cardano((
                        blk,
                        CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_)),
                    )) => Ok(RollbackWrapper::Rollback((Arc::new(blk.clone()),res.clone()))),
                    msg => bail!("Unexpected message {msg:?} for {}", $param),
                }
            }
        }
    };
}

pub struct ValidationContext {
    context: Arc<Context<Message>>,
    current_block: Option<Arc<BlockInfo>>,
    validation: ValidationOutcomes,
    validation_topic: String,
    module: String,
}

impl ValidationContext {
    pub fn new(context: &Arc<Context<Message>>, validation_topic: &str, module: &str) -> Self {
        Self {
            validation: ValidationOutcomes::new(),
            current_block: None,
            context: context.clone(),
            validation_topic: validation_topic.to_owned(),
            module: module.to_owned(),
        }
    }

    pub fn get_block_info(&self) -> Result<Arc<BlockInfo>> {
        self.current_block.as_ref().ok_or_else(|| anyhow!("Current block missing")).cloned()
    }

    pub fn get_validation(&mut self) -> &mut ValidationOutcomes {
        &mut self.validation
    }

    pub fn get_current_block_opt(&self) -> Option<Arc<BlockInfo>> {
        self.current_block.clone()
    }

    pub fn handle_error(&mut self, handler: &str, error: &anyhow::Error) {
        let msg = format!("Error in module {}, {handler}: {error:#}", self.module);
        error!("{msg}");
        self.validation.push_anyhow(anyhow!("{msg}"));
    }

    /// Adds given `outcome` to the validation outcomes list.
    /// If the `outcome` is 'Err', then the error is added to the outcomes list instead,
    /// annotated with `handler` string.
    pub fn merge(&mut self, handler: &str, outcome: Result<ValidationOutcomes>) {
        match outcome {
            Err(e) => self.handle_error(handler, &e),
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
                self.handle_error(handler, &e);
                T::default()
            }
        }
    }

    /// Analyzes message retrieved from a subscriber used for synchronization:
    /// * checks errors (adds them to validation outcome);
    /// * sets current_block (in case the message is not empty).
    pub fn consume_sync<T>(
        &mut self,
        handler: &str,
        inp: Result<RollbackWrapper<T>>,
    ) -> Result<RollbackWrapper<T>> {
        match &inp {
            Ok(RollbackWrapper::Normal((blk, _msg))) => {
                if self.current_block.is_some() {
                    self.check_sync(handler, blk);
                } else {
                    self.current_block = Some(blk.clone());
                }
            }
            Ok(RollbackWrapper::Rollback((blk, _msg))) => {
                if self.current_block.is_some() {
                    self.check_sync(handler, blk);
                } else {
                    self.current_block = Some(blk.clone());
                }
            }
            Err(e) => {
                self.current_block = None;
                bail!("Error handling sync block: {e}");
            }
        }
        inp
    }

    pub async fn publish(&mut self) {
        if let Some(blk) = &self.current_block {
            if let Err(e) = self
                .validation
                .publish(&self.context, &self.module, &self.validation_topic, blk)
                .await
            {
                error!("Publish failed in {}: {:?}", self.module, e);
            }
        } else {
            self.validation.print_errors(&self.module, None);
        }
    }

    /// Check for synchronisation
    fn check_sync(&mut self, handler: &str, actual: &BlockInfo) {
        if let Some(ref block) = self.current_block {
            if block.number != actual.number {
                self.handle_error(
                    handler,
                    &anyhow!(
                        "Messages out of sync: expected {:?}, actual {:?}",
                        block,
                        actual
                    ),
                );
            }
        }
    }
}
