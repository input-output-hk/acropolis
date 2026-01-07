#!/usr/bin/env python3
"""
Test script for Acropolis MCP Server

This script tests the MCP server by sending JSON-RPC requests via subprocess.
Requires the MCP server to be built and the Acropolis process to be running
with mcp.enabled=true in the configuration.

Usage:
    python3 scripts/test_mcp_server.py

Note: This is a basic test. For full integration testing, use the MCP Inspector:
    npx @anthropics/mcp-inspector
"""

import json
import subprocess
import sys

def send_jsonrpc(request: dict) -> dict:
    """Send a JSON-RPC request and get the response."""
    request_str = json.dumps(request) + "\n"
    print(f"→ Sending: {json.dumps(request, indent=2)}")
    return request

def test_list_resources():
    """Test listing all available resources."""
    request = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "resources/list",
        "params": {}
    }
    print("\n=== Test: List Resources ===")
    send_jsonrpc(request)
    print("Expected: List of 60 Blockfrost API endpoints")

def test_read_epoch_latest():
    """Test reading the latest epoch resource."""
    request = {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "resources/read",
        "params": {
            "uri": "blockfrost://epochs/latest"
        }
    }
    print("\n=== Test: Read Latest Epoch ===")
    send_jsonrpc(request)
    print("Expected: JSON with epoch info (epoch number, start_time, end_time, etc.)")

def test_read_block_latest():
    """Test reading the latest block resource."""
    request = {
        "jsonrpc": "2.0",
        "id": 3,
        "method": "resources/read",
        "params": {
            "uri": "blockfrost://blocks/latest"
        }
    }
    print("\n=== Test: Read Latest Block ===")
    send_jsonrpc(request)
    print("Expected: JSON with block info (hash, slot, height, etc.)")

def test_read_pools_list():
    """Test reading the pools list resource."""
    request = {
        "jsonrpc": "2.0",
        "id": 4,
        "method": "resources/read",
        "params": {
            "uri": "blockfrost://pools"
        }
    }
    print("\n=== Test: Read Pools List ===")
    send_jsonrpc(request)
    print("Expected: JSON array of pool IDs")

def test_read_governance_dreps():
    """Test reading the DReps list resource."""
    request = {
        "jsonrpc": "2.0",
        "id": 5,
        "method": "resources/read",
        "params": {
            "uri": "blockfrost://governance/dreps"
        }
    }
    print("\n=== Test: Read DReps List ===")
    send_jsonrpc(request)
    print("Expected: JSON array of DRep IDs")

def print_sample_uris():
    """Print sample URIs that can be tested."""
    print("\n=== Sample MCP Resource URIs ===")
    sample_uris = [
        "blockfrost://epochs/latest",
        "blockfrost://epochs/507",
        "blockfrost://epochs/507/parameters",
        "blockfrost://blocks/latest",
        "blockfrost://blocks/12345678",
        "blockfrost://pools",
        "blockfrost://pools/extended",
        "blockfrost://governance/dreps",
        "blockfrost://governance/proposals",
        "blockfrost://assets",
        "blockfrost://accounts/{stake_address}",
        "blockfrost://addresses/{address}",
        "blockfrost://txs/{hash}",
    ]
    for uri in sample_uris:
        print(f"  {uri}")

def main():
    print("=" * 60)
    print("Acropolis MCP Server Test Script")
    print("=" * 60)
    print("\nThis script generates sample JSON-RPC requests for testing.")
    print("To actually test, you need to:")
    print("  1. Build the MCP server: cargo build -p acropolis_module_mcp_server")
    print("  2. Run Acropolis with mcp.enabled=true")
    print("  3. Use MCP Inspector: npx @anthropics/mcp-inspector")
    print("     OR send these JSON-RPC messages via stdio")
    
    test_list_resources()
    test_read_epoch_latest()
    test_read_block_latest()
    test_read_pools_list()
    test_read_governance_dreps()
    print_sample_uris()
    
    print("\n" + "=" * 60)
    print("For interactive testing, use:")
    print("  npx @anthropics/mcp-inspector")
    print("=" * 60)

if __name__ == "__main__":
    main()
