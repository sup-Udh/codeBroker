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
    Dashboard
}


fn main() {
    let cli = Cli::parse();
    match &cli.command {
        Commands::Init => {
            println!("Initializing CodeBroker...");

            let _ = fs::remove_file("codebroker.db");

            
            // 1. Boot up the database
            let db = storage::Database::new("codebroker.db").expect("Failed to create DB");
            db.init_schema().expect("Failed to initialize schema");
            use parser::frontend::{LanguageFrontend, RustFrontend};
            use parser::typescript_frontend::{TypeScriptFrontend, TsxFrontend};
            use parser::python_frontend::PythonFrontend;
            use parser::javascript_frontend::JavaScriptFrontend;
            use parser::config_frontend::ConfigFrontend;
             let frontends: Vec<Box<dyn LanguageFrontend>> = vec![
                Box::new(RustFrontend),
                Box::new(TypeScriptFrontend),
                Box::new(TsxFrontend),
                Box::new(PythonFrontend),
                Box::new(JavaScriptFrontend),
                Box::new(ConfigFrontend),
            ];

            // 1.5 Load Aliases
            let mut alias_map: Vec<(String, String)> = Vec::new();
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
                            if file_path.contains("app/") {
                                let parts: Vec<&str> = file_path.split("app/").collect();
                                if parts.len() > 1 {
                                    let route_parts: Vec<&str> = parts[1].split('/').collect();
                                    if route_parts.len() > 0 {
                                        let file_name = route_parts.last().unwrap();
                                        route_segment = Some(file_name.split('.').next().unwrap_or(file_name).to_string());
                                        let dir_path = route_parts[..route_parts.len()-1].join("/");
                                        route_path = Some(format!("/{}", dir_path));
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
            for (_raw_id, source_file_id, import_name, import_source) in raw_imports {
                // Determine if we have a source path we can resolve via aliases
                let mut resolved_source = import_source.clone();
                if let Some(src) = &import_source {
                    for (alias, path_prefix) in &alias_map {
                        if src.starts_with(alias) {
                            resolved_source = Some(src.replace(alias, path_prefix));
                            break;
                        }
                    }
                }

                // If we resolved a path, let's try to link exactly to that file's export
                if let Some(src) = resolved_source {
                    // Very rudimentary resolution: find a file containing the path
                    let mut file_stmt = db.conn.prepare("SELECT id FROM files WHERE path LIKE ?1 LIMIT 1").unwrap();
                    let search_path = format!("%{}%", src);
                    if let Ok(target_file_id) = file_stmt.query_row(params![search_path], |row| row.get::<_, i64>(0)) {
                        // find a symbol in that file that matches the name
                        let mut sym_stmt = db.conn.prepare("SELECT id FROM symbols WHERE file_id = ?1 AND name = ?2 LIMIT 1").unwrap();
                        if let Ok(target_symbol_id) = sym_stmt.query_row(params![target_file_id, import_name], |row| row.get::<_, i64>(0)) {
                            let _ = db.insert_edge(source_file_id, target_symbol_id, "imports");
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
                        let _ = db.insert_edge(source_file_id, target_symbol_id, "imports");
                        edges_created += 1;
                    }
                }
            }

            println!("Linking complete. Created {} true graph edges.", edges_created);
            println!("Indexing complete! Run a query to test it.");
        }
        Commands::Query { text } => {
            // Connect to the existing DB
            let db = storage::Database::new("codebroker.db").expect("DB not found. Run init first.");
            
            println!("Searching for: '{}'", text);
            
            // For Phase 0, we just do a raw SQL search across our symbols
            let mut stmt = db.conn.prepare(
                "SELECT files.path, symbols.kind, symbols.name, symbols.line_number 
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

                        let db = storage::Database::new("codebroker.db").expect("DB not found.");
            
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
            let db = storage::Database::new("codebroker.db").expect("DB not found.");
            
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
            let db = storage::Database::new("codebroker.db").expect("DB not found. Run init first.");
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
            let db = storage::Database::new("codebroker.db").expect("DB not found.");
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
            let db = storage::Database::new("codebroker.db").expect("DB not found.");
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
            if let Ok(conn) = rusqlite::Connection::open("codebroker.db") {
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




        }
    }
