#!/usr/bin/env python3
"""
Standalone MCP Server Test

Tests the Acropolis MCP server using the HTTP+SSE transport protocol.
This is independent of VS Code and validates the full MCP handshake.
"""

import json
import sys
import requests
from typing import Optional

MCP_URL = "http://127.0.0.1:4341/mcp"
HEADERS = {
    "Content-Type": "application/json",
    "Accept": "application/json, text/event-stream"
}


def parse_sse_response(response: requests.Response) -> Optional[dict]:
    """Parse SSE response and extract JSON data."""
    for line in response.iter_lines():
        if line:
            decoded = line.decode('utf-8')
            if decoded.startswith('data: '):
                return json.loads(decoded[6:])
    return None


def test_mcp_server():
    """Test the MCP server with proper protocol flow."""
    print("=" * 60)
    print("Acropolis MCP Server Standalone Test")
    print("=" * 60)
    
    session = requests.Session()
    
    # Step 1: Initialize
    print("\n[1] Sending initialize request...")
    init_request = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "standalone-test", "version": "1.0"}
        }
    }
    
    try:
        resp = session.post(MCP_URL, json=init_request, headers=HEADERS, stream=True, timeout=10)
        resp.raise_for_status()
    except requests.exceptions.ConnectionError:
        print("ERROR: Cannot connect to MCP server at", MCP_URL)
        print("Make sure the Acropolis server is running.")
        return False
    except Exception as e:
        print(f"ERROR: {e}")
        return False
    
    # Extract session ID from headers
    session_id = resp.headers.get('mcp-session-id')
    print(f"    Session ID: {session_id}")
    
    # Parse initialize response
    init_response = parse_sse_response(resp)
    if not init_response:
        print("ERROR: No response from initialize")
        return False
    
    if 'error' in init_response:
        print(f"ERROR: {init_response['error']}")
        return False
    
    result = init_response.get('result', {})
    server_info = result.get('serverInfo', {})
    capabilities = result.get('capabilities', {})
    
    print(f"    Server: {server_info.get('name')} v{server_info.get('version')}")
    print(f"    Protocol: {result.get('protocolVersion')}")
    print(f"    Capabilities: {list(capabilities.keys())}")
    
    # Check if tools capability is present
    if 'tools' not in capabilities:
        print("WARNING: Server does not advertise tools capability")
    
    # Step 2: Send initialized notification
    print("\n[2] Sending initialized notification...")
    if session_id:
        HEADERS['mcp-session-id'] = session_id
    
    init_notif = {
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    }
    
    try:
        resp2 = session.post(MCP_URL, json=init_notif, headers=HEADERS, stream=True, timeout=10)
        # Notification may not return data, that's OK
        print("    OK (notification sent)")
    except Exception as e:
        print(f"    Warning: {e}")
    
    # Step 3: List tools
    print("\n[3] Requesting tools/list...")
    tools_request = {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    }
    
    try:
        resp3 = session.post(MCP_URL, json=tools_request, headers=HEADERS, stream=True, timeout=10)
        tools_response = parse_sse_response(resp3)
        
        if not tools_response:
            print("ERROR: No response from tools/list")
            return False
        
        if 'error' in tools_response:
            print(f"ERROR: {tools_response['error']}")
            return False
        
        tools = tools_response.get('result', {}).get('tools', [])
        print(f"    Found {len(tools)} tools:")
        
        for i, tool in enumerate(tools[:10]):  # Show first 10
            print(f"      - {tool.get('name')}: {tool.get('description', '')[:50]}...")
        
        if len(tools) > 10:
            print(f"      ... and {len(tools) - 10} more")
            
    except Exception as e:
        print(f"ERROR listing tools: {e}")
        return False
    
    # Step 4: List resources
    print("\n[4] Requesting resources/list...")
    resources_request = {
        "jsonrpc": "2.0",
        "id": 3,
        "method": "resources/list",
        "params": {}
    }
    
    try:
        resp4 = session.post(MCP_URL, json=resources_request, headers=HEADERS, stream=True, timeout=10)
        resources_response = parse_sse_response(resp4)
        
        if resources_response and 'result' in resources_response:
            resources = resources_response.get('result', {}).get('resources', [])
            print(f"    Found {len(resources)} resources")
        elif resources_response and 'error' in resources_response:
            print(f"    Error: {resources_response['error']}")
        else:
            print("    No response")
            
    except Exception as e:
        print(f"    Error: {e}")
    
    # Step 5: Call a tool (if tools exist)
    if tools:
        print("\n[5] Testing tool call (get_epoch_info or similar)...")
        
        # Find a simple tool to test
        test_tool = None
        for tool in tools:
            name = tool.get('name', '')
            # Look for tools that don't require parameters
            if 'latest' in name.lower() or name == 'get_epoch_info':
                test_tool = tool
                break
        
        if not test_tool:
            # Just use the first tool
            test_tool = tools[0]
        
        print(f"    Calling tool: {test_tool.get('name')}")
        
        # Build arguments based on schema
        args = {}
        schema = test_tool.get('inputSchema', {})
        required = schema.get('required', [])
        
        # Provide dummy values for required params
        for param in required:
            if 'epoch' in param.lower() or 'number' in param.lower():
                args[param] = "1"  # Use epoch 1 as test
            elif 'hash' in param.lower():
                args[param] = "latest"
            else:
                args[param] = "test"
        
        call_request = {
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": test_tool.get('name'),
                "arguments": args
            }
        }
        
        try:
            resp5 = session.post(MCP_URL, json=call_request, headers=HEADERS, stream=True, timeout=30)
            call_response = parse_sse_response(resp5)
            
            if call_response and 'result' in call_response:
                content = call_response['result'].get('content', [])
                print(f"    Result: Got {len(content)} content item(s)")
                if content:
                    first = content[0]
                    if first.get('type') == 'text':
                        text = first.get('text', '')[:200]
                        print(f"    Preview: {text}...")
            elif call_response and 'error' in call_response:
                print(f"    Error: {call_response['error'].get('message', call_response['error'])}")
            else:
                print("    No response")
                
        except Exception as e:
            print(f"    Error: {e}")
    
    # Summary
    print("\n" + "=" * 60)
    print("TEST SUMMARY")
    print("=" * 60)
    print(f"✓ Server connected: {server_info.get('name')}")
    print(f"✓ Protocol version: {result.get('protocolVersion')}")
    print(f"✓ Tools available: {len(tools)}")
    print(f"✓ Session management: {'Working' if session_id else 'Not using sessions'}")
    
    if len(tools) == 0:
        print("\n⚠ WARNING: No tools found! Check ROUTES registration.")
        return False
    
    print("\n✓ MCP Server is working correctly!")
    return True


if __name__ == "__main__":
    success = test_mcp_server()
    sys.exit(0 if success else 1)
