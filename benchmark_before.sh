#!/bin/bash
cargo run --bin codebroker-mcp <<'INPUT' > before_context.txt
{"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "get_context", "arguments": {"symbol": "explore_graph"}}}
INPUT

cargo run --bin codebroker-mcp <<'INPUT' > before_search.txt
{"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "search_codebase", "arguments": {"query": "context"}}}
INPUT

cargo run --bin codebroker-mcp <<'INPUT' > before_graph.txt
{"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "explore_graph", "arguments": {"symbol": "search_symbols", "depth": 2}}}
INPUT
