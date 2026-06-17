with open("mcp/src/main.rs", "r") as f:
    lines = f.readlines()

new_block = """                        let start_time = std::time::Instant::now();
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
                                match storage::Database::new("codebroker.db") {
                                    Ok(db) => {
                                        estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_search_context(&db);
                                        match query::engine::search_symbols(&db, keyword) {
                                            Ok(results) => serde_json::to_string_pretty(&results).unwrap_or_default(),
                                            Err(e) => format!("Error searching: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "find_symbol" => {
                                let symbol = arguments.get("symbol").and_then(|s| s.as_str()).unwrap_or("");
                                match storage::Database::new("codebroker.db") {
                                    Ok(db) => {
                                        estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_find_symbol_context(&db, symbol);
                                        match query::engine::find_symbol_exact(&db, symbol) {
                                            Ok(results) => {
                                                if results.is_empty() {
                                                    format!("Symbol '{}' not found.", symbol)
                                                } else {
                                                    let mut s = format!("Exact matches for '{}':\\n", symbol);
                                                    for (path, kind, line) in results {
                                                        s.push_str(&format!("- [{}] at {}:{}\\n", kind, path, line));
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
                            _ => {
                                format!("Error: Unknown tool '{}'", tool_name)
                            }
                        };

                        let execution_time_ms = start_time.elapsed().as_millis() as usize;
                        let delivered_token_count = analytics::accounting::TokenAccounting::estimate_tokens(tool_result.len());

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
                        }"""

with open("mcp/src/main.rs", "w") as f:
    f.writelines(lines[:178])
    f.write(new_block)
    f.write("\n")
    f.writelines(lines[328:])
