//! Helper functions for REST handlers

use crate::rest_error::RESTError;
use crate::messages::{Message, RESTResponse};
use anyhow::{anyhow, Result};
use caryatid_sdk::Context;
use futures::future::Future;
use num_traits::ToPrimitive;
use std::{collections::HashMap, sync::Arc};
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
    Fut: Future<Output = Result<RESTResponse, RESTError>> + Send + 'static,
{
    context.handle(topic, move |message: Arc<Message>| {
        let handler = handler.clone();
        async move {
            let response = match message.as_ref() {
                Message::RESTRequest(request) => {
                    info!("REST received {} {}", request.method, request.path);
                    handler().await.unwrap_or_else(|error| error.into())
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

/// Handle a REST request with path parameters
pub fn handle_rest_with_path_parameter<F, Fut>(
    context: Arc<Context<Message>>,
    topic: &str,
    handler: F,
) -> JoinHandle<()>
where
    F: Fn(&[&str]) -> Fut + Send + Sync + Clone + 'static,
    Fut: Future<Output = Result<RESTResponse, RESTError>> + Send + 'static,
{
    let topic_owned = topic.to_string();
    context.handle(topic, move |message: Arc<Message>| {
        let handler = handler.clone();
        let topic_owned = topic_owned.clone();
        async move {
            let response = match message.as_ref() {
                Message::RESTRequest(request) => {
                    info!("REST received {} {}", request.method, request.path);
                    let params_vec =
                        extract_params_from_topic_and_path(&topic_owned, &request.path_elements);
                    let params_slice: Vec<&str> = params_vec.iter().map(|s| s.as_str()).collect();

                    handler(&params_slice).await.unwrap_or_else(|error| error.into())
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

/// Handle a REST request with query parameters
pub fn handle_rest_with_query_parameters<F, Fut>(
    context: Arc<Context<Message>>,
    topic: &str,
    handler: F,
) -> JoinHandle<()>
where
    F: Fn(HashMap<String, String>) -> Fut + Send + Sync + Clone + 'static,
    Fut: Future<Output = Result<RESTResponse, RESTError>> + Send + 'static,
{
    context.handle(topic, move |message: Arc<Message>| {
        let handler = handler.clone();
        async move {
            let response = match message.as_ref() {
                Message::RESTRequest(request) => {
                    let params = request.query_parameters.clone();
                    handler(params).await.unwrap_or_else(|error| error.into())
                }
                _ => RESTResponse::with_text(500, "Unexpected message in REST request"),
            };

            Arc::new(Message::RESTResponse(response))
        }
    })
}

/// Handle a REST request with path and query parameters
pub fn handle_rest_with_path_and_query_parameters<F, Fut>(
    context: Arc<Context<Message>>,
    topic: &str,
    handler: F,
) -> JoinHandle<()>
where
    F: Fn(&[&str], HashMap<String, String>) -> Fut + Send + Sync + Clone + 'static,
    Fut: Future<Output = Result<RESTResponse, RESTError>> + Send + 'static,
{
    let topic_owned = topic.to_string();
    context.handle(topic, move |message: Arc<Message>| {
        let handler = handler.clone();
        let topic_owned = topic_owned.clone();
        async move {
            let response = match message.as_ref() {
                Message::RESTRequest(request) => {
                    let params_vec =
                        extract_params_from_topic_and_path(&topic_owned, &request.path_elements);
                    let params_slice: Vec<&str> = params_vec.iter().map(|s| s.as_str()).collect();
                    let query_params = request.query_parameters.clone();
                    handler(&params_slice, query_params).await.unwrap_or_else(|error| error.into())
                }
                _ => RESTResponse::with_text(500, "Unexpected message in REST request"),
            };

            Arc::new(Message::RESTResponse(response))
        }
    })
}

/// Extract parameters from the request path based on the topic pattern.
/// Skips the first 3 parts of the topic as these are never parameters
fn extract_params_from_topic_and_path(topic: &str, path_elements: &[String]) -> Vec<String> {
    let topic_parts: Vec<&str> = topic.split('.').collect();

    // Find indexes of '*' in topic
    let param_positions: Vec<usize> = topic_parts
        .iter()
        .enumerate()
        .filter_map(|(i, &part)| if part == "*" { Some(i) } else { None })
        .collect();

    let offset = 2;

    // Map topic '*' positions to corresponding path elements
    param_positions
        .iter()
        .filter_map(|&pos| pos.checked_sub(offset).and_then(|idx| path_elements.get(idx)).cloned())
        .collect()
}

pub trait ToCheckedF64 {
    fn to_checked_f64(&self, name: &str) -> Result<f64>;
}

impl<T: ToPrimitive> ToCheckedF64 for T {
    fn to_checked_f64(&self, name: &str) -> Result<f64> {
        self.to_f64().ok_or_else(|| anyhow!("Failed to convert {name} to f64"))
    }
}

// Macros for extracting and validating REST query parameters
#[macro_export]
macro_rules! extract_strict_query_params {
    ($params:expr, { $($key:literal => $var:ident : Option<$type:ty>,)* }) => {
        $(
            let mut $var: Option<$type> = None;
        )*

        for (k, v) in &$params {
            match k.as_str() {
                $(
                    $key => {
                        $var = match v.parse::<$type>() {
                            Ok(val) => Some(val),
                            Err(_) => {
                                return Ok($crate::messages::RESTResponse::with_text(
                                    400,
                                    concat!("Invalid ", $key, " query parameter: must be a valid type"),
                                ));
                            }
                        };
                    }
                )*
                _ => {
                    return Ok($crate::messages::RESTResponse::with_text(
                        400,
                        concat!("Unexpected query parameter: only allowed keys are: ", $( $key, " ", )*)
                    ));
                }
            }
        }
    };
}
