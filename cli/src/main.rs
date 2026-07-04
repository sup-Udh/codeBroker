use clap::{Parser, Subcommand};
use rusqlite::params;
use std::fs;
use storage::GENERIC_SYMBOL_NAMES;

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
    Query {
        text: String,
    },
    Dependents {
        symbol: String,
    },
    Context {
        symbol: String,
    },
    Explain {
        symbol: String,
    },
    Knowledge,
    Refresh,
    Metrics,
    Analytics,
    Dashboard,
    /// Re-parses only the given files and re-links their edges, instead of a
    /// full repository rebuild. Faster for small edits; see indexer::reindex
    /// for what it intentionally skips relative to a full Init.
    ReindexIncremental {
        #[arg(required = true)]
        paths: Vec<String>,
    },
    /// Instantly hooks up Claude Desktop and Antigravity to the current directory
    Bind,
    /// Validates the structure and health of the compiled graph
    Validate,
    /// Inspects a single relationship's deterministic resolution trace
    Inspect {
        relationship_id: i64,
    },
    /// Analyzes the resolution pipeline and outputs aggregate statistics
    AnalyzeResolution,
    /// Prints a high-level summary of the project
    Summary,
    /// Prints a logical overview of the project's subsystems
    Overview,
    /// Prints project structural hotspots
    Hotspots,
    /// Prints architectural layers inferred from dependencies
    Architecture,
    /// Prints categorized entrypoints
    Entrypoints,
    /// Prepares context for a subsystem
    PrepareContext {
        target: String,
    },
    /// Updates the CodeBroker CLI to the latest release
    Update,
    /// Internal: backfills missing symbol embeddings against the already-
    /// published database. Spawned as a detached background process by
    /// `init` so semantic-search embedding calls (network-bound, can take
    /// tens of seconds) never block indexing completion. Not meant to be
    /// invoked directly.
    #[command(hide = true, name = "_embed-backfill")]
    EmbedBackfill,
}

fn main() {
    let cli = Cli::parse();
    match &cli.command {
        Commands::Init => {
            use indicatif::{ProgressBar, ProgressStyle};
            use std::sync::Arc;
            use std::sync::atomic::{AtomicUsize, Ordering};

            let t_pipeline = std::time::Instant::now();
            let show_timing = std::env::var("CODEBROKER_TIMING").is_ok();

            let spinner_style = ProgressStyle::with_template(
                "  🔴 {spinner:.red}  {wide_msg}",
            )
            .unwrap()
            .tick_strings(&["⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧","⠇","⠏",""]);

            let pb = ProgressBar::new_spinner();
            pb.set_style(spinner_style);
            pb.enable_steady_tick(std::time::Duration::from_millis(80));
            pb.set_message("Churning  —  loading cache & warming database");

            let _ = fs::create_dir_all(".codebroker");

            const FINAL_DB_PATH: &str = ".codebroker/codebroker.db";
            const TMP_DB_PATH: &str = ".codebroker/codebroker.db.tmp";
            // Bumped to "13": extract_ts_symbols now indexes module-level
            // singleton instances (`export const inventoryRepository = new
            // InventoryRepository();`, a common DI pattern) as symbols, and
            // VariableFlowEngine::load_imports (reordered after
            // load_constructors) now follows such a singleton one hop to its
            // own constructed type instead of using its lowercase instance
            // name as if it were the class itself — find_method_in_type
            // needs the actual class name. Also fixed extract_ts_symbols
            // deriving its query-cache key from the file path's ".tsx"
            // suffix instead of an explicit is_tsx parameter: any caller
            // parsing non-.tsx-suffixed content with the TSX grammar (e.g.
            // VueFrontend parsing a .vue file's <script> block through
            // TsxFrontend) collided with the "typescript_symbols" cache slot
            // populated by a real .ts file parsed with the plain TypeScript
            // grammar — a query compiled for one grammar silently matches
            // nothing against a tree built with the other. This made Vue
            // (and any similarly-embedded TSX-grammar) symbol extraction
            // depend on file processing order, worked in isolation and
            // failed once enough real .ts files primed the wrong cache slot.
            // Existing indexes must be fully reparsed.
            const PIPELINE_VERSION: &str = "13";

            // Load embeddings from the OLD database before wiping it.
            // On every full `init`, the temp DB starts empty, so without
            // this cache every symbol would be re-embedded via the API even
            // if it hasn't changed — the dominant bottleneck as codebases grow.
            let t_cache_load = std::time::Instant::now();
            let embedding_cache = semantic::embeddings::load_embedding_cache(FINAL_DB_PATH);
            if show_timing {
                eprintln!("[TIMING] Load old embedding cache: {}ms ({} cached embeddings)",
                    t_cache_load.elapsed().as_millis(), embedding_cache.len());
            }

            let _ = fs::remove_file(TMP_DB_PATH);
            let _ = fs::remove_file(format!("{}-wal", TMP_DB_PATH));
            let _ = fs::remove_file(format!("{}-shm", TMP_DB_PATH));

            // True incremental init: if a previous index exists, copy it into
            // the temp path instead of starting empty, then only reparse the
            // files that actually changed below — unchanged files' rows are
            // simply never touched, already sitting correctly in the copy.
            // Checkpoint the live DB first so the plain file copy is a
            // complete, self-contained snapshot (its WAL may hold commits —
            // e.g. from an MCP-triggered `reindex_paths` — never checkpointed).
            let has_previous_index = std::path::Path::new(FINAL_DB_PATH).exists();
            if has_previous_index {
                if let Ok(old_db) = storage::Database::new(FINAL_DB_PATH) {
                    let _ = old_db.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
                }
                let _ = fs::copy(FINAL_DB_PATH, TMP_DB_PATH);
            }

            // 1. Boot up the database
            // Scoped so `db` (and every Statement borrowed from it) is fully dropped,
            // releasing the file, before we checkpoint/rename below.
            {
                let t_db_open = std::time::Instant::now();
                let db = storage::Database::new(TMP_DB_PATH).expect("Failed to create DB");
                db.init_schema().expect("Failed to initialize schema");
                if show_timing { eprintln!("[TIMING] DB open + schema init: {}ms", t_db_open.elapsed().as_millis()); }

                // Trust content hashes to predict old parse output only if
                // the copied index was built by this same pipeline version.
                let stored_version: Option<String> = db
                    .conn
                    .query_row(
                        "SELECT value FROM metadata WHERE key = 'pipeline_version'",
                        [],
                        |r| r.get(0),
                    )
                    .ok();
                let trust_hashes =
                    has_previous_index && stored_version.as_deref() == Some(PIPELINE_VERSION);
                let full_rebuild = !trust_hashes;
                if has_previous_index && !trust_hashes {
                    println!("Pipeline version changed since last index — reparsing every file.");
                }

                let t_old_hashes = std::time::Instant::now();
                let mut old_hashes: std::collections::HashMap<String, (i64, String)> =
                    std::collections::HashMap::new();
                {
                    let mut stmt = db
                        .conn
                        .prepare("SELECT id, path, content_hash FROM files WHERE content_hash IS NOT NULL")
                        .expect("Failed to query existing files");
                    let mut rows = stmt.query([]).expect("Failed to read existing files");
                    while let Some(row) = rows.next().expect("Failed to step existing files") {
                        let id: i64 = row.get(0).unwrap();
                        let path: String = row.get(1).unwrap();
                        let hash: String = row.get(2).unwrap();
                        old_hashes.insert(path, (id, hash));
                    }
                }
                if show_timing {
                    eprintln!("[TIMING] Load previous file hashes: {}ms ({} files known)",
                        t_old_hashes.elapsed().as_millis(), old_hashes.len());
                }

                use parser::config_frontend::ConfigFrontend;
                use parser::frontend::{LanguageFrontend, RustFrontend};
                use parser::javascript_frontend::JavaScriptFrontend;
                use parser::python_frontend::PythonFrontend;
                use parser::svelte_frontend::SvelteFrontend;
                use parser::typescript_frontend::{TsxFrontend, TypeScriptFrontend};
                use parser::vue_frontend::VueFrontend;
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
                if let Ok(config_str) = fs::read_to_string("tsconfig.json")
                    .or_else(|_| fs::read_to_string("jsconfig.json"))
                {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&config_str) {
                        if let Some(paths) = json
                            .get("compilerOptions")
                            .and_then(|c| c.get("paths"))
                            .and_then(|p| p.as_object())
                        {
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
                if let Ok(config_str) = fs::read_to_string("vite.config.ts")
                    .or_else(|_| fs::read_to_string("vite.config.js"))
                {
                    if let Ok(re) = regex::Regex::new(
                        r#"['"]?([^'"]+)['"]?\s*:\s*(?:fileURLToPath\(new URL\(['"]([^'"]+)['"]|path\.resolve\(__dirname,\s*['"]([^'"]+)['"]|['"]([^'"]+)['"])"#,
                    ) {
                        for cap in re.captures_iter(&config_str) {
                            let key = cap.get(1).map_or("", |m| m.as_str()).to_string();
                            let val = cap
                                .get(2)
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
                pb.set_message("Discovering  —  walking the filesystem");
                let t_walk = std::time::Instant::now();
                let files = indexer::walker::collect_files(".");
                let total_files = files.len();
                if show_timing {
                    eprintln!("[TIMING] File walk: {}ms ({} files found)", t_walk.elapsed().as_millis(), total_files);
                }
                pb.println(format!("     Found {} files to index.", total_files));

                // 3. The Main Indexing Loop — track per-file progress via an
                // AtomicUsize polled by a background thread that refreshes the
                // spinner message every 80 ms with a live count + ETA.
                let t_pass1 = std::time::Instant::now();
                let t_parse = std::time::Instant::now();

                let processed = Arc::new(AtomicUsize::new(0));

                // Background watcher thread: updates message with count + ETA
                let pb_watcher = pb.clone();
                let processed_watcher = processed.clone();
                let watcher = std::thread::spawn(move || {
                    let t0 = std::time::Instant::now();
                    loop {
                        let n = processed_watcher.load(Ordering::Relaxed);
                        let eta_str = if n > 1 {
                            let rate = n as f64 / t0.elapsed().as_secs_f64().max(0.001);
                            let rem = total_files.saturating_sub(n);
                            let secs = (rem as f64 / rate).ceil() as u64;
                            if secs < 60 { format!("~{}s left", secs) }
                            else { format!("~{}m {}s left", secs / 60, secs % 60) }
                        } else {
                            "calculating…".to_string()
                        };
                        pb_watcher.set_message(format!(
                            "Indexing  —  {}/{} files  ·  {}",
                            n, total_files, eta_str
                        ));
                        if n >= total_files { break; }
                        std::thread::sleep(std::time::Duration::from_millis(80));
                    }
                });

                let processed_rayon = processed.clone();
                let statuses: Vec<indexer::parsed_file::FileStatus> = {
                    use rayon::prelude::*;
                    files
                        .par_iter()
                        .filter_map(|file_path| {
                            let r = indexer::parsed_file::classify_and_parse(
                                file_path,
                                &old_hashes,
                                trust_hashes,
                                &frontends,
                            );
                            processed_rayon.fetch_add(1, Ordering::Relaxed);
                            r
                        })
                        .collect()
                };
                let _ = watcher.join();
                let mut parsed_files: Vec<indexer::parsed_file::ParsedFile> = Vec::new();
                let mut unchanged_count = 0usize;
                for status in statuses {
                    match status {
                        indexer::parsed_file::FileStatus::Unchanged => unchanged_count += 1,
                        indexer::parsed_file::FileStatus::Changed(pf) => parsed_files.push(pf),
                    }
                }
                parsed_files.sort_by(|a, b| a.path.cmp(&b.path));
                let pass1_parse_ms = t_parse.elapsed().as_millis();

                // Files that existed in the previous index but weren't seen
                // in this walk at all — deleted from disk since last `init`.
                let current_paths: std::collections::HashSet<&str> =
                    files.iter().map(|s| s.as_str()).collect();
                let deleted_file_ids: Vec<i64> = old_hashes
                    .iter()
                    .filter(|(path, _)| !current_paths.contains(path.as_str()))
                    .map(|(_, (id, _))| *id)
                    .collect();

                let t_db_ins = std::time::Instant::now();
                let mut pass1_files = 0usize;
                let mut pass1_symbols = 0usize;
                let mut pass1_imports = 0usize;
                let mut touched_file_ids: Vec<i64> = Vec::new();
                // One transaction for every delete/file/symbol/relationship/
                // binding write in this pass instead of an implicit
                // autocommit per statement — the dominant cost at larger
                // file counts.
                db.conn.execute_batch("BEGIN IMMEDIATE").unwrap();
                for &old_id in &deleted_file_ids {
                    let _ = db.delete_file_data(old_id);
                }
                for pf in &parsed_files {
                    // A changed file that already had a row copied forward
                    // from the previous index must have that stale row (and
                    // its now-dangling edges) removed first — its symbols
                    // are about to get new auto-increment ids.
                    if let Some((old_id, _)) = old_hashes.get(&pf.path) {
                        let _ = db.delete_file_data(*old_id);
                    }
                    let metadata_str = pf.metadata.metadata.as_deref().unwrap_or("{}");
                    let file_id = db.insert_file(&pf.path, &pf.content_hash).unwrap();
                    touched_file_ids.push(file_id);
                    let _ = db.update_file_metadata(file_id, Some(metadata_str));
                    pass1_files += 1;

                    // Deduplicate symbols by (name, kind, start_byte) before
                    // insertion to prevent duplicate DB rows from overlapping
                    // tree-sitter query captures.
                    let mut seen_syms = std::collections::HashSet::new();
                    for symbol in &pf.symbols {
                        let key = (symbol.name.clone(), symbol.kind.clone(), symbol.start_byte);
                        if seen_syms.insert(key) {
                            db.insert_symbol(file_id, symbol).unwrap();
                            pass1_symbols += 1;
                        }
                    }

                    for import in &pf.relationships {
                        db.insert_relationship(file_id, import).unwrap();
                        pass1_imports += 1;
                    }

                    for binding in &pf.semantic_bindings {
                        let _ = db.insert_semantic_binding(file_id, binding);
                    }
                }
                db.conn.execute_batch("COMMIT").unwrap();
                let pass1_db_insert_ms = t_db_ins.elapsed().as_millis();
                if show_timing {
                    eprintln!(
                        "[TIMING] Pass 1 (parse + DB inserts): {}ms total | parse={}ms db_inserts={}ms | changed={} unchanged={} deleted={} symbols={} relationships={}",
                        t_pass1.elapsed().as_millis(), pass1_parse_ms, pass1_db_insert_ms,
                        pass1_files, unchanged_count, deleted_file_ids.len(), pass1_symbols, pass1_imports
                    );
                }

                // Nothing to relink/recompute/re-embed if this was a true
                // incremental run and literally no file changed or was
                // deleted — this is the fast path that gets a no-op `init`
                // down toward the sub-200ms target instead of re-running a
                // full-repo linker/feature/embedding pass for no reason.
                let nothing_changed =
                    !full_rebuild && touched_file_ids.is_empty() && deleted_file_ids.is_empty();

                let mut edges_created: usize = 0;

              if nothing_changed {
                pb.set_message("Already up to date  —  no files changed since last index");
              } else {
                pb.set_message("Linking  —  weaving graph edges");
                let t_pass2 = std::time::Instant::now();
                let metrics = if full_rebuild {
                    indexer::resolver::resolve_relationships(&db, None).expect("Linker failed")
                } else {
                    indexer::resolver::resolve_relationships(&db, Some(&touched_file_ids))
                        .unwrap_or_default()
                };
                edges_created = metrics.relationships_emitted;
                let total_relationships = metrics.relationships_input;
                let version = env!("CARGO_PKG_VERSION");
                let _ = db.save_pipeline_manifest(version, "1.0", "1.0", "1.0", "1.0", "1.0", pass1_files as i64, pass1_symbols as i64, total_relationships as i64, edges_created as i64);
                if show_timing { eprintln!("[TIMING] Pass 2 (edge linking): {}ms", t_pass2.elapsed().as_millis()); }

                // 4.4 Infer logical interaction edges (HTTP/WebSocket runtime
                // boundaries) and then precompute graph features (pagerank,
                // fan-in/out, communities, and the is_entrypoint flag). The
                // full `init` previously skipped BOTH of these passes — they
                // only ran on the incremental `reindex_paths` path — so a
                // freshly-`init`ed repository had an empty `symbol_features`
                // table: no entrypoints, no centrality, no interaction edges,
                // until something happened to trigger an incremental reindex.
                // That made `list_entrypoints`/`project_overview` report zero
                // entrypoints on a clean index regardless of how routes were
                // detected. Running them here makes a full index complete and
                // identical in shape to the incremental path.
                pb.set_message("Wiring  —  inferring runtime interaction edges");
                let t_interactions = std::time::Instant::now();
                let _ = indexer::interactions::infer_interactions(&db);
                if show_timing { eprintln!("[TIMING] infer_interactions: {}ms", t_interactions.elapsed().as_millis()); }

                pb.set_message("Wiring  —  computing pagerank & entrypoints");
                let t_features = std::time::Instant::now();
                let _ = indexer::features::extract_features(&db);
                if show_timing { eprintln!("[TIMING] extract_features: {}ms", t_features.elapsed().as_millis()); }

                pb.set_message("Wiring  —  validating graph structure");
                let t_validate = std::time::Instant::now();
                let validation_summary = match query::validation::validate(&db) {
                    Ok(report) => format!(
                        "     Graph: {} symbols  ·  {} edges  ·  {:.1}% resolution",
                        report.total_symbols, report.total_edges,
                        report.import_resolution_rate() * 100.0,
                    ),
                    Err(_) => String::new(),
                };
                match query::metrics::compute_metrics(&db) {
                    Ok(metrics) => { let _ = query::metrics::save_metrics(&db, &metrics); }
                    Err(_) => {}
                }
                if show_timing { eprintln!("[TIMING] validate+metrics: {}ms", t_validate.elapsed().as_millis()); }

                pb.set_message("Wiring  —  tagging domain concepts");
                let t_concepts = std::time::Instant::now();
                let _ = query::concepts::tag_concepts(&db);
                if show_timing { eprintln!("[TIMING] tag_concepts: {}ms", t_concepts.elapsed().as_millis()); }

                // 4.5 Replay embeddings for symbols the pre-wipe cache still
                // covers, if a key is configured. Silently skipped (not an
                // error) without OPENAI_API_KEY, matching every other
                // AI-backed feature's degrade-gracefully behavior. This is
                // DB-only (no network calls), so it stays synchronous here —
                // it's the actual `backfill_missing_embeddings` API calls
                // (network-bound, can take tens of seconds) that get deferred
                // to a detached background process after publish, below.
                let openai_key = runtime::environment::openai_api_key().unwrap_or_default();
                if !openai_key.is_empty() {
                    pb.set_message("Embedding  —  replaying semantic cache");
                    let t_apply = std::time::Instant::now();
                    let cache_hits = semantic::embeddings::apply_embedding_cache(&db, &embedding_cache);
                    if show_timing {
                        eprintln!("[TIMING] Apply embedding cache: {}ms ({} hits from cache, bypassed API)",
                            t_apply.elapsed().as_millis(), cache_hits);
                    }
                }

                // Print the validation summary line (persists above final Done line)
                if !validation_summary.is_empty() {
                    pb.println(&validation_summary);
                }
              }

                // 5. Update Metadata timestamp
                if let Ok(timestamp) =
                    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                {
                    let _ = db.conn.execute(
                    "INSERT OR REPLACE INTO metadata (key, value) VALUES ('last_indexed_at', ?1)",
                    rusqlite::params![timestamp.as_secs().to_string()]
                );
                }
                // Stamp the pipeline version that produced this index so the
                // next `init` knows whether it can trust stored content
                // hashes to predict old parse output.
                let _ = db.conn.execute(
                    "INSERT OR REPLACE INTO metadata (key, value) VALUES ('pipeline_version', ?1)",
                    rusqlite::params![PIPELINE_VERSION],
                );

                // Flush WAL into the main file before closing, so the temp file is a
                // single self-contained snapshot before we publish it.
                pb.set_message("Publishing  —  checkpointing & finalizing index");
                let t_wal = std::time::Instant::now();
                let _ = db.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
                if show_timing {
                    let pipeline_secs = (t_pipeline.elapsed().as_millis() as f64 / 1000.0).max(0.001);
                    eprintln!("[TIMING] WAL checkpoint: {}ms", t_wal.elapsed().as_millis());
                    eprintln!("[TIMING] === PIPELINE TOTAL (inside DB scope): {}ms ===", t_pipeline.elapsed().as_millis());
                    eprintln!(
                        "[TIMING] === THROUGHPUT: {:.1} files/sec | {:.1} symbols/sec | {:.1} relationships/sec | {:.1} edges/sec ===",
                        pass1_files as f64 / pipeline_secs,
                        pass1_symbols as f64 / pipeline_secs,
                        pass1_imports as f64 / pipeline_secs,
                        edges_created as f64 / pipeline_secs,
                    );
                }

                let elapsed_secs = t_pipeline.elapsed().as_secs_f64();
                let elapsed_str = if elapsed_secs < 60.0 {
                    format!("{:.1}s", elapsed_secs)
                } else {
                    format!("{}m {}s", elapsed_secs as u64 / 60, elapsed_secs as u64 % 60)
                };
                pb.finish_and_clear();
                println!(
                    "  ✅  Done  —  {} files  ·  {} symbols  ·  {} edges  ·  {}",
                    total_files, pass1_symbols, edges_created, elapsed_str
                );
            }

            let _ = fs::remove_file(format!("{}-wal", FINAL_DB_PATH));
            let _ = fs::remove_file(format!("{}-shm", FINAL_DB_PATH));
            fs::rename(TMP_DB_PATH, FINAL_DB_PATH).expect("Failed to publish rebuilt index");
            let _ = fs::remove_file(format!("{}-wal", TMP_DB_PATH));
            let _ = fs::remove_file(format!("{}-shm", TMP_DB_PATH));

            // Backfill any embeddings the pre-wipe cache couldn't cover
            // (genuinely new/changed symbols) in a detached background
            // process against the now-published database, instead of
            // blocking this process on OpenAI API calls before returning.
            if !runtime::environment::openai_api_key().unwrap_or_default().is_empty() {
                if let Ok(exe) = std::env::current_exe() {
                    let _ = std::process::Command::new(exe)
                        .arg("_embed-backfill")
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                }
            }

            if show_timing {
                eprintln!("[TIMING] === TOTAL WALL TIME (including file rename): {}ms ===", t_pipeline.elapsed().as_millis());
            }
        }
        Commands::Update => {
            println!("Checking for updates...");
            let r2_url = "https://pub-aa0624d820a7465aa2d7388f8ad39d1b.r2.dev";
            let latest_version_url = format!("{}/latest_version.txt", r2_url);
            
            let latest_version = match reqwest::blocking::get(&latest_version_url) {
                Ok(resp) => resp.text().unwrap_or_default().trim().to_string(),
                Err(e) => {
                    println!("Failed to check for updates. Could not reach update server: {}", e);
                    return;
                }
            };

            let current_version = env!("CARGO_PKG_VERSION");
            if latest_version.is_empty() {
                println!("Failed to parse the latest version from the server.");
                return;
            }

            // A binary that self-updated *before* this check existed would
            // have replaced itself and stopped, never copying the sibling
            // codebroker-mcp binary out of the archive — so "versions match"
            // alone isn't sufficient to skip: also re-run if that sibling
            // binary is missing from disk, so a stuck install can self-heal
            // on the very next `codebroker update` instead of needing a
            // fresh install.sh/install.ps1 run.
            let mcp_missing = runtime::executables::find_mcp_binary().is_err();

            if latest_version == current_version && !mcp_missing {
                println!("You are already on the latest version ({}).", current_version);
                return;
            }

            if latest_version == current_version {
                println!(
                    "Already on version {}, but codebroker-mcp is missing locally — re-downloading to install it.",
                    current_version
                );
            } else {
                println!("New version found: {}. Updating...", latest_version);
            }

            // Determine OS and Arch
            let os = std::env::consts::OS;
            let arch = std::env::consts::ARCH;
            
            let os_name = match os {
                "linux" => "linux",
                "macos" | "darwin" => "macos",
                "windows" => "windows",
                _ => { println!("Unsupported OS for auto-update."); return; }
            };

            let arch_name = match arch {
                "x86_64" => "x86_64",
                "aarch64" => "aarch64",
                _ => { println!("Unsupported architecture for auto-update."); return; }
            };

            let ext = if os == "windows" { "zip" } else { "tar.gz" };
            let asset_name = format!("codebroker-{}-{}.{}", os_name, arch_name, ext);
            let download_url = format!("{}/{}", r2_url, asset_name);

            let tmp_dir = tempfile::Builder::new().prefix("codebroker-update").tempdir().unwrap();
            let archive_path = tmp_dir.path().join(&asset_name);
            
            println!("Downloading {}...", download_url);
            let mut resp = reqwest::blocking::get(&download_url).unwrap();
            let mut archive_file = std::fs::File::create(&archive_path).unwrap();
            std::io::copy(&mut resp, &mut archive_file).unwrap();

            println!("Extracting...");
            let archive_format = if os == "windows" {
                self_update::ArchiveKind::Zip
            } else {
                self_update::ArchiveKind::Tar(Some(self_update::Compression::Gz))
            };
            
            self_update::Extract::from_source(&archive_path)
                .archive(archive_format)
                .extract_into(tmp_dir.path()).unwrap();

            let exe_name = if os == "windows" { "codebroker.exe" } else { "codebroker" };
            let new_binary = tmp_dir.path().join(exe_name);

            // The archive also contains codebroker-mcp — self_replace only
            // swaps the currently-running binary, so the sibling MCP server
            // binary has to be copied into place separately or `update`
            // leaves it stale (or entirely absent on a first-time install).
            let mcp_exe_name = format!("codebroker-mcp{}", runtime::platform::exe_suffix());
            let new_mcp_binary = tmp_dir.path().join(&mcp_exe_name);
            if new_mcp_binary.exists() {
                let current_exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from(exe_name));
                let bin_dir = current_exe.parent().unwrap_or_else(|| std::path::Path::new("."));
                let mcp_dest = bin_dir.join(&mcp_exe_name);
                match std::fs::copy(&new_mcp_binary, &mcp_dest) {
                    Ok(_) => {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            let _ = std::fs::set_permissions(&mcp_dest, std::fs::Permissions::from_mode(0o755));
                        }
                        println!("Updated {}", mcp_dest.display());
                    }
                    Err(e) => println!("Warning: failed to update {}: {}", mcp_exe_name, e),
                }
            } else {
                println!(
                    "Warning: release archive did not contain {} — the MCP server was not updated.",
                    mcp_exe_name
                );
            }

            println!("Replacing executable...");
            self_replace::self_replace(&new_binary).unwrap();

            println!("Successfully updated to version {}!", latest_version);
        }
        Commands::Query { text } => {
            // Connect to the existing DB
            let db = storage::Database::new(".codebroker/codebroker.db")
                .expect("DB not found. Run init first.");

            println!("Searching for: '{}'", text);

            // For Phase 0, we just do a raw SQL search across our symbols
            let mut stmt = db
                .conn
                .prepare(
                    "SELECT files.path, symbols.kind, symbols.name, symbols.start_line 
                 FROM symbols 
                 JOIN files ON symbols.file_id = files.id 
                 WHERE symbols.name LIKE ?1",
                )
                .unwrap();
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
                        println!(
                            "WARNING: Modifying '{}' will impact the following {} files:",
                            symbol,
                            files.len()
                        );
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

            println!("Assembling context object for '{}'...\n", symbol);
            // Call our new assembly engine!
            let profile = query::response::ResponseProfile::Standard;
            match query::context::ContextResponseBuilder::new(&db, symbol, None, profile) {
                Ok(Some(builder)) => {
                    // This is the magic: We convert our rich graph structs into clean JSON
                    let json_payload =
                        serde_json::to_string_pretty(&builder.build_json().unwrap()).unwrap();
                    println!("{}", json_payload);
                }
                Ok(None) => println!("Symbol '{}' not found in the graph.", symbol),
                Err(e) => println!("Error assembling context: {}", e),
            }
        }
        Commands::Inspect { relationship_id } => {
            let current_dir = std::env::current_dir().unwrap();
            let db_path = current_dir.join(".codebroker").join("codebroker.db");
            if !db_path.exists() {
                eprintln!("Error: Not a CodeBroker repository. Run `codebroker init` first.");
                std::process::exit(1);
            }
            let db = storage::Database::new(db_path.to_str().unwrap()).expect("Failed to open DB");
            let raw_rel = db.conn.query_row(
                "SELECT file_id, name, source, kind, line_number FROM relationships WHERE id = ?1",
                rusqlite::params![relationship_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                }
            );

            match raw_rel {
                Ok((file_id, name, source, kind, line_number)) => {
                    let enclosing_symbol = db.enclosing_symbol_id(file_id, line_number).unwrap_or(None);
                    
                    let input_ir = indexer::ir::RelationshipIR {
                        id: *relationship_id,
                        source_file_id: file_id,
                        node: graph::models::RelationshipNode {
                            name: name.clone(),
                            source: source.clone(),
                            line_number: line_number as usize,
                            kind,
                        },
                        enclosing_symbol_id: enclosing_symbol,
                    };

                    println!("Replaying resolution for Relationship ID: {}", relationship_id);
                    println!("Relationship: {} (Source: {:?})\n", name, source);
                    
                    use indexer::pipeline::PipelineStage;
                    // Run resolver with tracing enabled
                    let resolver = indexer::resolver::Resolver::new(&db, true);
                    let resolved = resolver.execute(vec![input_ir]).unwrap();
                    
                    if let Some(res) = resolved.first() {
                        for decision in &res.decisions {
                            println!("\n──────────────────────────");
                            println!("{}", decision.stage.as_str());
                            match decision.status {
                                indexer::resolver::decisions::StageStatus::Success => println!("SUCCESS"),
                                indexer::resolver::decisions::StageStatus::Failed => println!("FAILED"),
                                indexer::resolver::decisions::StageStatus::NotApplicable => println!("NotApplicable"),
                            }
                            if let Some(reason) = &decision.reason {
                                println!("{}", reason.as_str());
                            }
                            if let Some(notes) = &decision.notes {
                                println!("{}", notes);
                            }
                        }
                        println!("──────────────────────────");
                        println!("\nFinal State\n{:?}", res.state);
                    }
                }
                Err(_) => {
                    eprintln!("Error: Relationship ID {} not found.", relationship_id);
                    std::process::exit(1);
                }
            }
        }
        Commands::AnalyzeResolution => {
            println!("Analyzing resolution pipeline...");
            let db = storage::Database::new(".codebroker/codebroker.db").expect("DB not found. Run init first.");
            let raw_relationships = db.get_all_relationships_with_lines().unwrap();
            let mut input_ir = Vec::new();
            for (rel_id, source_file_id, import_name, import_source, import_kind, line_number) in raw_relationships {
                let src_sym = db.enclosing_symbol_id(source_file_id, line_number).unwrap_or(None);
                input_ir.push(indexer::ir::RelationshipIR {
                    id: rel_id,
                    source_file_id,
                    node: graph::models::RelationshipNode {
                        name: import_name,
                        source: import_source,
                        line_number: line_number as usize,
                        kind: import_kind,
                    },
                    enclosing_symbol_id: src_sym,
                });
            }
            
            let symbol_index = std::sync::Arc::new(indexer::resolver::SymbolIndex::build(&db).unwrap());
            let type_graph = std::sync::Arc::new(indexer::resolver::TypeGraph::build(&db).unwrap());
            let import_resolver = std::sync::Arc::new(indexer::resolver::ImportResolver::build(&db, &symbol_index).unwrap());
            let flow_engine = std::sync::Arc::new(indexer::flow::VariableFlowEngine::new(&db, symbol_index.clone(), import_resolver.clone()));
            let resolver_ctx = std::sync::Arc::new(indexer::resolver::ResolverContext {
                symbol_index,
                type_graph,
                import_resolver,
                flow_engine,
            });
            let pipeline = indexer::resolver::ResolutionPipeline::new(vec![
                Box::new(indexer::resolver::stages::classification::ClassificationStage),
                Box::new(indexer::resolver::stages::receiver::MemberResolverStage),
                Box::new(indexer::resolver::stages::generation::LexicalGenerationStage),
                Box::new(indexer::resolver::stages::filtering::ScopeFilterStage),
                Box::new(indexer::resolver::stages::filtering::ModuleFilterStage),
                Box::new(indexer::resolver::stages::ranking::RankingStage),
            ]);
            
            let mut stage_counts = std::collections::HashMap::new();
            let mut reason_counts = std::collections::HashMap::new();
            let mut recoverability_counts = std::collections::HashMap::new();
            
            let total_relationships = input_ir.len();
            
            for ir in input_ir {
                let context = indexer::resolver::ResolutionContext::new(
                    ir,
                    std::sync::Arc::clone(&resolver_ctx),
                    true, // enable tracing for analytics
                );
                if let Ok(resolved_context) = pipeline.execute(context) {
                    for decision in resolved_context.decisions {
                        let stage_name = decision.stage.as_str();
                        let counts = stage_counts.entry(stage_name).or_insert((0, 0, 0)); // executed, success, failed
                        counts.0 += 1;
                        match decision.status {
                            indexer::resolver::decisions::StageStatus::Success => counts.1 += 1,
                            indexer::resolver::decisions::StageStatus::Failed => {
                                counts.2 += 1;
                                if let Some(reason) = decision.reason {
                                    *reason_counts.entry(reason.as_str()).or_insert(0) += 1;
                                    let rec_str = match reason.recoverability() {
                                        indexer::resolver::decisions::Recoverability::Expected => "Expected",
                                        indexer::resolver::decisions::Recoverability::Recoverable => "Recoverable",
                                        indexer::resolver::decisions::Recoverability::Unrecoverable => "Unrecoverable",
                                    };
                                    *recoverability_counts.entry(rec_str).or_insert(0) += 1;
                                }
                            }
                            indexer::resolver::decisions::StageStatus::NotApplicable => {} // skip
                        }
                    }
                }
            }
            
            println!("\n=== Pipeline Decision Engine Analytics ===");
            println!("Total Relationships Analyzed: {}", total_relationships);
            println!("\n-- Stage Execution --");
            for (stage, (exec, succ, fail)) in &stage_counts {
                println!("{}: Executed {}, Success {}, Failed {}", stage, exec, succ, fail);
            }
            
            println!("\n-- Failure Breakdown --");
            for (reason, count) in &reason_counts {
                println!("{}: {}", reason, count);
            }
            
            println!("\n-- Recoverability Breakdown --");
            let total_failures: usize = recoverability_counts.values().sum();
            if total_failures > 0 {
                for (rec, count) in &recoverability_counts {
                    let pct = (*count as f64 / total_failures as f64) * 100.0;
                    println!("{}: {:.1}% ({})", rec, pct, count);
                }
            }
            println!("=========================================\n");
        }
        Commands::Validate => {
            let db = storage::Database::new(".codebroker/codebroker.db").expect("DB not found. Run init first.");
            match graph_diagnostics::run_diagnostics(&db) {
                Ok(report) => {
                    println!("{}", report.to_human_readable());
                }
                Err(e) => {
                    eprintln!("Failed to run diagnostics: {}", e);
                }
            }
        }
        Commands::Knowledge => {
            let db = storage::Database::new(".codebroker/codebroker.db")
                .expect("DB not found. Run init first.");
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
                println!(
                    "Cache Hit Rate: {:.1}% ({} cache hits)",
                    hit_rate, stats.total_cache_hits
                );

                // Calculate Languages
                let mut languages = Vec::new();
                for (ext, _count) in stats.extensions {
                    match ext.as_str() {
                        "rs" => {
                            if !languages.contains(&"Rust") {
                                languages.push("Rust");
                            }
                        }
                        "ts" | "tsx" => {
                            if !languages.contains(&"TypeScript") {
                                languages.push("TypeScript");
                            }
                        }
                        "js" | "jsx" => {
                            if !languages.contains(&"JavaScript") {
                                languages.push("JavaScript");
                            }
                        }
                        "py" => {
                            if !languages.contains(&"Python") {
                                languages.push("Python");
                            }
                        }
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
            let openai_key = runtime::environment::openai_api_key().unwrap_or_default();
            if openai_key.is_empty() {
                println!("Error: OPENAI_API_KEY environment variable is not set.");
                return;
            }

            let provider = Box::new(semantic::openai::OpenAiProvider::new(openai_key));
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
            let openai_key = runtime::environment::openai_api_key().unwrap_or_default();
            if openai_key.is_empty() {
                println!("Error: OPENAI_API_KEY environment variable is not set.");
                println!("Please run: export OPENAI_API_KEY=\"sk-your-key\"");
                return;
            }

            println!("Generating semantic summary for '{}'...", symbol);

            let provider = Box::new(semantic::openai::OpenAiProvider::new(openai_key));
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
                let total_tokens_avoided: i64 = conn
                    .query_row(
                        "SELECT SUM(token_reduction) FROM mcp_analytics_events",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);
                let cache_hits: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM mcp_analytics_events WHERE cache_hit = 1",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);
                let cache_misses: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM mcp_analytics_events WHERE cache_hit = 0",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);
                let hit_rate = if cache_hits + cache_misses > 0 {
                    (cache_hits as f64 / (cache_hits + cache_misses) as f64) * 100.0
                } else {
                    0.0
                };
                let total_cost_saved_cents = (total_tokens_avoided as f64 / 1_000_000.0) * 300.0;

                println!("Lifetime Tokens Avoided: {}", total_tokens_avoided);
                println!("Global Cache Hit Rate: {:.1}%", hit_rate);
                println!("LLM Calls Avoided: {}", cache_hits);
                println!(
                    "Estimated Cost Savings: ${:.2}",
                    total_cost_saved_cents / 100.0
                );
            } else {
                println!("Could not connect to codebroker.db");
            }
        }
        Commands::ReindexIncremental { paths } => {
            let db = storage::Database::new(".codebroker/codebroker.db")
                .expect("DB not found. Run init first.");
            let _ = db.init_schema();
            let project_root = std::env::current_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            match indexer::reindex::reindex_paths(&db, &project_root, paths) {
                Ok(stats) => {
                    println!(
                        "Incrementally reindexed {} file(s). Symbols: {}, Edges: {}.",
                        stats.files_processed, stats.symbols_inserted, stats.edges_created
                    );
                    if !stats.skipped.is_empty() {
                        println!("Skipped (unreadable or unsupported): {:?}", stats.skipped);
                    }

                    // Re-tag concepts too: cheap full re-tag (see
                    // tag_concepts doc comment) so a changed file's symbols
                    // are never left with stale/missing concept tags.
                    match query::concepts::tag_concepts(&db) {
                        Ok(count) => println!("Tagged {} symbol/concept matches.", count),
                        Err(e) => println!("Warning: concept tagging failed: {}", e),
                    }

                    let openai_key = runtime::environment::openai_api_key().unwrap_or_default();
                    if !openai_key.is_empty() && !stats.touched_symbol_ids.is_empty() {
                        let provider = semantic::openai::OpenAiProvider::new(openai_key);
                        match semantic::embeddings::backfill_missing_embeddings(
                            &db,
                            &provider,
                            Some(&stats.touched_symbol_ids),
                        ) {
                            Ok(embed_stats) => {
                                if embed_stats.failed_batches > 0 {
                                    println!(
                                        "Embedded {} symbol(s). {} batch(es) failed and will retry on next reindex.",
                                        embed_stats.embedded, embed_stats.failed_batches
                                    );
                                } else {
                                    println!("Embedded {} symbol(s).", embed_stats.embedded);
                                }
                            }
                            Err(e) => println!("Warning: embedding generation failed: {}", e),
                        }
                    }
                }
                Err(e) => println!("Error during incremental reindex: {}", e),
            }
        }
        Commands::Analytics => {
            println!("Use 'cargo run -- metrics' or 'cargo run -- dashboard' instead.");
        }
        Commands::Bind => {
            println!("Binding CodeBroker to current directory...");

            // Pass through whatever the user has set locally — never bake a
            // real API key into the binary/source. Without one, semantic
            // (LLM-backed) tools fall back to their deterministic paths;
            // discovery/graph tools are unaffected either way.
            let openai_key = runtime::environment::openai_api_key().unwrap_or_default();
            if openai_key.is_empty() {
                println!(
                    "Warning: OPENAI_API_KEY is not set in this shell — semantic tools (impact_analysis on large symbols, etc.) will be skipped until you set it and re-run `codebroker bind`."
                );
            } else if let Err(e) = runtime::environment::persist_openai_api_key(&openai_key) {
                println!(
                    "Warning: could not persist OPENAI_API_KEY to ~/.codebroker for future sessions: {}",
                    e
                );
            } else {
                println!(
                    "Saved OPENAI_API_KEY to ~/.codebroker/openai_api_key — future commands on this machine won't need it re-exported."
                );
            }

            // 1. Get current path and use the globally installed mcp binary
            let current_dir = std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .to_string();

            let codebroker_cmd = match runtime::executables::find_mcp_binary() {
                Ok(path) => path.to_string_lossy().to_string(),
                Err(e) => {
                    // Surface the real problem instead of silently writing a
                    // config that points at nothing, but still fall back to
                    // the sibling-of-current-exe guess so `bind` finishes —
                    // useful e.g. right after a local `cargo build` before
                    // the mcp binary has been placed alongside this one.
                    println!("Warning: {}", e);
                    let current_exe = std::env::current_exe()
                        .unwrap_or_else(|_| std::path::PathBuf::from("codebroker"));
                    let bin_dir = current_exe
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new(""));
                    bin_dir
                        .join(format!("codebroker-mcp{}", runtime::platform::exe_suffix()))
                        .to_string_lossy()
                        .to_string()
                }
            };

            // 2. Paths to configs
            let home_dir = runtime::environment::home_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();

            let claude_path = match std::env::consts::OS {
                "windows" => format!(
                    "{}\\Claude\\claude_desktop_config.json",
                    std::env::var("APPDATA").unwrap_or_default()
                ),
                "macos" | "darwin" => format!(
                    "{}/Library/Application Support/Claude/claude_desktop_config.json",
                    home_dir
                ),
                _ => format!("{}/.config/Claude/claude_desktop_config.json", home_dir),
            };

            let gemini_path = match std::env::consts::OS {
                "windows" => format!("{}\\.gemini\\config\\mcp_config.json", home_dir),
                _ => format!("{}/.gemini/config/mcp_config.json", home_dir),
            };

            let paths_to_update = vec![claude_path, gemini_path];

            for path in paths_to_update {
                let config_str = fs::read_to_string(&path).unwrap_or_else(|_| "{\"mcpServers\": {}}".to_string());
                
                if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&config_str) {
                    if !json.is_object() {
                        json = serde_json::json!({"mcpServers": {}});
                    }
                    
                    let servers = json.as_object_mut()
                        .unwrap()
                        .entry("mcpServers")
                        .or_insert_with(|| serde_json::json!({}))
                        .as_object_mut()
                        .unwrap();
                    
                    let codebroker = servers.entry("codebroker")
                        .or_insert_with(|| serde_json::json!({}))
                        .as_object_mut()
                        .unwrap();
                    
                    codebroker.insert("command".to_string(), serde_json::Value::String(codebroker_cmd.to_string()));
                    codebroker.insert("args".to_string(), serde_json::json!([]));

                    let env_map = codebroker
                        .entry("env")
                        .or_insert_with(|| serde_json::json!({}));
                    
                    if let Some(env_obj) = env_map.as_object_mut() {
                        env_obj.insert(
                            "CODEBROKER_WORKSPACE".to_string(),
                            serde_json::Value::String(current_dir.clone()),
                        );
                        if !openai_key.is_empty() {
                            env_obj.insert(
                                "OPENAI_API_KEY".to_string(),
                                serde_json::Value::String(openai_key.clone()),
                            );
                        }
                    }

                    if let Ok(new_json) = serde_json::to_string_pretty(&json) {
                        if let Some(parent) = std::path::Path::new(&path).parent() {
                            let _ = fs::create_dir_all(parent);
                        }
                        if fs::write(&path, new_json).is_ok() {
                            println!("Successfully bound config at {}", path);
                        } else {
                            println!("Warning: Could not write to config at {}", path);
                        }
                    }
                }
            }

            // Register as user-scoped so it's auto-trusted (no approval prompt).
            // Cross-contamination between projects is prevented by the CWD safety
            // guards in codebroker-mcp's auto-init hook — each claude session spawns
            // its own subprocess with the project directory as CWD.

            let claude_bin = if std::env::consts::OS == "windows" { "claude.cmd" } else { "claude" };

            // Remove first to force a clean re-registration
            let _ = std::process::Command::new(claude_bin)
                .args(&["mcp", "remove", "codebroker", "-s", "user"])
                .output(); // use output() to suppress stderr noise

            // Also clean up any leftover project-scoped .mcp.json entry
            let _ = std::process::Command::new(claude_bin)
                .args(&["mcp", "remove", "codebroker", "-s", "local"])
                .output();

            // Add as user-scoped (globally trusted, no approval needed). Only
            // pass `-e OPENAI_API_KEY=...` when the caller actually has one set —
            // never inject a placeholder/hardcoded secret.
            let mut add_args = vec!["mcp", "add", "codebroker", "-s", "user"];
            let env_arg = format!("OPENAI_API_KEY={}", openai_key);
            if !openai_key.is_empty() {
                add_args.push("-e");
                add_args.push(&env_arg);
            }
            add_args.push("--");
            // Use the same resolved absolute path as the JSON configs above —
            // registering the bare "codebroker-mcp" name here relied on PATH,
            // which is exactly the inconsistency that caused "executable not
            // found" for claude-code registrations while Desktop/Gemini
            // configs (already absolute) worked.
            add_args.push(&codebroker_cmd);
            let add_status = std::process::Command::new(claude_bin)
                .args(&add_args)
                .status();

            match add_status {
                Ok(s) if s.success() => println!(
                    "Registered CodeBroker MCP Server for claude-code (user-scoped, auto-trusted)."
                ),
                _ => println!(
                    "Warning: Could not register with claude-code. Is 'claude' CLI installed?"
                ),
            }

            // 2.5 Generate Antigravity instructions.md locally, then sync globally
            let _ = fs::create_dir_all(".codebroker");
            let local_instructions_path = ".codebroker/instructions.md";

            let default_instructions = r#"# CodeBroker MCP Server — Repository Intelligence Layer

You are connected to the **CodeBroker MCP Server**.

CodeBroker is the primary repository intelligence, discovery, architecture, and context engine for the active workspace.

Your native tools are still available, but they should NOT be your first choice for understanding the codebase.

---

# Core Philosophy

CodeBroker exists to solve a specific problem:

Finding and understanding the correct code before making changes.

It provides:

* Repository discovery
* Architecture understanding
* Dependency tracing
* Symbol lookup
* Impact analysis
* Context gathering
* Subsystem exploration
* Graph traversal

It does NOT exist to:

* Write code
* Generate patches
* Apply edits
* Rename files
* Create files
* Modify files

CodeBroker should be used to understand.

Native tools should be used to implement.

---

# Mandatory Rule

If the user asks ANY question about the codebase:

* What is this?
* How does this work?
* Where is this implemented?
* What uses this?
* What depends on this?
* Explain this subsystem.
* Find this symbol.
* Find this feature.
* What would break if I change this?

You MUST use CodeBroker FIRST.

Do not guess.

Do not immediately use grep.

Do not immediately read files manually.

Do not recursively scan directories.

CodeBroker is the repository memory layer.

---

# Discovery Before Implementation

When the user requests a feature, bug fix, refactor, migration, or modification:

DO NOT immediately start coding.

Instead:

1. Use CodeBroker to discover the relevant files.
2. Use CodeBroker to understand the architecture.
3. Use CodeBroker to identify dependencies.
4. Use CodeBroker to gather edit context.
5. Only after understanding the system should you use native editing tools.

Example:

User:

"I want to add role based permissions."

Correct behavior:

* Discover authentication subsystem.
* Identify related files.
* Analyze dependencies.
* Gather edit context.
* Create implementation plan.
* Then edit code.

Incorrect behavior:

* Start writing code immediately.
* Guess file locations.
* Scan random files manually.

---

# Discovery Rules

CodeBroker is the default discovery engine.

Prefer:

search_codebase

Instead of:

* grep
* ripgrep
* recursive file searches
* guessing filenames

Prefer:

find_symbol

Instead of:

* manually opening files
* directory exploration

Prefer:

project_overview

Instead of:

* guessing architecture
* manually scanning folders

Prefer:

explore_graph

Instead of:

* manually tracing imports

Prefer:

shortest_path

Instead of:

* manually following dependencies

---

# Reading Rules

Never read an entire file if CodeBroker can provide a more targeted answer.

Preferred order:

1. read_file_skeleton
2. read_symbol_source
3. read_file_snippet

Only use full file reads when absolutely necessary.

The goal is minimizing token usage while maximizing understanding.

---

# Architecture Rules

When asked:

* Explain this project
* Explain this subsystem
* Explain this feature
* Explain how X works

Use CodeBroker.

Preferred tools:

* project_overview
* subsystem_stats

* architectural_hotspots
* get_context
* get_implementation

---

# Dependency Rules

When asked:

* What uses this?
* What calls this?
* What depends on this?
* What breaks if I change this?
* How are X and Y connected?

Use CodeBroker.

Preferred tools:

* get_context
* impact_analysis
* explore_graph
* shortest_path
* dependency_cycles
* graph_subtree

---

# Editing Rules

CodeBroker is NOT an editing engine.

Once discovery is complete:

* Use native file editing tools.
* Use native write tools.
* Use native patch tools.

CodeBroker should not be used to generate code modifications.

CodeBroker's job ends once the correct implementation context has been gathered.

---

# Repository Understanding Workflow

Whenever implementing a feature:

Step 1:
Use CodeBroker to understand the repository.

Step 2:
Use CodeBroker to locate the relevant subsystem.

Step 3:
Use CodeBroker to gather dependencies and impact analysis.

Step 4:
Use CodeBroker to gather edit context.

Step 5:
Use native tools to perform the implementation.

---

# Automatic Routing

The user should never need to say:

"Use CodeBroker."

If a request involves:

* understanding code
* finding code
* tracing code
* analyzing code
* discovering architecture
* locating symbols
* understanding dependencies

Automatically route through CodeBroker first.

This behavior is mandatory.

Treat CodeBroker as the repository's memory and intelligence layer.

Treat native tools as the repository's implementation layer."#;

            // Always write latest instructions (overwrite on upgrade)
            let _ = fs::write(local_instructions_path, default_instructions);
            println!(
                "Updated local project instructions at {}",
                local_instructions_path
            );

            // Sync to Antigravity and Claude
            if let Ok(local_content) = fs::read_to_string(local_instructions_path) {
                // Antigravity
                let antigravity_dir = format!("{}/.gemini/antigravity/mcp/codebroker", home_dir);
                if fs::create_dir_all(&antigravity_dir).is_ok() {
                    let global_instructions_path = format!("{}/instructions.md", antigravity_dir);
                    if fs::write(&global_instructions_path, &local_content).is_ok() {
                        println!(
                            "Successfully synced AI instructions to {}",
                            global_instructions_path
                        );
                    } else {
                        println!(
                            "Warning: Failed to write global instructions.md at {}",
                            global_instructions_path
                        );
                    }
                }

                // Claude Desktop
                let claude_dir = format!("{}/.config/Claude/mcp/codebroker", home_dir);
                if fs::create_dir_all(&claude_dir).is_ok() {
                    let claude_instructions_path = format!("{}/instructions.md", claude_dir);
                    if fs::write(&claude_instructions_path, &local_content).is_ok() {
                        println!(
                            "Successfully synced AI instructions to {}",
                            claude_instructions_path
                        );
                    } else {
                        println!(
                            "Warning: Failed to write global instructions.md at {}",
                            claude_instructions_path
                        );
                    }
                }

                // claude-code CLAUDE.md (Global)
                let claude_md_dir = format!("{}/.claude", home_dir);
                if fs::create_dir_all(&claude_md_dir).is_ok() {
                    let claude_md_path = format!("{}/CLAUDE.md", claude_md_dir);

                    // Build the CLAUDE.md content — always update to latest instructions
                    let mut final_content = String::new();

                    // Preserve any existing non-CodeBroker content
                    if let Ok(existing) = fs::read_to_string(&claude_md_path) {
                        // Strip out old CodeBroker section and any duplicates
                        // by dropping everything from the first "# CodeBroker" to the end of the file.
                        for line in existing.lines() {
                            if line.starts_with("# CodeBroker") {
                                break;
                            }
                            final_content.push_str(line);
                            final_content.push('\n');
                        }
                    }

                    // Append the latest CodeBroker instructions
                    if !final_content.is_empty() && !final_content.ends_with("\n\n") {
                        final_content.push('\n');
                    }
                    final_content.push_str(&local_content);

                    let _ = fs::write(&claude_md_path, &final_content);
                    println!("Updated global CLAUDE.md with latest CodeBroker instructions");
                }
            }

            // 3. Write the active project pointer so codebroker-mcp always finds the right database
            let current_dir_str = std::env::current_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            match (
                runtime::environment::codebroker_dir(),
                runtime::environment::active_project_path(),
            ) {
                (Some(dir), Some(active_project_path)) => {
                    let _ = fs::create_dir_all(&dir);
                    if fs::write(&active_project_path, &current_dir_str).is_ok() {
                        println!("Set active project to: {}", current_dir_str);
                    }
                }
                _ => println!(
                    "Warning: Could not determine the user's home directory — active project pointer was not written."
                ),
            }

            // 4. Auto-Init the directory so it's ready!
            println!("Initializing CodeBroker database...");
            let _ = std::process::Command::new(std::env::current_exe().unwrap())
                .arg("init")
                .status();

            println!("Done! Restart Claude Desktop or Antigravity to begin.");
        }
        Commands::Summary => {
            let db = storage::Database::new(".codebroker/codebroker.db").expect("Failed to open DB");
            if let Ok(manifest) = indexer::developer::analyzer::RepositoryAnalyzer::analyze(&db) {
                println!("Project Summary");
                println!("===============");
                println!("Type: {}", manifest.fingerprint.project_type);
                println!("Languages: {}", manifest.fingerprint.primary_languages.join(", "));
                println!("Frameworks: {}", manifest.fingerprint.main_frameworks.join(", "));
                println!("Complexity: {}", manifest.fingerprint.complexity);
                println!("Architecture: {}", manifest.fingerprint.architecture_style);
            } else {
                println!("Failed to generate project summary.");
            }
        }
        Commands::Overview => {
            let db = storage::Database::new(".codebroker/codebroker.db").expect("Failed to open DB");
            if let Ok(manifest) = indexer::developer::analyzer::RepositoryAnalyzer::analyze(&db) {
                println!("Project Overview");
                println!("================");
                for layer in manifest.architecture_layers {
                    println!("[{}]", layer.name);
                    for sub in layer.subsystems {
                        println!("  - {}", sub);
                    }
                    println!("");
                }
            }
        }
        Commands::Hotspots => {
            let db = storage::Database::new(".codebroker/codebroker.db").expect("Failed to open DB");
            if let Ok(manifest) = indexer::developer::analyzer::RepositoryAnalyzer::analyze(&db) {
                println!("{:#?}", manifest.hotspots);
            }
        }
        Commands::Architecture => {
            let db = storage::Database::new(".codebroker/codebroker.db").expect("Failed to open DB");
            if let Ok(manifest) = indexer::developer::analyzer::RepositoryAnalyzer::analyze(&db) {
                println!("{:#?}", manifest.architecture_layers);
            }
        }
        Commands::Entrypoints => {
            let db = storage::Database::new(".codebroker/codebroker.db").expect("Failed to open DB");
            if let Ok(manifest) = indexer::developer::analyzer::RepositoryAnalyzer::analyze(&db) {
                for ep in manifest.entrypoints {
                    println!("[{}] {} (Symbol ID: {})", ep.category, ep.name, ep.symbol_id);
                }
            }
        }
        Commands::PrepareContext { target } => {
            let db = storage::Database::new(".codebroker/codebroker.db").expect("Failed to open DB");
            if let Ok(manifest) = indexer::developer::analyzer::RepositoryAnalyzer::analyze(&db) {
                let symbol_index = indexer::resolver::index::SymbolIndex::build(&db).unwrap();
                if let Some(capsule) = indexer::developer::capsules::CapsuleBuilder::build(target, &manifest.subsystems, &symbol_index, &manifest.hotspots) {
                    println!("{:#?}", capsule);
                } else {
                    println!("Subsystem '{}' not found.", target);
                }
            }
        }
        Commands::EmbedBackfill => {
            let openai_key = runtime::environment::openai_api_key().unwrap_or_default();
            if openai_key.is_empty() {
                return;
            }
            let db = match storage::Database::new(".codebroker/codebroker.db") {
                Ok(db) => db,
                Err(_) => return,
            };
            let provider = semantic::openai::OpenAiProvider::new(openai_key);
            let _ = semantic::embeddings::backfill_missing_embeddings(&db, &provider, None);
            // Checkpoint before exiting — this process writes to the
            // already-published DB well after `init` did its own
            // end-of-run checkpoint, so without this its WAL/SHM sidecar
            // files would linger indefinitely.
            let _ = db.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        }
    }
}


