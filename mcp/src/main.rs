use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Write};

#[derive(Serialize, Deserialize, Debug)]


// json rpc structures
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<serde_json::Value>,
    method: String,
    params: Option<serde_json::Value>,
}



#[derive(Serialize, Deserialize, Debug)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: serde_json::Value,
    result: serde_json::Value,
}



fn main() {
    // it will corrupt the protocol and crash the AI agent!
    eprintln!("CodeBroker MCP Server starting up...");
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    // The Infinite Transport Loop
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break, // Exit if stdin is closed (IDE shut down)
        };
        if line.trim().is_empty() {
            continue;
        }
        // Parse the incoming JSON-RPC request from the agent
        if let Ok(request) = serde_json::from_str::<JsonRpcRequest>(&line) {
            eprintln!("Received method: {}", request.method);
            // The Router
            match request.method.as_str() {
                "initialize" => {
                    // The agent is saying hello. We must respond with our capabilities.
                    if let Some(id) = request.id {
                        let response = JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
                            result: serde_json::json!({
                                "protocolVersion": "2024-11-05",
                                "capabilities": {
                                    "tools": {} // We will add tools in Phase 3
                                },
                                "serverInfo": {
                                    "name": "codebroker-mcp",
                                    "version": "0.1.0"
                                }
                            }),
                        };
                        
                        let response_str = serde_json::to_string(&response).unwrap();
                        println!("{}", response_str); // Write JSON to stdout!
                        stdout.flush().unwrap();
                    }
                }
                "initialized" => {
                    eprintln!("Agent initialization handshake complete.");
                }
                "tools/list" => {
                    // Tell the AI agent exactly what tools we have
                    if let Some(id) = request.id {
                        let response = JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
                            result: serde_json::json!({
                                "tools": [
                                    {
                                        "name": "get_context",
                                        "description": "Returns the exact architectural graph dependencies and blast radius dependents for a given code symbol.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": {
                                                    "type": "string",
                                                    "description": "The exact name of the struct, function, or trait."
                                                }
                                            },
                                            "required": ["symbol"]
                                        }
                                    },
                                    {
                                        "name": "impact_analysis",
                                        "description": "Returns a deep, AI-generated semantic architectural summary of a code symbol, utilizing Qwen2.5-Coder to explain the blast radius and context.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": {
                                                    "type": "string",
                                                    "description": "The exact name of the struct, function, or trait to analyze."
                                                }
                                            },
                                            "required": ["symbol"]
                                        }
                                    },
                                    {
                                        "name": "search_codebase",
                                        "description": "Discovery tool to find where a keyword or concept is mentioned in symbol names.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "keyword": {
                                                    "type": "string",
                                                    "description": "The concept or name to search for (e.g. 'AuthService')."
                                                }
                                            },
                                            "required": ["keyword"]
                                        }
                                    },
                                    {
                                        "name": "find_symbol",
                                        "description": "Exact lookup tool to find the definition file, line number, and kind of a specific symbol.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": {
                                                    "type": "string",
                                                    "description": "The exact name of the symbol."
                                                },
                                                "context_lines": {
                                                    "type": "number",
                                                    "description": "Optional. Number of lines of context to fetch above and below the symbol. Defaults to 3."
                                                }
                                            },
                                            "required": ["symbol"]
                                        }
                                    },
                                    {
                                        "name": "project_overview",
                                        "description": "Returns a raw topological map of the repository, including file counts, symbol counts, and subsystem directories.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {},
                                            "required": []
                                        }
                                    },
                                    {
                                        "name": "project_overview_ai",
                                        "description": "Returns a deeply cached, AI-generated architectural summary of the entire repository.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {},
                                            "required": []
                                        }
                                    },
                                    {
                                        "name": "repository_stats",
                                        "description": "Returns raw JSON counts of files, symbols, edges, and languages in the repository.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {},
                                            "required": []
                                        }
                                    },
                                    {
                                        "name": "read_symbol_source",
                                        "description": "Read exact source code for a symbol without returning the entire file.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": { "type": "string" }
                                            },
                                            "required": ["symbol"]
                                        }
                                    },
                                    {
                                        "name": "read_file_snippet",
                                        "description": "Read a specific line range from a file.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "path": { "type": "string" },
                                                "start_line": { "type": "number" },
                                                "end_line": { "type": "number" }
                                            },
                                            "required": ["path", "start_line", "end_line"]
                                        }
                                    },
                                    {
                                        "name": "get_implementation",
                                        "description": "Return everything necessary to understand how a symbol is implemented.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": { "type": "string" }
                                            },
                                            "required": ["symbol"]
                                        }
                                    },
                                    {
                                        "name": "get_edit_context",
                                        "description": "Prepare future code-editing workflows with target implementation and context.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": { "type": "string" }
                                            },
                                            "required": ["symbol"]
                                        }
                                    }
                                ]
                            }),
                        };
                        println!("{}", serde_json::to_string(&response).unwrap());
                        stdout.flush().unwrap();
                    }
                }
                "tools/call" => {
                    // Intercept the AI agent trying to execute a tool
                    if let Some(id) = request.id {
                        let params = request.params.unwrap_or_default();
                        let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        let default_args = serde_json::json!({});
                        let arguments = params.get("arguments").unwrap_or(&default_args);
                        
                        eprintln!("AI Agent requested tool execution: {}", tool_name);

                        let start_time = std::time::Instant::now();
                        let mut cache_hit = false;
                        let mut estimated_raw_context_tokens = 0;
                        let model_used = "Claude Desktop";

                        let tool_result = match tool_name {
                            "get_context" => {
                                let symbol = arguments.get("symbol").and_then(|s| s.as_str()).unwrap_or("");
                                match storage::Database::new("codebroker.db") {
                                    Ok(db) => {
                                        estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_graph_context(&db);
                                        match query::context::ContextObject::assemble(&db, symbol) {
                                            Ok(Some(context)) => {
                                                serde_json::to_string_pretty(&context).unwrap_or_else(|_| "Error serializing context JSON".to_string())
                                            }
                                            Ok(None) => format!("Symbol '{}' not found in database.", symbol),
                                            Err(e) => format!("Error assembling context: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "impact_analysis" => {
                                let symbol = arguments.get("symbol").and_then(|s| s.as_str()).unwrap_or("");
                                let hf_token = std::env::var("HF_API_TOKEN").unwrap_or_default();
                                if hf_token.is_empty() {
                                    "Error: HF_API_TOKEN environment variable is not set.".to_string()
                                } else {
                                    match storage::Database::new("codebroker.db") {
                                        Ok(db) => {
                                            estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_graph_context(&db);
                                            let provider = Box::new(semantic::huggingface::HuggingFaceProvider::new(hf_token));
                                            let generator = semantic::generator::SummaryGenerator::new(&db, provider);
                                            match generator.generate(symbol) {
                                                Ok((summary, hit)) => {
                                                    cache_hit = hit;
                                                    summary
                                                },
                                                Err(e) => format!("Error generating impact analysis: {}", e),
                                            }
                                        }
                                        Err(_) => "Error connecting to db".to_string(),
                                    }
                                }
                            }
                            "search_codebase" => {
                                let keyword = arguments.get("keyword").and_then(|s| s.as_str()).unwrap_or("");
                                
                                // Perform Semantic Expansion if token is present
                                let hf_token = std::env::var("HF_API_TOKEN").unwrap_or_default();
                                let semantic_tokens = if !hf_token.is_empty() {
                                    use semantic::provider::LlmProvider;
                                    let provider = semantic::huggingface::HuggingFaceProvider::new(hf_token);
                                    provider.expand_query(keyword).map(|(tokens, _)| tokens).unwrap_or_default()
                                } else {
                                    vec![]
                                };

                                match storage::Database::new("codebroker.db") {
                                    Ok(db) => {
                                        estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_search_context(&db);
                                        match query::engine::search_symbols(&db, keyword, &semantic_tokens) {
                                            Ok(results) => serde_json::to_string_pretty(&results).unwrap_or_default(),
                                            Err(e) => format!("Error searching: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "find_symbol" => {
                                let symbol = arguments.get("symbol").and_then(|s| s.as_str()).unwrap_or("");
                                let context_lines = arguments.get("context_lines").and_then(|n| n.as_u64()).unwrap_or(3) as usize;
                                
                                match storage::Database::new("codebroker.db") {
                                    Ok(db) => {
                                        estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_find_symbol_context(&db, symbol);
                                        match query::engine::find_symbol_exact(&db, symbol, context_lines) {
                                            Ok(results) => {
                                                if results.is_empty() {
                                                    format!("Symbol '{}' not found.", symbol)
                                                } else {
                                                    let mut s = format!("Exact matches for '{}':\n", symbol);
                                                    for (path, kind, line, preview) in results {
                                                        s.push_str(&format!("- [{}] at {}:{}\n```rust\n{}\n```\n\n", kind, path, line, preview));
                                                    }
                                                    s
                                                }
                                            }
                                            Err(e) => format!("Error finding symbol: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "project_overview" => {
                                match storage::Database::new("codebroker.db") {
                                    Ok(db) => {
                                        estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_graph_context(&db);
                                        match query::engine::build_project_overview(&db) {
                                            Ok(overview) => serde_json::to_string_pretty(&overview).unwrap_or_default(),
                                            Err(e) => format!("Error building overview: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "repository_stats" => {
                                match storage::Database::new("codebroker.db") {
                                    Ok(db) => {
                                        estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_graph_context(&db);
                                        match query::engine::build_project_overview(&db) {
                                            Ok(overview) => {
                                                let stats = serde_json::json!({
                                                    "files": overview.files,
                                                    "symbols": overview.symbols,
                                                    "edges": overview.edges,
                                                    "languages": overview.languages
                                                });
                                                serde_json::to_string_pretty(&stats).unwrap_or_default()
                                            }
                                            Err(e) => format!("Error fetching stats: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "project_overview_ai" => {
                                let hf_token = std::env::var("HF_API_TOKEN").unwrap_or_default();
                                if hf_token.is_empty() {
                                    "Error: HF_API_TOKEN environment variable is not set.".to_string()
                                } else {
                                    match storage::Database::new("codebroker.db") {
                                        Ok(db) => {
                                            estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_graph_context(&db);
                                            let provider = Box::new(semantic::huggingface::HuggingFaceProvider::new(hf_token));
                                            let generator = semantic::generator::ProjectOverviewGenerator::new(&db, provider);
                                            match generator.generate() {
                                                Ok((summary, hit)) => {
                                                    cache_hit = hit;
                                                    summary
                                                },
                                                Err(e) => format!("Error generating overview: {}", e),
                                            }
                                        }
                                        Err(_) => "Error connecting to db".to_string(),
                                    }
                                }
                            }
                            "read_symbol_source" => {
                                let symbol = arguments.get("symbol").and_then(|s| s.as_str()).unwrap_or("");
                                match storage::Database::new("codebroker.db") {
                                    Ok(db) => {
                                        match query::retrieval::read_symbol_source(&db, symbol) {
                                            Ok(results) => serde_json::to_string_pretty(&results).unwrap_or_default(),
                                            Err(e) => format!("Error reading source: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "read_file_snippet" => {
                                let path = arguments.get("path").and_then(|s| s.as_str()).unwrap_or("");
                                let start_line = arguments.get("start_line").and_then(|n| n.as_u64()).unwrap_or(1) as usize;
                                let end_line = arguments.get("end_line").and_then(|n| n.as_u64()).unwrap_or(1) as usize;
                                match query::retrieval::read_file_snippet(path, start_line, end_line) {
                                    Ok(res) => serde_json::to_string_pretty(&res).unwrap_or_default(),
                                    Err(e) => format!("Error reading file snippet: {}", e),
                                }
                            }
                            "get_implementation" => {
                                let symbol = arguments.get("symbol").and_then(|s| s.as_str()).unwrap_or("");
                                match storage::Database::new("codebroker.db") {
                                    Ok(db) => {
                                        let source = query::retrieval::read_symbol_source(&db, symbol).unwrap_or_default();
                                        let context = query::context::ContextObject::assemble(&db, symbol).unwrap_or_default();
                                        let implementation = serde_json::json!({
                                            "symbol_source": source,
                                            "context": context
                                        });
                                        serde_json::to_string_pretty(&implementation).unwrap_or_default()
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "get_edit_context" => {
                                let symbol = arguments.get("symbol").and_then(|s| s.as_str()).unwrap_or("");
                                match storage::Database::new("codebroker.db") {
                                    Ok(db) => {
                                        let source = query::retrieval::read_symbol_source(&db, symbol).unwrap_or_default();
                                        let context = query::context::ContextObject::assemble(&db, symbol).unwrap_or_default();
                                        let edit_context = serde_json::json!({
                                            "target_implementation": source,
                                            "forward_dependencies": context.as_ref().map(|c| c.forward_dependencies.clone()).unwrap_or_default(),
                                            "reverse_dependencies": context.as_ref().map(|c| c.reverse_dependencies.clone()).unwrap_or_default(),
                                            "suggested_edit_boundaries": "Use start_line and end_line from target_implementation"
                                        });
                                        serde_json::to_string_pretty(&edit_context).unwrap_or_default()
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            _ => {
                                format!("Error: Unknown tool '{}'", tool_name)
                            }
                        };

                        let execution_time_ms = start_time.elapsed().as_millis() as usize;
                        let delivered_token_count = analytics::accounting::TokenAccounting::estimate_tokens(tool_result.len());
                        let source_lines_returned = tool_result.lines().count();

                        eprintln!("[Analytics] Tool: {}, Exec Time: {}ms, Lines: {}, Tokens: {}, Cache Hit: {}", 
                                   tool_name, execution_time_ms, source_lines_returned, delivered_token_count, cache_hit);

                        if let Ok(db) = storage::Database::new("codebroker.db") {
                            let collector = analytics::collector::MetricsCollector::new(&db);
                            collector.log_comprehensive_event(
                                tool_name,
                                execution_time_ms,
                                delivered_token_count,
                                estimated_raw_context_tokens,
                                cache_hit,
                                model_used
                            );
                        }

                        // Send the result back to the AI agent
                        let response = JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
                            result: serde_json::json!({
                                "content": [
                                    {
                                        "type": "text",
                                        "text": tool_result
                                    }
                                ]
                            }),
                        };
                        println!("{}", serde_json::to_string(&response).unwrap());
                        stdout.flush().unwrap();
                    }
                }
                _ => {
                    eprintln!("Unknown method received: {}", request.method);
                }
            }
        }
    }
}