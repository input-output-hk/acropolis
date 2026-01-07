//! MCP Server Implementation
//!
//! Exposes Blockfrost endpoints as MCP resources using the shared routes registry.

use std::sync::Arc;

use anyhow::Result;
use config::Config;
use tracing::info;

use acropolis_common::messages::Message;
use caryatid_sdk::Context;

use rmcp::model::{
    Annotated, ErrorCode, ErrorData, ListResourcesResult, PaginatedRequestParam, RawResource,
    ReadResourceRequestParam, ReadResourceResult, ResourceContents,
};
use rmcp::service::RequestContext;
use rmcp::transport::stdio;
use rmcp::{RoleServer, ServerHandler};

use crate::resources::{get_all_resources, handle_resource};

pub struct AcropolisMCPServer {
    context: Arc<Context<Message>>,
    config: Arc<Config>,
}

impl AcropolisMCPServer {
    pub fn new(context: Arc<Context<Message>>, config: Arc<Config>) -> Self {
        Self { context, config }
    }

    pub async fn run(self) -> Result<()> {
        info!("Starting Acropolis MCP server on stdio");

        let transport = stdio();
        let _service = rmcp::service::serve_server(self, transport).await?;

        // Note: RunningService doesn't have a run() method
        // It handles the protocol automatically
        // Keep the service alive
        tokio::signal::ctrl_c().await?;
        info!("MCP server shutting down");

        Ok(())
    }
}

impl ServerHandler for AcropolisMCPServer {
    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        info!("MCP client requested resource list");

        // Build resource list from the shared routes registry
        let resources: Vec<Annotated<RawResource>> = get_all_resources()
            .iter()
            .map(|route| Annotated {
                raw: RawResource {
                    uri: route.mcp_uri_template.to_string(),
                    name: route.name.to_string(),
                    title: None,
                    description: Some(route.description.to_string()),
                    mime_type: Some("application/json".to_string()),
                    size: None,
                    icons: None,
                },
                annotations: None,
            })
            .collect();

        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        info!("MCP client reading resource: {}", request.uri);

        // Use the generic handler that dispatches based on URI
        let json_result = handle_resource(self.context.clone(), self.config.clone(), &request.uri)
            .await
            .map_err(|e| {
                ErrorData::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Failed to handle resource: {}", e),
                    None,
                )
            })?;

        // Return as MCP resource
        Ok(ReadResourceResult {
            contents: vec![ResourceContents::TextResourceContents {
                uri: request.uri,
                mime_type: Some("application/json".to_string()),
                text: serde_json::to_string_pretty(&json_result).map_err(|e| {
                    ErrorData::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to serialize JSON: {}", e),
                        None,
                    )
                })?,
                meta: None,
            }],
        })
    }
}
