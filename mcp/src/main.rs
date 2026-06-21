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

struct ResolvedWorkspace {
    db_path: String,
    project_root: String,
    exists: bool,
}

/// Resolves the active CodeBroker workspace.
/// If ~/.codebroker/active_project has ever been set (e.g. via `set_workspace`
/// or `codebroker bind`), that workspace is always honored verbatim, even if
/// its database hasn't been indexed yet. We deliberately do NOT fall back to
/// a CWD-relative database in that case: silently serving a different
/// project's data is what caused duplicate-tree / stale-graph results when
/// switching workspaces. Only when no active_project pointer exists at all
/// do we fall back to a CWD-relative database (first-run behavior).
fn resolve_workspace() -> ResolvedWorkspace {
    if let Ok(home) = std::env::var("HOME") {
        let active_file = format!("{}/.codebroker/active_project", home);
        if let Ok(project_path) = std::fs::read_to_string(&active_file) {
            let project_path = project_path.trim().to_string();
            if !project_path.is_empty() {
                let db_path = format!("{}/.codebroker/codebroker.db", project_path);
                let exists = std::path::Path::new(&db_path).exists();
                if exists {
                    eprintln!("Using active project database: {}", db_path);
                } else {
                    eprintln!("Active workspace '{}' has no index yet at {}", project_path, db_path);
                }
                return ResolvedWorkspace { db_path, project_root: project_path, exists };
            }
        }
    }

    // No active_project pointer has ever been set: fall back to CWD.
    let cwd_db = ".codebroker/codebroker.db".to_string();
    let exists = std::path::Path::new(&cwd_db).exists();
    let project_root = std::env::current_dir().unwrap_or_default().to_string_lossy().to_string();
    ResolvedWorkspace { db_path: cwd_db, project_root, exists }
}

fn resolve_db_path() -> String {
    resolve_workspace().db_path
}

/// Runs `codebroker init` rooted at `project_dir` (via current_dir), rather than
/// inheriting the MCP server process's own CWD. Previously the auto-init hook
/// spawned the indexer without pinning its working directory, so it could index
/// whatever directory the MCP process happened to be launched from instead of
/// the workspace that `set_workspace` / `active_project` actually pointed to,
/// leaving the intended workspace's database empty (0 edges) or never indexed.
fn run_index(project_dir: &str) -> Result<String, String> {
    let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let parent = current_exe.parent().ok_or_else(|| "Could not resolve codebroker-mcp binary directory".to_string())?;
    let cli_path = parent.join("codebroker");

    let status = std::process::Command::new(&cli_path)
        .arg("init")
        .current_dir(project_dir)
        .status()
        .map_err(|e| format!("Failed to spawn indexer process in {}: {}", project_dir, e))?;

    if status.success() {
        Ok(format!("Indexing complete for workspace: {}", project_dir))
    } else {
        Err(format!("Indexer exited with non-zero status ({}) while indexing {}", status, project_dir))
    }
}

fn main() {
    // Resolve the active workspace (db path + the project root it belongs to)
    let resolved = resolve_workspace();

    // AUTO-INIT HOOK: If the resolved workspace has no index yet, index its
    // actual project root (not the MCP process's ambient CWD).
    if !resolved.exists {
        let project_dir = resolved.project_root.clone();
        let home_dir = std::env::var("HOME").unwrap_or_default();
        let project_path_buf = std::path::PathBuf::from(&project_dir);

        if project_dir.is_empty() || project_dir == home_dir || project_path_buf.parent().is_none() {
            eprintln!("Refusing to auto-initialize codebroker in home or root directory to prevent massive indexing.");
        } else {
            eprintln!("No index found for workspace '{}'. Auto-initializing...", project_dir);
            match run_index(&project_dir) {
                Ok(msg) => eprintln!("{}", msg),
                Err(e) => eprintln!("Auto-initialization failed: {}", e),
            }
        }
    }

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
                                    "tools": {}, // We will add tools in Phase 3
                                    "prompts": {}
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
                                        "description": "Returns a raw topological map of the local repository. Use this to understand the high-level architecture.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {}
                                        }
                                    },
                                    {
                                        "name": "project_overview_ai",
                                        "description": "Returns an AI-generated architectural summary of the entire repository. Use this to explain what the project does.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {}
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
                                        "description": "Read exact source code for one or more symbols without returning the entire file.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": { "type": "string", "description": "Single symbol name" },
                                                "symbols": { "type": "array", "items": { "type": "string" }, "description": "Array of symbol names (for batch fetching)" },
                                                "include_dependencies": { 
                                                    "type": "boolean",
                                                    "description": "If true, also includes directly related dependencies (e.g., inherited parent classes, prop interfaces)." 
                                                }
                                            }
                                        }
                                    },
                                    {
                                        "name": "set_workspace",
                                        "description": "Change the active CodeBroker indexing workspace. Triggers a database swap, and automatically indexes the new workspace if it hasn't been indexed yet.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "absolute_path": { "type": "string", "description": "Absolute path to the new workspace project root" }
                                            },
                                            "required": ["absolute_path"]
                                        }
                                    },
                                    {
                                        "name": "reindex_workspace",
                                        "description": "Force a full re-index of the active (or given) workspace, rebuilding the symbol table and dependency graph edges from scratch. Use this whenever repository_stats/project_overview reports 0 edges, or after making large changes to the codebase.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "absolute_path": { "type": "string", "description": "Optional. Absolute path to re-index. Defaults to the currently active workspace." }
                                            }
                                        }
                                    },
                                    {
                                        "name": "read_file_skeleton",
                                        "description": "Returns a skeletonized view of a file where sibling functions and classes are collapsed down to their signatures.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "file_path": {
                                                    "type": "string",
                                                    "description": "The path to the file to skeletonize."
                                                },
                                                "target_symbol": {
                                                    "type": "string",
                                                    "description": "Optional. The exact name of the symbol to keep fully expanded. All other sibling symbols will be collapsed."
                                                }
                                            },
                                            "required": ["file_path"]
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
                                        "name": "generate_patch",
                                        "description": "Generates a unified diff patch to modify a symbol, using CodeBroker's onboard AI and semantic context.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": { "type": "string", "description": "The exact name of the symbol to modify." },
                                                "instruction": { "type": "string", "description": "Instructions for what change to make." }
                                            },
                                            "required": ["symbol", "instruction"]
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
                                    },
                                    {
                                        "name": "subsystem_stats",
                                        "description": "Deterministic discovery of a subsystem (files, symbols, dependencies, entrypoints). Does not use AI.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "subsystem_name": { "type": "string" }
                                            },
                                            "required": ["subsystem_name"]
                                        }
                                    },
                                    {
                                        "name": "subsystem_overview",
                                        "description": "Generate an AI architectural explanation of a subsystem.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "subsystem_name": { "type": "string" }
                                            },
                                            "required": ["subsystem_name"]
                                        }
                                    },
                                    {
                                        "name": "explore_graph",
                                        "description": "Explore the code graph from a symbol using BFS traversal.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": { "type": "string" },
                                                "depth": { "type": "number", "description": "Traversal depth. Max 5." },
                                                "direction": { "type": "string", "description": "Incoming, Outgoing, or Both" },
                                                "max_nodes": { "type": "number", "description": "Cap on returned nodes (1-200). Default 100. Lower this for hotspot symbols with large fan-in/out to avoid oversized responses." }
                                            },
                                            "required": ["symbol", "depth", "direction"]
                                        }
                                    },
                                    {
                                        "name": "shortest_path",
                                        "description": "Find the shortest relationship path between two symbols in the repository graph.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "from": { "type": "string", "description": "The starting symbol" },
                                                "to": { "type": "string", "description": "The target symbol" }
                                            },
                                            "required": ["from", "to"]
                                        }
                                    },
                                    {
                                        "name": "architectural_hotspots",
                                        "description": "Identify the most critical and highly connected symbols in the repository.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "limit": { "type": "number", "description": "Max number of hotspots to return. Default 20." }
                                            }
                                        }
                                    },
                                    {
                                        "name": "dependency_cycles",
                                        "description": "Detect architectural circular dependencies within the repository graph.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "limit": { "type": "number", "description": "Max number of cycles to return in detail (1-500). Default 25. `cycles_found` in the response always reports the true total." }
                                            }
                                        }
                                    },
                                    {
                                        "name": "find_duplicate_logic",
                                        "description": "Scan the repository for near-duplicate function/component bodies that live in different files (copy-pasted logic). Unlike graph tools, this catches duplication even when there is no shared call/import edge between the copies.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "min_length": { "type": "number", "description": "Minimum normalized source length (characters) to consider. Default 80. Lower values surface more, noisier matches." }
                                            }
                                        }
                                    },
                                    {
                                        "name": "graph_subtree",
                                        "description": "Extract the undirected neighborhood dependency subtree originating from a root symbol.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "root_symbol": { "type": "string", "description": "The root symbol name" },
                                                "depth": { "type": "number", "description": "Traversal depth. Max 5." }
                                            },
                                            "required": ["root_symbol", "depth"]
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
                        let arguments = params.get("arguments").and_then(|a| a.as_object()).unwrap_or(default_args.as_object().unwrap());
                        
                        eprintln!("AI Agent requested tool execution: {}", tool_name);

                        // Re-resolve active workspace for each call
                        let db_path = resolve_db_path();
                        let mut estimated_raw_context_tokens = 0;
                        let mut cache_hit = false;
                        let start_time = std::time::Instant::now();
                        let model_used = "Claude Desktop";

                        let tool_result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            match tool_name {
                            "get_context" => {
                                let symbol = arguments.get("symbol").and_then(|s| s.as_str()).unwrap_or("");
                                match storage::Database::new(&db_path) {
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
                                    match storage::Database::new(&db_path) {
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
                                
                                let hf_token = std::env::var("HF_API_TOKEN").unwrap_or_default();
                                let mut llm_used = false;
                                let semantic_tokens = if !hf_token.is_empty() {
                                    use semantic::provider::LlmProvider;
                                    let provider = semantic::huggingface::HuggingFaceProvider::new(hf_token);
                                    match provider.expand_query(keyword, 5) {
                                        Ok((tokens, _)) => {
                                            llm_used = true;
                                            tokens
                                        },
                                        Err(e) => {
                                            eprintln!("Semantic expansion failed/skipped: {}", e);
                                            vec![]
                                        }
                                    }
                                } else {
                                    vec![]
                                };

                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_search_context(&db);
                                        match query::engine::search_symbols(&db, keyword, &semantic_tokens, llm_used) {
                                            Ok(results) => {
                                                let payload = serde_json::json!({
                                                    "workspace_root": db.project_root,
                                                    "results": results
                                                });
                                                serde_json::to_string_pretty(&payload).unwrap_or_default()
                                            },
                                            Err(e) => serde_json::json!({"success": false, "error": format!("Error searching: {}", e)}).to_string(),
                                        }
                                    }
                                    Err(_) => serde_json::json!({"success": false, "error": "Error connecting to db"}).to_string(),
                                }
                            }
                            "find_symbol" => {
                                let symbol = arguments.get("symbol").and_then(|s| s.as_str()).unwrap_or("");
                                let context_lines = arguments.get("context_lines").and_then(|n| n.as_u64()).unwrap_or(3) as usize;

                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_find_symbol_context(&db, symbol);
                                        match query::engine::find_symbol(&db, symbol, context_lines) {
                                            Ok(results) => {
                                                if results.is_empty() {
                                                    format!("Symbol '{}' not found in workspace '{}'.", symbol, db.project_root)
                                                } else {
                                                    let mut s = format!("Workspace: {}\nMatches for '{}':\n", db.project_root, symbol);
                                                    for (path, kind, line, preview, score) in results {
                                                        s.push_str(&format!("- [{}] at {}:{} (Score: {})\n```rust\n{}\n```\n\n", kind, path, line, score, preview));
                                                    }
                                                    s
                                                }
                                            }
                                            Err(e) => serde_json::json!({"success": false, "error": format!("Error finding symbol: {}", e)}).to_string(),
                                        }
                                    }
                                    Err(_) => serde_json::json!({"success": false, "error": "Error connecting to db"}).to_string(),
                                }
                            }
                            "project_overview" => {
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_graph_context(&db);
                                        match query::engine::build_project_overview(&db) {
                                            Ok(overview) => {
                                                let mut output = serde_json::to_value(&overview).unwrap();
                                                output["workspace_root"] = serde_json::Value::String(db.project_root.clone());
                                                serde_json::to_string_pretty(&output).unwrap_or_default()
                                            },
                                            Err(e) => format!("Error building overview: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "repository_stats" => {
                                match storage::Database::new(&db_path) {
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
                                    match storage::Database::new(&db_path) {
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
                                let include_deps = arguments.get("include_dependencies").and_then(|s| s.as_bool()).unwrap_or(false);
                                
                                let mut targets = Vec::new();
                                if let Some(s) = arguments.get("symbol").and_then(|s| s.as_str()) {
                                    targets.push(s.to_string());
                                }
                                if let Some(arr) = arguments.get("symbols").and_then(|a| a.as_array()) {
                                    for v in arr {
                                        if let Some(s) = v.as_str() {
                                            targets.push(s.to_string());
                                        }
                                    }
                                }

                                if targets.is_empty() {
                                    "Error: Must provide 'symbol' or 'symbols'".to_string()
                                } else {
                                    match storage::Database::new(&db_path) {
                                        Ok(db) => {
                                            let mut combined_results = Vec::new();
                                            let mut has_error = false;
                                            let mut err_msg = String::new();
                                            for symbol in targets {
                                                match query::retrieval::read_symbol_source(&db, &symbol, include_deps) {
                                                    Ok(results) => combined_results.extend(results),
                                                    Err(e) => {
                                                        has_error = true;
                                                        err_msg = format!("Error reading source for {}: {}", symbol, e);
                                                        break;
                                                    }
                                                }
                                            }
                                            if has_error {
                                                err_msg
                                            } else {
                                                serde_json::to_string_pretty(&combined_results).unwrap_or_default()
                                            }
                                        }
                                        Err(_) => "Error connecting to db".to_string(),
                                    }
                                }
                            }
                            "set_workspace" => {
                                let path = arguments.get("absolute_path").and_then(|s| s.as_str()).unwrap_or("");
                                if path.is_empty() {
                                    "Error: absolute_path is required".to_string()
                                } else if !std::path::Path::new(path).exists() {
                                    format!("Error: Path '{}' does not exist.", path)
                                } else {
                                    if let Ok(home) = std::env::var("HOME") {
                                        let active_file = format!("{}/.codebroker/active_project", home);
                                        std::fs::create_dir_all(format!("{}/.codebroker", home)).unwrap_or_default();
                                        if let Err(e) = std::fs::write(&active_file, path) {
                                            format!("Error saving workspace: {}", e)
                                        } else {
                                            let new_db_path = format!("{}/.codebroker/codebroker.db", path);
                                            if std::path::Path::new(&new_db_path).exists() {
                                                format!("Workspace successfully updated to {}. All subsequent tools will query this database.", path)
                                            } else {
                                                match run_index(path) {
                                                    Ok(_) => format!("Workspace successfully updated to {}. No existing index was found, so it was automatically indexed.", path),
                                                    Err(e) => format!("Workspace updated to {}, but automatic indexing failed: {}. Call reindex_workspace to retry.", path, e),
                                                }
                                            }
                                        }
                                    } else {
                                        "Error: HOME environment variable not set.".to_string()
                                    }
                                }
                            }
                            "reindex_workspace" => {
                                let target = arguments.get("absolute_path").and_then(|s| s.as_str());
                                let project_dir = match target {
                                    Some(p) if !p.is_empty() => p.to_string(),
                                    _ => resolve_workspace().project_root,
                                };
                                if !std::path::Path::new(&project_dir).exists() {
                                    format!("Error: Path '{}' does not exist.", project_dir)
                                } else {
                                    match run_index(&project_dir) {
                                        Ok(_) => {
                                            let reindexed_db_path = format!("{}/.codebroker/codebroker.db", project_dir);
                                            match storage::Database::new(&reindexed_db_path) {
                                                Ok(db) => match query::engine::build_project_overview(&db) {
                                                    Ok(overview) => format!(
                                                        "Re-indexed {}. Files: {}, Symbols: {}, Edges: {}.",
                                                        project_dir, overview.files, overview.symbols, overview.edges
                                                    ),
                                                    Err(e) => format!("Re-indexed {} but failed to read stats: {}", project_dir, e),
                                                },
                                                Err(e) => format!("Re-indexed {} but failed to open database afterwards: {}", project_dir, e),
                                            }
                                        }
                                        Err(e) => format!("Error re-indexing {}: {}", project_dir, e),
                                    }
                                }
                            }
                            "read_file_skeleton" => {
                                let path = arguments.get("file_path").and_then(|s| s.as_str()).unwrap_or("");
                                let target_symbol = arguments.get("target_symbol").and_then(|s| s.as_str());
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        match query::retrieval::skeletonize_file(&db, path, target_symbol) {
                                            Ok(res) => res,
                                            Err(e) => format!("Error reading file skeleton: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "explore_graph" => {
                                let symbol = arguments.get("symbol").and_then(|s| s.as_str()).unwrap_or("");
                                let depth = arguments.get("depth").and_then(|n| n.as_u64()).unwrap_or(2) as usize;
                                let direction_str = arguments.get("direction").and_then(|s| s.as_str()).unwrap_or("both");
                                let direction = query::graph::GraphDirection::from(direction_str);
                                let max_nodes = arguments.get("max_nodes").and_then(|n| n.as_u64()).unwrap_or(100) as usize;

                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_graph_context(&db);
                                        match query::graph::explore_graph(&db, symbol, depth, direction, max_nodes) {
                                            Ok(res) => serde_json::to_string_pretty(&res).unwrap_or_else(|_| "Error serializing graph".to_string()),
                                            Err(e) => format!("Error exploring graph: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "shortest_path" => {
                                let from_symbol = arguments.get("from").and_then(|s| s.as_str()).unwrap_or("");
                                let to_symbol = arguments.get("to").and_then(|s| s.as_str()).unwrap_or("");
                                
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        match query::graph::shortest_path(&db, from_symbol, to_symbol) {
                                            Ok(res) => {
                                                estimated_raw_context_tokens = res.nodes.len() * 50; // rough estimation
                                                serde_json::to_string_pretty(&res).unwrap_or_else(|_| "Error serializing path".to_string())
                                            },
                                            Err(e) => format!("Error finding shortest path: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "architectural_hotspots" => {
                                let limit = arguments.get("limit").and_then(|n| n.as_u64()).unwrap_or(20) as usize;
                                
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        match query::graph::architectural_hotspots(&db, limit) {
                                            Ok(res) => {
                                                estimated_raw_context_tokens = res.top_hotspots.len() * 50; // rough estimation
                                                serde_json::to_string_pretty(&res).unwrap_or_else(|_| "Error serializing hotspots".to_string())
                                            },
                                            Err(e) => format!("Error calculating architectural hotspots: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "dependency_cycles" => {
                                let limit = arguments.get("limit").and_then(|n| n.as_u64()).unwrap_or(25) as usize;
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        match query::graph::dependency_cycles(&db, limit) {
                                            Ok(res) => {
                                                estimated_raw_context_tokens = res.cycles.len() * 100; // rough estimation
                                                serde_json::to_string_pretty(&res).unwrap_or_else(|_| "Error serializing cycles".to_string())
                                            },
                                            Err(e) => format!("Error detecting dependency cycles: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "find_duplicate_logic" => {
                                let min_length = arguments.get("min_length").and_then(|n| n.as_u64()).unwrap_or(80) as usize;
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        match query::duplicates::find_duplicate_logic(&db, min_length) {
                                            Ok(res) => {
                                                estimated_raw_context_tokens = res.groups.len() * 60; // rough estimation
                                                serde_json::to_string_pretty(&res).unwrap_or_else(|_| "Error serializing duplicate logic report".to_string())
                                            }
                                            Err(e) => format!("Error scanning for duplicate logic: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "graph_subtree" => {
                                let root_symbol = arguments.get("root_symbol").and_then(|s| s.as_str()).unwrap_or("");
                                let depth = arguments.get("depth").and_then(|n| n.as_u64()).unwrap_or(3) as usize;
                                
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        match query::graph::graph_subtree(&db, root_symbol, depth) {
                                            Ok(res) => {
                                                estimated_raw_context_tokens = res.node_count * 50; // rough estimation
                                                serde_json::to_string_pretty(&res).unwrap_or_else(|_| "Error serializing graph subtree".to_string())
                                            },
                                    Err(e) => format!("Error exploring graph subtree: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "generate_patch" => {
                                let symbol = arguments.get("symbol").and_then(|s| s.as_str()).unwrap_or("");
                                let instruction = arguments.get("instruction").and_then(|s| s.as_str()).unwrap_or("");
                                let hf_token = std::env::var("HF_API_TOKEN").unwrap_or_default();
                                if hf_token.is_empty() {
                                    "Error: HF_API_TOKEN environment variable is not set.".to_string()
                                } else {
                                    match storage::Database::new(&db_path) {
                                        Ok(db) => {
                                            let provider = Box::new(semantic::huggingface::HuggingFaceProvider::new(hf_token));
                                            let patch_gen = semantic::generator::PatchGenerator::new(&db, provider);
                                            match patch_gen.generate_patch(symbol, instruction) {
                                                Ok(patch) => patch,
                                                Err(e) => format!("Error generating patch: {}", e),
                                            }
                                        }
                                        Err(_) => "Error connecting to db".to_string(),
                                    }
                                }
                            }
                            "read_file_snippet" => {
                                let path = arguments.get("path").and_then(|s| s.as_str()).unwrap_or("");
                                let start_line = arguments.get("start_line").and_then(|n| n.as_u64()).unwrap_or(1) as usize;
                                let end_line = arguments.get("end_line").and_then(|n| n.as_u64()).unwrap_or(1) as usize;
                                let resolved_path = match storage::Database::new(&db_path) {
                                    Ok(db) => db.resolve_path(path),
                                    Err(_) => path.to_string(),
                                };
                                match query::retrieval::read_file_snippet(&resolved_path, start_line, end_line) {
                                    Ok(res) => serde_json::to_string_pretty(&res).unwrap_or_default(),
                                    Err(e) => format!("Error reading file snippet: {}", e),
                                }
                            }
                            "get_implementation" => {
                                let symbol = arguments.get("symbol").and_then(|s| s.as_str()).unwrap_or("");
                                let include_deps = arguments.get("include_dependencies").and_then(|s| s.as_bool()).unwrap_or(false);
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        let source = query::retrieval::read_symbol_source(&db, symbol, false).unwrap_or_default();
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
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        let source = query::retrieval::read_symbol_source(&db, symbol, false).unwrap_or_default();
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
                            "subsystem_stats" => {
                                let name = arguments.get("subsystem_name").and_then(|s| s.as_str()).unwrap_or("");
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        match query::subsystem::discover_subsystem(&db, name) {
                                            Ok(stats) => serde_json::to_string_pretty(&stats).unwrap_or_default(),
                                            Err(e) => format!("Error discovering subsystem: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "subsystem_overview" => {
                                let name = arguments.get("subsystem_name").and_then(|s| s.as_str()).unwrap_or("");
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        let _ = db.init_schema();
                                        match query::subsystem::discover_subsystem(&db, name) {
                                            Ok(stats) => {
                                                let hf_token = std::env::var("HF_API_TOKEN").unwrap_or_default();
                                                if hf_token.is_empty() {
                                                    "Error: HF_API_TOKEN environment variable is not set.".to_string()
                                                } else {
                                                    let provider: Box<dyn semantic::provider::LlmProvider> = Box::new(semantic::huggingface::HuggingFaceProvider::new(hf_token));
                                                    let generator = semantic::subsystem::SubsystemOverviewGenerator::new(&provider, &db, "qwen2.5-coder".to_string());
                                                    match generator.generate_overview(&stats) {
                                                        Ok(overview) => {
                                                            cache_hit = false;
                                                            overview
                                                        }
                                                        Err(e) => format!("Error generating overview: {}", e)
                                                    }
                                                }
                                            }
                                            Err(e) => format!("Error discovering subsystem: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            _ => {
                                serde_json::json!({"success": false, "error": format!("Tool {} not recognized", tool_name)}).to_string()
                            }
                        }
                        })) {
                            Ok(res) => res,
                            Err(panic_err) => {
                                let msg = if let Some(s) = panic_err.downcast_ref::<&str>() { s.to_string() }
                                else if let Some(s) = panic_err.downcast_ref::<String>() { s.clone() }
                                else { "Unknown panic".to_string() };
                                serde_json::json!({ "success": false, "error": format!("Tool execution panicked: {}", msg) }).to_string()
                            }
                        };

                        // Backstop: regardless of per-tool pagination/limit params, never hand the
                        // client a payload so large it blows the MCP transport's token limit.
                        const MAX_TOOL_RESULT_CHARS: usize = 90_000;
                        let tool_result = if tool_result.len() > MAX_TOOL_RESULT_CHARS {
                            let mut truncated = tool_result.chars().take(MAX_TOOL_RESULT_CHARS).collect::<String>();
                            truncated.push_str(&format!(
                                "\n\n... [TRUNCATED: response was {} chars, exceeding the {} char safety limit. \
                                Re-run with a smaller `limit`/`max_nodes`/`depth` argument, or a more specific query, to get a complete result.]",
                                tool_result.len(), MAX_TOOL_RESULT_CHARS
                            ));
                            truncated
                        } else {
                            tool_result
                        };

                        let execution_time_ms = start_time.elapsed().as_millis() as usize;
                        let delivered_token_count = analytics::accounting::TokenAccounting::estimate_tokens(tool_result.len());
                        let source_lines_returned = tool_result.lines().count();

                        eprintln!("[Analytics] Tool: {}, Exec Time: {}ms, Lines: {}, Tokens: {}, Cache Hit: {}", 
                                   tool_name, execution_time_ms, source_lines_returned, delivered_token_count, cache_hit);

                        if let Ok(db) = storage::Database::new(&db_path) {
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
                "prompts/list" => {
                    if let Some(id) = request.id {
                        let response = JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
                            result: serde_json::json!({
                                "prompts": [
                                    {
                                        "name": "analyze_project",
                                        "description": "Get a comprehensive architectural overview of the local repository."
                                    },
                                    {
                                        "name": "explore_subsystem",
                                        "description": "Map out a specific subsystem or cluster in the codebase."
                                    }
                                ]
                            }),
                        };
                        println!("{}", serde_json::to_string(&response).unwrap());
                        stdout.flush().unwrap();
                    }
                }
                "prompts/get" => {
                    if let Some(id) = request.id {
                        let name = request.params.as_ref()
                            .and_then(|p| p.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("");
                            
                        let message_text = match name {
                            "analyze_project" => "Please use your CodeBroker tools (such as `project_overview` and `architectural_hotspots`) to analyze the local repository connected via MCP. Give me a high-level summary of what this codebase does and how the architecture is laid out.",
                            "explore_subsystem" => "Please use your CodeBroker `graph_subtree` and `subsystem_overview` tools to map out the core components of the local repository. Let me know what you find.",
                            _ => "Unknown prompt.",
                        };

                        let response = JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
                            result: serde_json::json!({
                                "description": "Automated CodeBroker Prompt",
                                "messages": [
                                    {
                                        "role": "user",
                                        "content": {
                                            "type": "text",
                                            "text": message_text
                                        }
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