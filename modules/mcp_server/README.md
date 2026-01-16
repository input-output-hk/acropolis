# Acropolis MCP Server

This module provides a Model Context Protocol (MCP) server that exposes Blockfrost-compatible API endpoints as MCP tools.

## Overview

The MCP server integrates with the existing Blockfrost REST handlers in acropolis, reusing the existing query logic and simply reformatting responses for MCP clients like Claude Desktop and VS Code.

## Architecture

- **Tools**: MCP tools map to Blockfrost API endpoints (60+ endpoints available)
- **Handler Reuse**: Calls existing Blockfrost handlers from `rest_blockfrost` module
- **Transport**: HTTP with Server-Sent Events (SSE) on configurable port (default: 4341)

## Configuration

Add to your acropolis config file:

```toml
[module.mcp-server]
enabled = true
address = "127.0.0.1"
port = 4341
```

## Usage with VS Code

The MCP server integrates with VS Code's Copilot via the `.vscode/mcp.json` configuration. When Acropolis is running with MCP enabled, Copilot can query blockchain data directly.

## Usage with Claude Desktop

Claude Desktop requires an stdio transport adapter. Create a simple connection script or use the HTTP endpoint directly if your MCP client supports HTTP/SSE transport.

For HTTP/SSE clients, connect to:
```
http://127.0.0.1:4341/sse
```

## Available Tools

The server exposes 60+ Blockfrost-compatible API endpoints as MCP tools, including:

- `get_epoch_information` - Epoch info (use "latest" for current)
- `get_epoch_parameters` - Protocol parameters for an epoch
- `get_block_information` - Block details by hash or number
- `get_account_information` - Stake account details
- `get_address_extended` - Address information
- `get_pool_information` - Stake pool details
- `get_drep_info` - DRep information (Conway era)
- `get_asset_information` - Native token details
- And many more...

## Testing

Use the MCP Inspector to test the server:
```bash
npx @anthropics/mcp-inspector
```

Or run the test scripts:
```bash
python3 scripts/test_mcp_standalone.py
```
