use clap::{Parser, Subcommand};
use parser::frontend;
use std::fs;
use rusqlite::params;

// 1. Define the CLI arguments
#[derive(Parser)]
#[command(name = "codebroker")]
#[command(about = "A blazing fast local code graph", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}


#[derive(Subcommand)]
enum Commands {
    /// Initializes the database and indexes the codebase
    Init,
    /// Queries the graph for a specific symbol
    Query {text: String},
    Dependents {symbol: String},
    Context { symbol: String },
    Explain { symbol: String },
    Knowledge,
    Refresh,
    Metrics,
    Analytics,
    Dashboard,
    /// Instantly hooks up Claude Desktop and Antigravity to the current directory
    Bind
}


fn main() {
    let cli = Cli::parse();
    match &cli.command {
        Commands::Init => {
            println!("Initializing CodeBroker...");

            let _ = fs::remove_file(".codebroker/codebroker.db");
            let _ = fs::create_dir_all(".codebroker");

            
            // 1. Boot up the database
            let db = storage::Database::new(".codebroker/codebroker.db").expect("Failed to create DB");
            db.init_schema().expect("Failed to initialize schema");
            use parser::frontend::{LanguageFrontend, RustFrontend};
            use parser::typescript_frontend::{TypeScriptFrontend, TsxFrontend};
            use parser::python_frontend::PythonFrontend;
            use parser::javascript_frontend::JavaScriptFrontend;
            use parser::config_frontend::ConfigFrontend;
            use parser::vue_frontend::VueFrontend;
            use parser::svelte_frontend::SvelteFrontend;
             let frontends: Vec<Box<dyn LanguageFrontend>> = vec![
                Box::new(RustFrontend),
                Box::new(TypeScriptFrontend),
                Box::new(TsxFrontend),
                Box::new(PythonFrontend),
                Box::new(JavaScriptFrontend),
                Box::new(ConfigFrontend),
                Box::new(VueFrontend),
                Box::new(SvelteFrontend),
            ];

            // 1.5 Load Aliases
            let mut alias_map: Vec<(String, String)> = Vec::new();
            
            // A. Try tsconfig.json / jsconfig.json
            if let Ok(config_str) = fs::read_to_string("tsconfig.json").or_else(|_| fs::read_to_string("jsconfig.json")) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&config_str) {
                    if let Some(paths) = json.get("compilerOptions").and_then(|c| c.get("paths")).and_then(|p| p.as_object()) {
                        for (k, v) in paths {
                            if let Some(v_arr) = v.as_array() {
                                if let Some(first) = v_arr.first().and_then(|s| s.as_str()) {
                                    let key_clean = k.replace("/*", "");
                                    let val_clean = first.replace("/*", "");
                                    alias_map.push((key_clean, val_clean));
                                }
                            }
                        }
                    }
                }
            }

            // B. Try vite.config.ts / vite.config.js
            if let Ok(config_str) = fs::read_to_string("vite.config.ts").or_else(|_| fs::read_to_string("vite.config.js")) {
                if let Ok(re) = regex::Regex::new(r#"['"]?([^'"]+)['"]?\s*:\s*(?:fileURLToPath\(new URL\(['"]([^'"]+)['"]|path\.resolve\(__dirname,\s*['"]([^'"]+)['"]|['"]([^'"]+)['"])"#) {
                    for cap in re.captures_iter(&config_str) {
                        let key = cap.get(1).map_or("", |m| m.as_str()).to_string();
                        let val = cap.get(2)
                            .or_else(|| cap.get(3))
                            .or_else(|| cap.get(4))
                            .map_or("", |m| m.as_str())
                            .to_string();
                        if !key.is_empty() && !val.is_empty() {
                            let val_clean = val.replace("./", "");
                            alias_map.push((key, val_clean));
                        }
                    }
                }
            }

            if alias_map.is_empty() {
                alias_map.push(("@/".to_string(), "src/".to_string()));
            }

            // 2. Walk the file system
            let files = indexer::walker::collect_files(".");
            println!("Found {} files to index.", files.len());

            // 3. The Main Indexing Loop
            for file_path in files {
                if let Ok(source_code) = fs::read_to_string(&file_path) {
                    
                    // A. Extract the file extension
                    let extension = std::path::Path::new(&file_path)
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .unwrap_or("");

                    // B. Find the correct language parser from our registry
                    let mut matched_frontend = None;
                    for frontend in &frontends {
                        if frontend.can_handle(&file_path) {
                            matched_frontend = Some(frontend);
                            break;
                        }
                    }

                    // C. If we have a parser for this language, process it!
                    if let Some(frontend) = matched_frontend {
                        
                        let file_id = db.insert_file(&file_path).unwrap();

                        // D. The Universal Extraction (Zero language-specific code here!)
                        if let Some((metadata, symbols, imports)) = frontend.parse_and_extract(&source_code, &file_path) {
                            
                            let mut route_path = None;
                            let mut route_segment = None;
                            
                            // Universal route discovery
                            let route_prefixes = [("app/", "/"), ("src/routes/", "/"), ("pages/", "/")];
                            for (prefix, _route_base) in route_prefixes.iter() {
                                if file_path.contains(prefix) {
                                    let parts: Vec<&str> = file_path.split(prefix).collect();
                                    if parts.len() > 1 {
                                        let route_parts: Vec<&str> = parts[1].split('/').collect();
                                        if !route_parts.is_empty() {
                                            let file_name = route_parts.last().unwrap();
                                            route_segment = Some(file_name.split('.').next().unwrap_or(file_name).to_string());
                                            
                                            // Handle Remix dot notation e.g. dashboard.users.tsx
                                            let dir_path = if file_name.contains('.') && !file_name.starts_with('+') && *prefix == "app/" {
                                                file_name.split('.').collect::<Vec<&str>>()[..file_name.split('.').count()-1].join("/")
                                            } else {
                                                route_parts[..route_parts.len()-1].join("/")
                                            };
                                            
                                            route_path = Some(format!("/{}", dir_path));
                                            break;
                                        }
                                    }
                                }
                            }

                            db.update_file_metadata(file_id, metadata.directive.as_deref(), route_path.as_deref(), route_segment.as_deref()).unwrap();

                            for symbol in symbols {
                                db.insert_symbol(file_id, &symbol).unwrap();
                            }
                            
                            for import in imports {
                                db.insert_raw_import(file_id, &import).unwrap();
                            }
                        }
                    }
                }
            }

            // --- PASS 2: THE LINKER ---
            println!("Pass 1 complete. Starting Pass 2: Linking graph edges...");

            // 1. Get all the "Missing Friends" from our staging table
            let raw_imports = db.get_all_raw_imports().expect("Failed to fetch raw imports");
            let mut edges_created = 0;

            // 2. Loop through every single staged import
            for (_raw_id, source_file_id, import_name, import_source, import_kind) in raw_imports {
                let edge_kind = import_kind.unwrap_or_else(|| "imports".to_string());
                // Determine if we have a source path we can resolve via aliases
                let mut resolved_source = import_source.clone();
                if let Some(src) = &import_source {
                    for (alias, path_prefix) in &alias_map {
                        if src.starts_with(alias) {
                            resolved_source = Some(src.replace(alias, path_prefix));
                            break;
                        }
                    }

                    if resolved_source.is_none() && src.contains('.') && !src.contains('/') {
                        let py_path = src.replace(".", "/");
                        resolved_source = Some(format!("{}.py", py_path));
                    }
                }

                // If we resolved a path, let's try to link exactly to that file's export
                if let Some(src) = resolved_source {
                    // Very rudimentary resolution: find a file containing the path
                    let mut file_stmt = db.conn.prepare("SELECT id FROM files WHERE path LIKE ?1 LIMIT 1").unwrap();
                    let search_path = format!("%{}%", src);
                    if let Ok(target_file_id) = file_stmt.query_row(params![search_path], |row| row.get::<_, i64>(0)) {
                        // find a symbol in that file that matches the name
                        let mut sym_stmt = db.conn.prepare("SELECT id FROM symbols WHERE file_id = ?1 AND LOWER(name) = LOWER(?2) LIMIT 1").unwrap();
                        if let Ok(target_symbol_id) = sym_stmt.query_row(params![target_file_id, import_name], |row| row.get::<_, i64>(0)) {
                            let _ = db.insert_edge(source_file_id, target_symbol_id, &edge_kind);
                            edges_created += 1;
                            continue;
                        }
                    }
                }

                // Fallback to global symbol resolution
                let words: Vec<&str> = import_name.split(|c: char| !c.is_alphanumeric()).collect();
                for word in words {
                    if word.is_empty() { continue; }
                    
                    if let Ok(Some(target_symbol_id)) = db.find_symbol_id_by_name(word) {
                        let _ = db.insert_edge(source_file_id, target_symbol_id, &edge_kind);
                        edges_created += 1;
                    }
                }
            }

            // 3. Link Prop Types
            let mut prop_stmt = db.conn.prepare("SELECT file_id, prop_type FROM symbols WHERE prop_type IS NOT NULL").unwrap();
            let mut prop_rows = prop_stmt.query([]).unwrap();
            while let Some(row) = prop_rows.next().unwrap_or(None) {
                let file_id: i64 = row.get(0).unwrap();
                let prop_type: String = row.get(1).unwrap();
                if let Ok(Some(target_symbol_id)) = db.find_symbol_id_by_name(&prop_type) {
                    let _ = db.insert_edge(file_id, target_symbol_id, "accepts_props");
                    edges_created += 1;
                }
            }

            // 4. Link Layouts to Pages (wraps_route)
            let mut layout_stmt = db.conn.prepare(
                "SELECT id, path FROM files WHERE path LIKE '%layout.%' OR path LIKE '%+layout.%'"
            ).unwrap();
            let mut layout_rows = layout_stmt.query([]).unwrap();
            while let Some(row) = layout_rows.next().unwrap_or(None) {
                let layout_file_id: i64 = row.get(0).unwrap();
                let layout_path: String = row.get(1).unwrap();
                
                if let Some(dir_end) = layout_path.rfind('/') {
                    let dir_prefix = &layout_path[..dir_end + 1];
                    let search_pattern = format!("{}%", dir_prefix);
                    
                    let mut page_stmt = db.conn.prepare(
                        "SELECT symbols.id FROM symbols JOIN files ON symbols.file_id = files.id 
                         WHERE files.path LIKE ?1 AND symbols.kind = 'page'"
                    ).unwrap();
                    let mut page_rows = page_stmt.query(params![search_pattern]).unwrap();
                    while let Some(page_row) = page_rows.next().unwrap_or(None) {
                        let page_symbol_id: i64 = page_row.get(0).unwrap();
                        let _ = db.insert_edge(layout_file_id, page_symbol_id, "wraps_route");
                        edges_created += 1;
                    }
                }
            }

            println!("Linking complete. Created {} true graph edges.", edges_created);
            println!("Indexing complete! Run a query to test it.");
        }
        Commands::Query { text } => {
            // Connect to the existing DB
            let db = storage::Database::new(".codebroker/codebroker.db").expect("DB not found. Run init first.");
            
            println!("Searching for: '{}'", text);
            
            // For Phase 0, we just do a raw SQL search across our symbols
            let mut stmt = db.conn.prepare(
                "SELECT files.path, symbols.kind, symbols.name, symbols.start_line 
                 FROM symbols 
                 JOIN files ON symbols.file_id = files.id 
                 WHERE symbols.name LIKE ?1"
            ).unwrap();
            // We use % so it does a wildcard search (e.g. searching "main" finds "main")
            let search_term = format!("%{}%", text);
            let mut rows = stmt.query([search_term]).unwrap();
            while let Some(row) = rows.next().unwrap() {
                let path: String = row.get(0).unwrap();
                let kind: String = row.get(1).unwrap();
                let name: String = row.get(2).unwrap();
                let line: i64 = row.get(3).unwrap();
                println!("Found {} '{}' in {} on line {}", kind, name, path, line);
            }
        }
        Commands::Dependents { symbol } => {

                        let db = storage::Database::new(".codebroker/codebroker.db").expect("DB not found.");
            
            println!("Traversing graph to find dependents of '{}'...", symbol);
            
            match query::engine::find_dependents(&db, symbol) {
                Ok(files) => {
                    if files.is_empty() {
                        println!("No files depend on '{}'. It is safe to delete!", symbol);
                    } else {
                        println!("WARNING: Modifying '{}' will impact the following {} files:", symbol, files.len());
                        for file in files {
                            println!("  -> {}", file);
                        }
                    }
                }
                Err(e) => println!("Error querying graph: {}", e),
            }

        }

        Commands::Context { symbol } => {
            let db = storage::Database::new(".codebroker/codebroker.db").expect("DB not found.");
            
            println!("Assembling context bject for '{}'...\n", symbol);
            // Call our new assembly engine!
            match query::context::ContextObject::assemble(&db, symbol) {
                Ok(Some(context_obj)) => {
                    // This is the magic: We convert our rich graph structs into clean JSON
                    let json_payload = serde_json::to_string_pretty(&context_obj).unwrap();
                    println!("{}", json_payload);
                }
                Ok(None) => println!("Symbol '{}' not found in the graph.", symbol),
                Err(e) => println!("Error assembling context: {}", e),
            }
        }
        Commands::Knowledge => {
            let db = storage::Database::new(".codebroker/codebroker.db").expect("DB not found. Run init first.");
            if let Ok(stats) = db.get_codebroker_stats() {
                println!("\n=== CODEBROKER KNOWLEDGE DASHBOARD ===");
                println!("Files Indexed: {}", stats.files_indexed);
                println!("Summaries Generated: {}", stats.summaries_generated);
                
                // Calculate Hit Rate (Total Queries = Generated + Hits)
                let total_queries = stats.summaries_generated + stats.total_cache_hits;
                let hit_rate = if total_queries > 0 {
                    (stats.total_cache_hits as f64 / total_queries as f64) * 100.0
                } else {
                    0.0
                };
                println!("Cache Hit Rate: {:.1}% ({} cache hits)", hit_rate, stats.total_cache_hits);

                // Calculate Languages
                let mut languages = Vec::new();
                for (ext, _count) in stats.extensions {
                    match ext.as_str() {
                        "rs" => if !languages.contains(&"Rust") { languages.push("Rust"); },
                        "ts" | "tsx" => if !languages.contains(&"TypeScript") { languages.push("TypeScript"); },
                        "js" | "jsx" => if !languages.contains(&"JavaScript") { languages.push("JavaScript"); },
                        "py" => if !languages.contains(&"Python") { languages.push("Python"); },
                        _ => {}
                    }
                }
                
                if !languages.is_empty() {
                    println!("\nLanguages Detected:");
                    for lang in languages {
                        println!(" - {}", lang);
                    }
                }
                println!("======================================\n");
            }
        }
        Commands::Refresh => {
            let db = storage::Database::new(".codebroker/codebroker.db").expect("DB not found.");
            let hf_token = std::env::var("HF_API_TOKEN").unwrap_or_default();
            if hf_token.is_empty() {
                println!("Error: HF_API_TOKEN environment variable is not set.");
                return;
            }
            
            let provider = Box::new(semantic::huggingface::HuggingFaceProvider::new(hf_token));
            let generator = semantic::generator::SummaryGenerator::new(&db, provider);
            
            println!("Regenerating stale summaries in the background...");
            
            if let Ok(mut stmt) = db.conn.prepare("SELECT name FROM symbols") {
                if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) {
                    for symbol in rows.flatten() {
                        let _ = generator.generate(&symbol);
                    }
                }
            }
            println!("Refresh complete!");
        }
        Commands::Explain { symbol } => {
            let db = storage::Database::new(".codebroker/codebroker.db").expect("DB not found.");
            let hf_token = std::env::var("HF_API_TOKEN").unwrap_or_default();
            if hf_token.is_empty() {
                println!("Error: HF_API_TOKEN environment variable is not set.");
                println!("Please run: export HF_API_TOKEN=\"hf_your_token\"");
                return;
            }
            
            println!("Generating semantic summary for '{}'...", symbol);
            
            let provider = Box::new(semantic::huggingface::HuggingFaceProvider::new(hf_token));
            let generator = semantic::generator::SummaryGenerator::new(&db, provider);
            
            match generator.generate(&symbol) {
                Ok((summary, _)) => {
                    println!("\n=== Generated Summary ===");
                    println!("{}", summary);
                    println!("========================\n");
                }
                Err(e) => println!("Error generating summary: {}", e),
            }
        }


        Commands::Dashboard => {
            // Boot a Tokio runtime to host the Axum server indefinitely!
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                analytics::server::start_server().await;
            });
        }
        Commands::Metrics => {
            println!("--- CodeBroker Analytics Engine ---");
            if let Ok(conn) = rusqlite::Connection::open(".codebroker/codebroker.db") {
                let total_tokens_avoided: i64 = conn.query_row("SELECT SUM(token_reduction) FROM mcp_analytics_events", [], |row| row.get(0)).unwrap_or(0);
                let cache_hits: i64 = conn.query_row("SELECT COUNT(*) FROM mcp_analytics_events WHERE cache_hit = 1", [], |row| row.get(0)).unwrap_or(0);
                let cache_misses: i64 = conn.query_row("SELECT COUNT(*) FROM mcp_analytics_events WHERE cache_hit = 0", [], |row| row.get(0)).unwrap_or(0);
                let hit_rate = if cache_hits + cache_misses > 0 {
                    (cache_hits as f64 / (cache_hits + cache_misses) as f64) * 100.0
                } else {
                    0.0
                };
                let total_cost_saved_cents = (total_tokens_avoided as f64 / 1_000_000.0) * 300.0;

                println!("Lifetime Tokens Avoided: {}", total_tokens_avoided);
                println!("Global Cache Hit Rate: {:.1}%", hit_rate);
                println!("LLM Calls Avoided: {}", cache_hits);
                println!("Estimated Cost Savings: ${:.2}", total_cost_saved_cents / 100.0);
            } else {
                println!("Could not connect to codebroker.db");
            }
        }
        Commands::Analytics => {
            println!("Use 'cargo run -- metrics' or 'cargo run -- dashboard' instead.");
        }
        Commands::Bind => {
            println!("Binding CodeBroker to current directory...");
            
            // 1. Get current path and use the globally installed mcp binary
            let current_dir = std::env::current_dir().unwrap().to_string_lossy().to_string();
            
            let new_arg = format!("cd {} && codebroker-mcp", current_dir);
            
            // 2. Paths to configs
            let home_dir = std::env::var("HOME").unwrap_or_default();
            let claude_path = format!("{}/.config/Claude/claude_desktop_config.json", home_dir);
            let gemini_path = format!("{}/.gemini/config/mcp_config.json", home_dir);
            
            let paths_to_update = vec![claude_path, gemini_path];
            
            for path in paths_to_update {
                if let Ok(config_str) = fs::read_to_string(&path) {
                    if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&config_str) {
                        if let Some(servers) = json.get_mut("mcpServers").and_then(|s| s.as_object_mut()) {
                            if let Some(codebroker) = servers.get_mut("codebroker").and_then(|c| c.as_object_mut()) {
                                if let Some(args) = codebroker.get_mut("args").and_then(|a| a.as_array_mut()) {
                                    if args.len() >= 2 {
                                        args[1] = serde_json::Value::String(new_arg.clone());
                                    }
                                }
                                
                                // Inject HF_API_TOKEN
                                let env_map = codebroker.entry("env").or_insert_with(|| serde_json::json!({}));
                                if let Some(env_obj) = env_map.as_object_mut() {
                                    env_obj.insert("HF_API_TOKEN".to_string(), serde_json::Value::String("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".to_string()));
                                }
                                
                                if let Ok(new_json) = serde_json::to_string_pretty(&json) {
                                    let _ = fs::write(&path, new_json);
                                    println!("Successfully bound config at {}", path);
                                }
                            }
                        }
                    }
                } else {
                    println!("Warning: Config not found at {}", path);
                }
            }
            
            // 2.5 Generate Antigravity instructions.md locally, then sync globally
            let _ = fs::create_dir_all(".codebroker");
            let local_instructions_path = ".codebroker/instructions.md";
            
            let default_instructions = r#"# CodeBroker MCP Tools
You are connected to the CodeBroker MCP Server for the current workspace.

**CRITICAL RULES:**
1. **Focus Exclusively on this Workspace:** Your primary objective is to analyze and edit the repository where CodeBroker is initialized.
2. **Prioritize CodeBroker Tools:** ALWAYS use `search_codebase`, `find_symbol`, and `get_context` over native tools like `grep_search` or `read_file`.
3. **Be Concise & Semantic:** Do not use `grep_search` for code discovery; `search_codebase` is semantic and vastly superior.
4. **Context Management:** Never read entire files unless necessary. Use `read_file_skeleton` or `read_symbol_source` to preserve your context window.
5. **Architectural Understanding:** When asked how something works or what its impact is, immediately use `project_overview` or `impact_analysis` instead of guessing."#;

            // Write locally if it doesn't exist
            if !std::path::Path::new(local_instructions_path).exists() {
                let _ = fs::write(local_instructions_path, default_instructions);
                println!("Created local project instructions at {}", local_instructions_path);
            }

            // Sync to Antigravity and Claude
            if let Ok(local_content) = fs::read_to_string(local_instructions_path) {
                // Antigravity
                let antigravity_dir = format!("{}/.gemini/antigravity/mcp/codebroker", home_dir);
                if fs::create_dir_all(&antigravity_dir).is_ok() {
                    let global_instructions_path = format!("{}/instructions.md", antigravity_dir);
                    if fs::write(&global_instructions_path, &local_content).is_ok() {
                        println!("Successfully synced AI instructions to {}", global_instructions_path);
                    } else {
                        println!("Warning: Failed to write global instructions.md at {}", global_instructions_path);
                    }
                }
                
                // Claude Desktop
                let claude_dir = format!("{}/.config/Claude/mcp/codebroker", home_dir);
                if fs::create_dir_all(&claude_dir).is_ok() {
                    let claude_instructions_path = format!("{}/instructions.md", claude_dir);
                    if fs::write(&claude_instructions_path, &local_content).is_ok() {
                        println!("Successfully synced AI instructions to {}", claude_instructions_path);
                    } else {
                        println!("Warning: Failed to write global instructions.md at {}", claude_instructions_path);
                    }
                }
            }
            
            // 3. Auto-Init the directory so it's ready!
            println!("Initializing CodeBroker database...");
            let _ = std::process::Command::new(std::env::current_exe().unwrap())
                .arg("init")
                .status();
                
            println!("Done! Restart Claude Desktop or Antigravity to begin.");
        }
        }
    }
