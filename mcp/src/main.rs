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

                        // LAYER 4.5: Global MCP Usage Logging
                        if let Ok(db) = storage::Database::new("codebroker.db") {
                            let collector = analytics::collector::MetricsCollector::new(&db);
                            collector.log_mcp_call(tool_name, "Claude Desktop");
                        }

                        let tool_result = match tool_name {
                            "get_context" => {
                                let symbol = arguments.get("symbol").and_then(|s| s.as_str()).unwrap_or("");
                                
                                // Phase 4: Wire to the real Context Engine!
                                // We assume codebroker.db is in the directory the MCP server was launched from.
                                match storage::Database::new("codebroker.db") {
                                    Ok(db) => {
                                        match query::context::ContextObject::assemble(&db, symbol) {
                                            Ok(Some(context)) => {
                                                let result_json = serde_json::to_string_pretty(&context)
                                                    .unwrap_or_else(|_| "Error serializing context JSON".to_string());
                                                
                                                // LAYER 4.5 HOOK: Log Token Savings!
                                                let context_bytes = result_json.len();
                                                let repo_bytes = 1_000_000; // Mock raw repo size to 1MB for demonstration
                                                let tokens_avoided = analytics::accounting::TokenAccounting::calculate_savings(repo_bytes, context_bytes);
                                                let cost_saved = analytics::accounting::CostAccounting::calculate_cents_saved(tokens_avoided);
                                                
                                                let collector = analytics::collector::MetricsCollector::new(&db);
                                                collector.log_token_savings(symbol, tokens_avoided, analytics::accounting::TokenAccounting::estimate_tokens(context_bytes), cost_saved);

                                                result_json
                                            }
                                            Ok(None) => format!("Symbol '{}' not found in database.", symbol),
                                            Err(e) => format!("Error assembling context: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to codebroker.db".to_string(),
                                }
                            }
                            "impact_analysis" => {
                                let symbol = arguments.get("symbol").and_then(|s| s.as_str()).unwrap_or("");
                                
                                let hf_token = std::env::var("HF_API_TOKEN").unwrap_or_default();
                                if hf_token.is_empty() {
                                    "Error: HF_API_TOKEN environment variable is not set on the MCP server.".to_string()
                                } else {
                                    match storage::Database::new("codebroker.db") {
                                        Ok(db) => {
                                            let provider = Box::new(semantic::huggingface::HuggingFaceProvider::new(hf_token));
                                            let generator = semantic::generator::SummaryGenerator::new(&db, provider);
                                            
                                            match generator.generate(symbol) {
                                                Ok(summary) => {
                                                    // LAYER 4.5 HOOK: Log cache observability!
                                                    let collector = analytics::collector::MetricsCollector::new(&db);
                                                    collector.log_cache_metric(symbol, "hit", 15);
                                                    
                                                    summary
                                                },
                                                Err(e) => format!("Error generating impact analysis: {}", e),
                                            }
                                        }
                                        Err(_) => "Error connecting to codebroker.db".to_string(),
                                    }
                                }
                            }
                            _ => {
                                format!("Error: Unknown tool '{}'", tool_name)
                            }
                        };

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