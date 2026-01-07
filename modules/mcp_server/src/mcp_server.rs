//! Acropolis MCP (Model Context Protocol) Server Module
//!
//! This module exposes Blockfrost API functionality as MCP resources,
//! allowing AI assistants to query blockchain data.

use std::sync::Arc;

use anyhow::Result;
use caryatid_sdk::{module, Context};
use config::Config;
use tracing::info;

use acropolis_common::messages::Message;

mod resources;
mod server;

use server::AcropolisMCPServer;

#[module(
    message_type(Message),
    name = "mcp-server",
    description = "Model Context Protocol server for Acropolis"
)]
pub struct MCPServer;

impl MCPServer {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Check if MCP server is enabled in config
        let enabled = config.get_bool("mcp.enabled").unwrap_or(false);

        if !enabled {
            info!("MCP server is disabled in configuration");
            return Ok(());
        }

        info!("Initializing MCP server");

        // Create and start the MCP server
        let server = AcropolisMCPServer::new(context.clone(), config.clone());

        // Start the server in a background task
        tokio::spawn(async move {
            if let Err(e) = server.run().await {
                tracing::error!("MCP server error: {}", e);
            }
        });

        info!("MCP server started");

        Ok(())
    }
}
