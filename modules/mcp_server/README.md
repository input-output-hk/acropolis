# Acropolis MCP Server

This module provides a Model Context Protocol (MCP) server that exposes Blockfrost-compatible API endpoints as MCP resources.

## Overview

The MCP server integrates with the existing Blockfrost REST handlers in acropolis, reusing the existing query logic and simply reformatting responses for MCP clients like Claude Desktop.

## Architecture

- **Resources**: MCP resources map to Blockfrost API endpoints
- **Handler Reuse**: Calls existing Blockfrost handlers from `rest_blockfrost` module
- **Protocol**: Implements MCP over stdio using the `rmcp` crate

## Supported Resources

Currently implemented MCP resources:

1. `blockfrost://network/info` - Current network information
2. `blockfrost://epochs/{epoch_number}` - Epoch information (use "latest" for current)
3. `blockfrost://epochs/{epoch_number}/parameters` - Protocol parameters
4. `blockfrost://blocks/{hash_or_number}` - Block information

## Configuration

Add to your acropolis config file:

```toml
[mcp]
enabled = true
epochs_query_topic = "query.epochs"
historical_epochs_query_topic = "query.historical-epochs"
parameters_query_topic = "query.parameters"
blocks_query_topic = "query.blocks"
```

## Usage with Claude Desktop

Add to Claude Desktop configuration:

```json
{
  "mcpServers": {
    "acropolis": {
      "command": "/path/to/acropolis",
      "args": ["--mcp-mode"],
      "env": {}
    }
  }
}
```

## Future Work

- Add more Blockfrost endpoints as MCP resources
- Implement MCP tools for write operations (transaction submission)
- Add resource subscriptions for real-time updates
- Add prompt templates for common queries
