use clap::{Parser, Subcommand};
use parser::frontend;
use std::fs;

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
            use parser::typescript_frontend::TypeScriptFrontend;
            use parser::python_frontend::PythonFrontend;
            use parser::javascript_frontend::JavaScriptFrontend;
            use parser::config_frontend::ConfigFrontend;
             let frontends: Vec<Box<dyn LanguageFrontend>> = vec![
                Box::new(RustFrontend),
                Box::new(TypeScriptFrontend),
                Box::new(PythonFrontend),
                Box::new(JavaScriptFrontend),
                Box::new(ConfigFrontend),
            ];

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
                        if let Some((symbols, imports)) = frontend.parse_and_extract(&source_code) {
                            
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
            for (_raw_id, source_file_id, import_name) in raw_imports {
                // Split the import path (like `storage::Database` or `graph::{SymbolNode}`) into individual words
                let words: Vec<&str> = import_name.split(|c: char| !c.is_alphanumeric()).collect();
                
                for word in words {
                    if word.is_empty() { continue; }
                    
                    // 3. Ask the database if a real Symbol exists with this exact word
                    if let Ok(Some(target_symbol_id)) = db.find_symbol_id_by_name(word) {
                        // 4. We found a match! Draw the physical Edge in the graph.
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


        }
    }
