use std::sync::Arc;

use crate::messages::{CardanoMessage, Message, StateTransitionMessage};
use crate::types::BlockInfo;
use crate::validation::ValidationOutcomes;
use anyhow::{anyhow, bail, Result};
use caryatid_sdk::{Context, MessageBounds};
use tracing::error;

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

#[derive(Debug)]
pub enum RollbackWrapper<T> {
    Rollback((Arc<BlockInfo>, Arc<Message>)),
    Normal((Arc<BlockInfo>, Arc<T>)),
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
    current_wrapper: Option<SyncMessageWrapper>,
    validation: ValidationOutcomes,
    validation_topic: String,
    module: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SyncMessageWrapper {
    Normal,
    Rollback,
}

impl ValidationContext {
    pub fn new(context: &Arc<Context<Message>>, validation_topic: &str, module: &str) -> Self {
        Self {
            validation: ValidationOutcomes::new(),
            current_block: None,
            current_wrapper: None,
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
                self.check_sync_wrapper(handler, SyncMessageWrapper::Normal);
                if self.current_block.is_some() {
                    self.check_sync(handler, blk);
                } else {
                    self.current_block = Some(blk.clone());
                }
            }
            Ok(RollbackWrapper::Rollback((blk, _msg))) => {
                self.check_sync_wrapper(handler, SyncMessageWrapper::Rollback);
                if self.current_block.is_some() {
                    self.check_sync(handler, blk);
                } else {
                    self.current_block = Some(blk.clone());
                }
            }
            Err(e) => {
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

    fn check_sync_wrapper(&mut self, handler: &str, actual: SyncMessageWrapper) {
        if let Some(expected) = self.current_wrapper {
            if expected != actual {
                self.handle_error(
                    handler,
                    &anyhow!(
                        "Messages out of sync wrapper: expected {:?}, actual {:?}",
                        expected,
                        actual
                    ),
                );
            }
        } else {
            self.current_wrapper = Some(actual);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use caryatid_sdk::{async_trait, MessageBus, Subscription};
    use config::Config;
    use std::time::Duration;

    struct DummyBus;

    #[async_trait]
    impl MessageBus<Message> for DummyBus {
        async fn publish(&self, _topic: &str, _message: Arc<Message>) -> Result<()> {
            Ok(())
        }

        fn request_timeout(&self) -> Duration {
            Duration::from_secs(1)
        }

        async fn request(&self, _topic: &str, _message: Arc<Message>) -> Result<Arc<Message>> {
            Err(anyhow!("unsupported request in tests"))
        }

        async fn subscribe(&self, _topic: &str) -> Result<Box<dyn Subscription<Message>>> {
            Err(anyhow!("unsupported subscribe in tests"))
        }

        async fn shutdown(&self) -> Result<()> {
            Ok(())
        }
    }

    fn create_validation_context() -> ValidationContext {
        let (_, startup_watch) = tokio::sync::watch::channel(true);
        ValidationContext::new(
            &Arc::new(Context {
                config: Arc::new(Config::default()),
                message_bus: Arc::new(DummyBus),
                startup_watch,
            }),
            "test.validation",
            "test_module",
        )
    }

    fn test_block(number: u64) -> BlockInfo {
        BlockInfo {
            status: crate::BlockStatus::Volatile,
            intent: crate::BlockIntent::ValidateAndApply,
            slot: number,
            number,
            hash: crate::BlockHash::default(),
            epoch: 0,
            epoch_slot: 0,
            new_epoch: false,
            is_new_era: false,
            tip_slot: None,
            timestamp: 0,
            era: crate::Era::default(),
        }
    }

    fn rollback_message(block: &BlockInfo) -> Arc<Message> {
        Arc::new(Message::Cardano((
            block.clone(),
            CardanoMessage::StateTransition(StateTransitionMessage::Rollback(crate::Point::Origin)),
        )))
    }

    fn assert_clean_validation(ctx: &mut ValidationContext) {
        assert!(
            ctx.get_validation().as_result().is_ok(),
            "validation should remain clean"
        );
    }

    #[test]
    fn consume_sync_sets_current_block_from_first_rollback() {
        let mut ctx = create_validation_context();
        let block = Arc::new(test_block(10));
        let rollback = rollback_message(block.as_ref());

        let consumed = ctx
            .consume_sync(
                "rollback",
                Ok::<_, anyhow::Error>(RollbackWrapper::<u8>::Rollback((block.clone(), rollback))),
            )
            .expect("rollback should be consumed");

        assert!(matches!(consumed, RollbackWrapper::Rollback(_)));
        let current = ctx.get_current_block_opt().expect("current block should be set");
        assert_eq!(current.number, block.number);
    }

    #[test]
    fn consume_sync_records_validation_error_for_mismatched_rollback_block() {
        let mut ctx = create_validation_context();
        let first = Arc::new(test_block(10));
        let second = Arc::new(test_block(11));

        ctx.consume_sync(
            "normal",
            Ok::<_, anyhow::Error>(RollbackWrapper::<u8>::Normal((first.clone(), Arc::new(1)))),
        )
        .expect("normal sync should succeed");

        ctx.consume_sync(
            "rollback",
            Ok::<_, anyhow::Error>(RollbackWrapper::<u8>::Rollback((
                second.clone(),
                rollback_message(second.as_ref()),
            ))),
        )
        .expect("rollback sync should succeed");

        assert_eq!(
            ctx.get_current_block_opt().expect("current block should remain set").number,
            first.number
        );
        assert!(ctx.get_validation().as_result().is_err());
    }

    #[test]
    fn consume_sync_normal_then_matching_rollback_records_validation_error() {
        let mut ctx = create_validation_context();
        let block = Arc::new(test_block(42));

        ctx.consume_sync(
            "normal",
            Ok::<_, anyhow::Error>(RollbackWrapper::<u8>::Normal((block.clone(), Arc::new(7)))),
        )
        .expect("normal sync should succeed");

        ctx.consume_sync(
            "rollback",
            Ok::<_, anyhow::Error>(RollbackWrapper::<u8>::Rollback((
                block.clone(),
                rollback_message(block.as_ref()),
            ))),
        )
        .expect("rollback sync should succeed");

        assert_eq!(
            ctx.get_current_block_opt().expect("current block should remain set").number,
            block.number
        );
        assert!(ctx.get_validation().as_result().is_err());
    }

    #[test]
    fn consume_sync_rollback_then_matching_normal_records_validation_error() {
        let mut ctx = create_validation_context();
        let block = Arc::new(test_block(24));

        ctx.consume_sync(
            "rollback",
            Ok::<_, anyhow::Error>(RollbackWrapper::<u8>::Rollback((
                block.clone(),
                rollback_message(block.as_ref()),
            ))),
        )
        .expect("rollback sync should succeed");

        ctx.consume_sync(
            "normal",
            Ok::<_, anyhow::Error>(RollbackWrapper::<u8>::Normal((block.clone(), Arc::new(3)))),
        )
        .expect("normal sync should succeed");

        assert_eq!(
            ctx.get_current_block_opt().expect("current block should remain set").number,
            block.number
        );
        assert!(ctx.get_validation().as_result().is_err());
    }

    #[test]
    fn consume_sync_error_preserves_current_block_for_later_sync_validation() {
        let mut ctx = create_validation_context();
        let block = Arc::new(test_block(9));

        ctx.consume_sync(
            "normal",
            Ok::<_, anyhow::Error>(RollbackWrapper::<u8>::Normal((block, Arc::new(1)))),
        )
        .expect("normal sync should succeed");
        assert!(ctx.get_current_block_opt().is_some());

        let err = ctx
            .consume_sync::<u8>("reader", Err(anyhow!("subscription failed")))
            .expect_err("consume_sync should return an error");
        assert!(
            err.to_string().contains("Error handling sync block"),
            "error should be wrapped by consume_sync"
        );
        assert!(
            ctx.get_current_block_opt().is_some(),
            "current block should be preserved on sync errors"
        );

        ctx.consume_sync(
            "post-error-mismatch",
            Ok::<_, anyhow::Error>(RollbackWrapper::<u8>::Normal((
                Arc::new(test_block(10)),
                Arc::new(2),
            ))),
        )
        .expect("subsequent sync call should still be processed");
        assert!(
            ctx.get_validation().as_result().is_err(),
            "post-error mismatch should still be detected"
        );
    }

    #[test]
    fn get_block_info_errors_when_no_sync_block_seen() {
        let ctx = create_validation_context();

        let err = ctx.get_block_info().expect_err("missing block info should error");
        assert!(
            err.to_string().contains("Current block missing"),
            "error message should explain missing current block"
        );
    }

    #[test]
    fn consume_sync_multiple_mismatches_keep_original_anchor_block() {
        let mut ctx = create_validation_context();
        let anchor = Arc::new(test_block(10));

        ctx.consume_sync(
            "normal",
            Ok::<_, anyhow::Error>(RollbackWrapper::<u8>::Normal((anchor.clone(), Arc::new(0)))),
        )
        .expect("initial sync should succeed");

        ctx.consume_sync(
            "mismatch-1",
            Ok::<_, anyhow::Error>(RollbackWrapper::<u8>::Rollback((
                Arc::new(test_block(11)),
                rollback_message(&test_block(11)),
            ))),
        )
        .expect("first mismatch should still return Ok wrapper");

        ctx.consume_sync(
            "mismatch-2",
            Ok::<_, anyhow::Error>(RollbackWrapper::<u8>::Normal((
                Arc::new(test_block(12)),
                Arc::new(5),
            ))),
        )
        .expect("second mismatch should still return Ok wrapper");

        assert_eq!(
            ctx.get_current_block_opt().expect("anchor block should remain set").number,
            anchor.number
        );
        assert!(ctx.get_validation().as_result().is_err());
    }

    #[test]
    fn consume_sync_matching_rollbacks_stay_in_sync() {
        let mut ctx = create_validation_context();
        let block = Arc::new(test_block(77));

        ctx.consume_sync(
            "rollback-1",
            Ok::<_, anyhow::Error>(RollbackWrapper::<u8>::Rollback((
                block.clone(),
                rollback_message(block.as_ref()),
            ))),
        )
        .expect("first rollback should succeed");

        ctx.consume_sync(
            "rollback-2",
            Ok::<_, anyhow::Error>(RollbackWrapper::<u8>::Rollback((
                block.clone(),
                rollback_message(block.as_ref()),
            ))),
        )
        .expect("second rollback should succeed");

        assert_eq!(
            ctx.get_current_block_opt().expect("current block should remain set").number,
            block.number
        );
        assert_clean_validation(&mut ctx);
    }
}
