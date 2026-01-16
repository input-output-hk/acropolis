//! MCP Tools
//!
//! Exposes Blockfrost endpoints as MCP tools using the shared routes registry.
//! Tools are callable functions that return JSON data from the blockchain.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use config::Config;
use serde_json::{json, Map};

use acropolis_common::messages::Message;
use caryatid_sdk::Context;

use acropolis_module_rest_blockfrost::routes::{RouteDefinition, ROUTES};

use rmcp::model::{CallToolResult, JsonObject, ListToolsResult, Tool, ToolAnnotations};

use crate::resources::handle_resource_with_query;

/// Convert a route name to a tool name (snake_case)
fn route_to_tool_name(route: &RouteDefinition) -> String {
    // Convert "Account Information" -> "get_account_information"
    // Convert "Epoch Info" -> "get_epoch_info"
    let name = route.name.to_lowercase().replace(' ', "_").replace('-', "_");
    format!("get_{}", name)
}

/// Build the JSON Schema for a tool's input parameters based on route param_names
fn build_input_schema(route: &RouteDefinition) -> Arc<JsonObject> {
    let mut properties = Map::new();
    let mut required = Vec::new();

    for &param in route.param_names {
        let param_description = match param {
            "stake_address" => "Bech32 stake address (e.g., stake1u...)",
            "hash_or_number" => "Block hash or number (use 'latest' for most recent)",
            "number" | "epoch_number" => "Epoch number",
            "slot" => "Slot number",
            "epoch_slot" => "Slot within the epoch",
            "drep_id" => "DRep ID (bech32 or hex)",
            "tx_hash" => "Transaction hash",
            "pool_id" => "Pool ID (bech32 pool...)",
            "asset" => "Asset identifier (policy_id + hex-encoded asset_name)",
            "policy_id" => "Policy ID",
            "address" => "Cardano address (bech32)",
            _ => "Parameter value",
        };

        properties.insert(
            param.to_string(),
            json!({
                "type": "string",
                "description": param_description
            }),
        );
        required.push(json!(param));
    }

    let schema = json!({
        "type": "object",
        "properties": properties,
        "required": required
    });

    Arc::new(schema.as_object().unwrap().clone())
}

/// Build URI from route template and provided arguments
fn build_uri_from_args(route: &RouteDefinition, args: &JsonObject) -> Result<String> {
    let mut uri = route.mcp_uri_template.to_string();

    for &param in route.param_names {
        let value = args
            .get(param)
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: {}", param))?;

        uri = uri.replace(&format!("{{{}}}", param), value);
    }

    Ok(uri)
}

/// Get all available MCP tools from the routes registry
pub fn get_all_tools() -> Vec<Tool> {
    ROUTES
        .iter()
        .map(|route| Tool {
            name: route_to_tool_name(route).into(),
            title: Some(route.name.to_string()),
            description: Some(route.description.into()),
            input_schema: build_input_schema(route),
            output_schema: None,
            annotations: Some(
                ToolAnnotations::new()
                    .read_only(true)
                    .destructive(false)
                    .idempotent(true)
                    .open_world(false),
            ),
            icons: None,
        })
        .collect()
}

/// Get list_tools result
pub fn list_tools_result() -> ListToolsResult {
    ListToolsResult {
        tools: get_all_tools(),
        next_cursor: None,
    }
}

/// Find a route by tool name
fn find_route_by_tool_name(tool_name: &str) -> Option<&'static RouteDefinition> {
    ROUTES.iter().find(|route| route_to_tool_name(route) == tool_name)
}

/// Handle a tool call by dispatching to the appropriate Blockfrost handler
pub async fn handle_tool_call(
    context: Arc<Context<Message>>,
    config: Arc<Config>,
    tool_name: &str,
    arguments: Option<JsonObject>,
) -> Result<CallToolResult> {
    // Find the route for this tool
    let route = find_route_by_tool_name(tool_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown tool: {}", tool_name))?;

    let args = arguments.unwrap_or_default();

    // Build the URI from the template and arguments
    let uri = build_uri_from_args(route, &args)?;

    // Extract any additional query parameters (non-path params)
    let query_params: HashMap<String, String> = args
        .iter()
        .filter(|(k, _)| !route.param_names.contains(&k.as_str()))
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
        .collect();

    // Call the existing resource handler
    let json_result = handle_resource_with_query(context, config, &uri, query_params).await?;

    // Return as tool result
    Ok(CallToolResult::success(vec![rmcp::model::Content::json(
        json_result,
    )
    .map_err(|e| anyhow::anyhow!("JSON error: {}", e))?]))
}
