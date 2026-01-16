//! MCP Server Implementation
//!
//! Exposes Blockfrost endpoints as MCP resources and tools using the shared routes registry.
//! Uses Streamable HTTP transport for integration with omnibus.

use std::sync::Arc;

use anyhow::Result;
use config::Config;
use tokio::net::TcpListener;
use tracing::info;

use acropolis_common::messages::Message;
use caryatid_sdk::Context;

use rmcp::model::{
    Annotated, CallToolRequestParam, CallToolResult, ErrorCode, ErrorData, Implementation,
    ListResourcesResult, ListToolsResult, PaginatedRequestParam, RawResource,
    ReadResourceRequestParam, ReadResourceResult, ResourceContents, ServerCapabilities, ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use rmcp::{RoleServer, ServerHandler};

use crate::resources::{get_all_resources, handle_resource};
use crate::tools::{handle_tool_call, list_tools_result};

/// MCP Server that wraps Blockfrost handlers
#[derive(Clone)]
pub struct AcropolisMCPServer {
    context: Arc<Context<Message>>,
    config: Arc<Config>,
}

impl AcropolisMCPServer {
    pub fn new(context: Arc<Context<Message>>, config: Arc<Config>) -> Self {
        Self { context, config }
    }

    /// Start the MCP server on HTTP with Streamable HTTP transport
    pub async fn run(self, address: &str, port: u16) -> Result<()> {
        let bind_addr = format!("{address}:{port}");
        info!("Starting Acropolis MCP server on http://{}/mcp", bind_addr);

        // Create the streamable HTTP service
        let context = self.context.clone();
        let config = self.config.clone();
        let service = StreamableHttpService::new(
            move || Ok(AcropolisMCPServer::new(context.clone(), config.clone())),
            LocalSessionManager::default().into(),
            StreamableHttpServerConfig::default(),
        );

        // Build axum router with MCP endpoint
        let router = axum::Router::new().nest_service("/mcp", service);

        // Start the HTTP server
        let listener = TcpListener::bind(&bind_addr).await?;
        info!("MCP server listening on http://{}/mcp", bind_addr);

        axum::serve(listener, router).await?;

        Ok(())
    }
}

impl ServerHandler for AcropolisMCPServer {
    fn get_info(&self) -> ServerInfo {
        let tools_count = crate::tools::get_all_tools().len();
        let resources_count = get_all_resources().len();
        info!(
            "MCP get_info called - advertising {} tools and {} resources",
            tools_count, resources_count
        );
        ServerInfo {
            protocol_version: Default::default(),
            capabilities: ServerCapabilities::builder()
                .enable_resources()
                .enable_tools()
                .build(),
            server_info: Implementation {
                name: "acropolis-mcp".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                title: Some("Acropolis MCP Server".to_string()),
                website_url: None,
                icons: None,
            },
            instructions: Some(
                "Acropolis MCP server provides Cardano blockchain data via Blockfrost-compatible API. \
                 Use tools like get_epoch_info, get_block_information, get_pool_info etc. to query blockchain state."
                    .to_string(),
            ),
        }
    }

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
                    format!("Failed to handle resource: {e}"),
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
                        format!("Failed to serialize JSON: {e}"),
                        None,
                    )
                })?,
                meta: None,
            }],
        })
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let result = list_tools_result();
        info!(
            "MCP client requested tools list - returning {} tools",
            result.tools.len()
        );
        Ok(result)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        info!("MCP client calling tool: {}", request.name);

        handle_tool_call(
            self.context.clone(),
            self.config.clone(),
            &request.name,
            request.arguments,
        )
        .await
        .map_err(|e| {
            ErrorData::new(
                ErrorCode::INTERNAL_ERROR,
                format!("Tool call failed: {e}"),
                None,
            )
        })
    }
}
