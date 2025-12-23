use std::sync::Arc;

use anyhow::Result;
use caryatid_sdk::{async_trait, Context, MessageBounds, Subscription};
use crate::messages::{CardanoMessage, Message, StateTransitionMessage};

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

#[macro_export]
macro_rules! declare_cardano_reader_with_rollback {
    ($name:ident, $msg_constructor:ident, $msg_type:ty) => {
        async fn $name<'a>(s: &'a mut Box<&'a mut dyn Subscription<Message>>) -> Result<(BlockInfo, RollbackWrapper<$msg_type>)> {
            let (_,msg) = s.read().await?;
            match msg.as_ref() { //s.read_ignoring_rollbacks().await?.1.as_ref() {
                Message::Cardano((blk, CardanoMessage::$msg_constructor(body))) => {
                    Ok((blk.clone(), RollbackWrapper::Normal(body.clone())))
                }
                Message::Cardano((blk, CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_)))) => {
                    Ok((blk.clone(), RollbackWrapper::Rollback(msg.clone())))
                }
                msg => Err(anyhow!(
                    "Unexpected message {msg:?} for {} topic",
                    stringify!($msg_constructor)
                )),
            }
        }
    };
}

pub enum RollbackWrapper<T> {
    Rollback(Arc<Message>),
    Normal(T)
}

#[macro_export]
macro_rules! declare_cardano_rdr {
    ($reader_name:ident, $param:expr, $def_topic:expr, $msg_constructor:ident, $msg_type:ty) => {
        pub struct $reader_name {
            sub: Option<Box<dyn Subscription<Message>>>,
        }

        impl $reader_name {
            pub async fn new(
                ctx: &Context<Message>,
                cfg: &Arc<Config>,
            ) -> Result<Self> {
                let topic_name = cfg.get($param).unwrap_or($def_topic);

                info!("Creating subscriber on '{topic_name}' for '{}'", $param);
                let subscription = ctx.subscribe(&topic_name).await?;

                Ok (Self {
                    sub: Some(subscription),
                })
            }

            pub fn new_none() -> Self {
                Self {
                    sub: None,
                }
            }

            pub async fn read_rb(&mut self) -> Result<(BlockInfo, RollbackWrapper<$msg_type>)> {
                let Some(sub) = self.sub.as_mut() else {
                    bail!("Subscription '{}' is disabled, cannot read", $param);
                };

                let res = sub.read().await?.1;
                match res.as_ref() {
                    Message::Cardano((blk, CardanoMessage::$msg_constructor(body))) => {
                        Ok((blk.clone(), RollbackWrapper::Normal(body.clone())))
                    },
                    Message::Cardano((
                        blk,
                        CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_))
                    )) => {
                        Ok((blk.clone(), RollbackWrapper::Rollback(res.clone())))
                    }
                    msg => bail!("Unexpected message {msg:?} for {}", $param),
                }
            }

            pub async fn read(&mut self) -> Result<(BlockInfo, $msg_type)> {
                let Some(sub) = self.sub.as_mut() else {
                    bail!("Subscription '{}' is disabled, cannot read", $param);
                };

                match sub.read_ignoring_rollbacks().await?.1.as_ref() {
                    Message::Cardano((blk, CardanoMessage::$msg_constructor(body))) => {
                        Ok((blk.clone(), body.clone()))
                    },
                    msg => bail!(
                        "Unexpected message {msg:?} for '{}' topic",
                        stringify!($msg_constructor)
                    ),
                }
            }

            pub async fn read_opt(&mut self) -> Result<Option<(BlockInfo, $msg_type)>> {
                match self.sub {
                    None => Ok(None),
                    Some(_) => Ok(Some(self.read().await?))
                }
            }
        }
    };
}
