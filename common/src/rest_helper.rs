//! Helper functions for REST handlers

use crate::messages::{Message, RESTResponse};
use anyhow::Result;
use caryatid_sdk::Context;
use futures::future::Future;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{error, info};

/// Handle a simple REST request with no parameters
pub fn handle_rest<F, Fut>(
    context: Arc<Context<Message>>,
    topic: &str,
    handler: F,
) -> JoinHandle<()>
where
    F: Fn() -> Fut + Send + Sync + Clone + 'static,
    Fut: Future<Output = Result<RESTResponse>> + Send + 'static,
{
    context.handle(topic, move |message: Arc<Message>| {
        let handler = handler.clone();
        async move {
            let response = match message.as_ref() {
                Message::RESTRequest(request) => {
                    info!("REST received {} {}", request.method, request.path);
                    match handler().await {
                        Ok(response) => response,
                        Err(error) => {
                            RESTResponse::with_text(500, &format!("{error:?}").to_string())
                        }
                    }
                }
                _ => {
                    error!("Unexpected message type {:?}", message);
                    RESTResponse::with_text(500, "Unexpected message in REST request")
                }
            };

            Arc::new(Message::RESTResponse(response))
        }
    })
}

/// Handle a simple REST request with one path parameter
pub fn handle_rest_with_parameter<F, Fut>(
    context: Arc<Context<Message>>,
    topic: &str,
    handler: F,
) -> JoinHandle<()>
where
    F: Fn(&str) -> Fut + Send + Sync + Clone + 'static,
    Fut: Future<Output = Result<RESTResponse>> + Send + 'static,
{
    context.handle(topic, move |message: Arc<Message>| {
        let handler = handler.clone();
        async move {
            let response = match message.as_ref() {
                Message::RESTRequest(request) => {
                    info!("REST received {} {}", request.method, request.path);
                    match request.path_elements.get(1) {
                        Some(param) => match handler(param).await {
                            Ok(response) => response,
                            Err(error) => {
                                RESTResponse::with_text(500, &format!("{error:?}").to_string())
                            }
                        },
                        None => RESTResponse::with_text(400, "Parameter must be provided"),
                    }
                }
                _ => {
                    error!("Unexpected message type {:?}", message);
                    RESTResponse::with_text(500, "Unexpected message in REST request")
                }
            };

            Arc::new(Message::RESTResponse(response))
        }
    })
}
