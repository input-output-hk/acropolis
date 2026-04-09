//! Acropolis MCP (Model Context Protocol) Server Module
//!
//! This module exposes Blockfrost API functionality as MCP resources and tools,
//! allowing AI assistants to query blockchain data.
//!
//! The server runs on HTTP with SSE transport, enabling AI clients to connect
//! without interfering with the main process's stdio.

use std::sync::Arc;

use anyhow::Result;
use caryatid_sdk::{module, Context};
use config::Config;
use tracing::info;

use acropolis_common::{
    configuration::{get_bool_flag, get_string_flag, get_u64_flag},
    messages::Message,
};

mod resources;
mod server;
mod tools;

use server::AcropolisMCPServer;

/// Default MCP server address
const DEFAULT_MCP_ADDRESS: (&str, &str) = ("address", "127.0.0.1");
/// Default MCP server port
const DEFAULT_MCP_PORT: (&str, u64) = ("port", 4341);
/// Default enabled status
const DEFAULT_ENABLED: (&str, bool) = ("enabled", false);

#[module(
    message_type(Message),
    name = "mcp-server",
    description = "Model Context Protocol server for Acropolis"
)]
pub struct MCPServer;

impl MCPServer {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Check if MCP server is enabled in config (under [module.mcp-server])
        // Note: `config` is already scoped to the module's config section
        let enabled = get_bool_flag(&config, DEFAULT_ENABLED);

        if !enabled {
            info!("MCP server is disabled in configuration");
            return Ok(());
        }

        // Get address and port from module config
        let address = get_string_flag(&config, DEFAULT_MCP_ADDRESS);
        let port: u16 = get_u64_flag(&config, DEFAULT_MCP_PORT).try_into()?;

        info!("Initializing MCP server on {}:{}", address, port);

        // Create and start the MCP server
        let server = AcropolisMCPServer::new(context.clone(), config.clone());

        // Start the server in a background task
        tokio::spawn(async move {
            if let Err(e) = server.run(&address, port).await {
                tracing::error!("MCP server error: {}", e);
            }
        });

        info!("MCP server started");

        Ok(())
    }
}
