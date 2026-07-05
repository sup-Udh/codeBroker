use crate::prompt::build_prompt;
use crate::provider::LlmProvider;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use storage::Database;
pub struct SummaryGenerator<'a> {
    db: &'a Database,
    provider: Box<dyn LlmProvider>,
}

impl<'a> SummaryGenerator<'a> {
    pub fn new(db: &'a Database, provider: Box<dyn LlmProvider>) -> Self {
        Self { db, provider }
    }

    pub fn generate(&self, symbol_name: &str) -> Result<(String, bool), String> {
        let mut stmt = self
            .db
            .conn
            .prepare("SELECT id FROM symbols WHERE name = ?1 LIMIT 1")
            .map_err(|e| e.to_string())?;
        let symbol_id = stmt.query_row([symbol_name], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|_| format!("Symbol '{}' not found in database.", symbol_name))?;
        
        self.generate_by_id(symbol_id, symbol_name)
    }

    pub fn generate_by_id(
        &self,
        symbol_id: i64,
        symbol_name: &str,
    ) -> Result<(String, bool), String> {
        // 1. Get the file path
        let file_path: String = self
            .db
            .conn
            .query_row(
                "SELECT files.path FROM symbols JOIN files ON symbols.file_id = files.id WHERE symbols.id = ?1",
                [symbol_id],
                |row| row.get(0)
            )
            .map_err(|e| e.to_string())?;

        // BUG FIX: this used to read `file_path` (the DB-relative path, e.g.
        // "./api/login/route.ts") directly with no project-root resolution.
        // That only worked by accident when the MCP process's CWD happened to
        // equal the project root; whenever it didn't, the read silently
        // failed and `unwrap_or_default()` fed an EMPTY string into the LLM
        // prompt — producing a confident-looking but completely fabricated
        // summary while burning the full token cost of a real API call for
        // zero useful signal. Must resolve to an absolute path first.
        let abs_file_path = self.db.resolve_path(&file_path);
        let source_code = fs::read_to_string(&abs_file_path).unwrap_or_default();

        // 2. Assemble the ContextResponseBuilder
        let builder = query::context::ContextResponseBuilder::new_by_id(
            self.db,
            symbol_id,
            query::response::ResponseProfile::Verbose,
        )
        .map_err(|e| e.to_string())?
        .ok_or("Failed to assemble context")?;
        let graph_indexed = builder.graph_indexed;

        // 3.5. Fetch config files
        let mut config_text = String::new();
        if let Ok(mut cfg_stmt) = self.db.conn.prepare("SELECT path FROM files WHERE path LIKE '%package.json' OR path LIKE '%Dockerfile' OR path LIKE '%Cargo.toml' OR path LIKE '%tsconfig.json' OR path LIKE '%pyproject.toml' OR path LIKE '%requirements.txt' OR path LIKE '%docker-compose.yml'") {
            if let Ok(cfg_iter) = cfg_stmt.query_map([], |row| row.get::<_, String>(0)) {
                for cfg_path in cfg_iter.flatten() {
                    // Same fix as above: resolve before reading.
                    let abs_cfg_path = self.db.resolve_path(&cfg_path);
                    if let Ok(content) = fs::read_to_string(&abs_cfg_path) {
                        config_text.push_str(&format!("--- {} ---\n{}\n\n", cfg_path, content));
                    }
                }
            }
        }

        let source_hash = calculate_hash(&source_code);
        let context_json =
            serde_json::to_string(&builder.build_json().unwrap_or_default()).unwrap_or_default();
        let context_hash = calculate_hash(&context_json);
        let model_name = self.provider.model_name();

        // Without graph edges, the LLM is reading only this one symbol's source
        // and guessing about callers/dependents from local context alone. Make
        // that explicit so the result isn't mistaken for a real graph traversal.
        const UNINDEXED_DISCLAIMER: &str = "⚠️ GRAPH NOT INDEXED: This repository's dependency graph has 0 edges. The analysis below is a source-only guess based on reading this symbol's code in isolation — it is NOT based on real caller/dependent traversal. Run `reindex_workspace` to build the graph before trusting blast-radius claims.\n\n";

        // 5. Check Cache
        if let Ok(Some(cached_summary)) =
            self.db
                .get_cached_summary(symbol_id, &source_hash, &context_hash, model_name)
        {
            let prefix = if graph_indexed {
                ""
            } else {
                UNINDEXED_DISCLAIMER
            };
            return Ok((format!("(Cached)\n{}{}", prefix, cached_summary), true));
        }

        // 6. Build Prompt & Call AI (with latency tracking!)
        let prompt = build_prompt(symbol_name, &source_code, &context_json, &config_text);

        let start_time = std::time::Instant::now();
        let (summary, token_count) = self.provider.generate_summary(&prompt, 30, 2048)?;
        let elapsed_ms = start_time.elapsed().as_millis();

        // 7. Save to Cache with all our rich metadata
        let _ = self.db.save_semantic_summary(
            symbol_id,
            &summary,
            &source_hash,
            &context_hash,
            model_name,
            token_count,
            elapsed_ms,
        );

        let final_summary = if graph_indexed {
            summary
        } else {
            format!("{}{}", UNINDEXED_DISCLAIMER, summary)
        };

        Ok((final_summary, false))
    }
}

fn calculate_hash(data: &str) -> String {
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

pub struct ProjectOverviewGenerator<'a> {
    db: &'a Database,
    provider: Box<dyn LlmProvider>,
}

impl<'a> ProjectOverviewGenerator<'a> {
    pub fn new(db: &'a Database, provider: Box<dyn LlmProvider>) -> Self {
        Self { db, provider }
    }

    pub fn generate(&self) -> Result<(String, bool), String> {
        let repo_hash = self
            .db
            .get_repository_topology_hash()
            .map_err(|e| e.to_string())?;
        let model_name = self.provider.model_name();

        if let Ok(Some(cached_overview)) = self
            .db
            .get_cached_repository_overview(&repo_hash, model_name)
        {
            return Ok((format!("(Cached Overview)\n{}", cached_overview), true));
        }

        let raw_overview =
            query::engine::build_project_overview(self.db).map_err(|e| e.to_string())?;

        let overview_json = serde_json::to_string_pretty(&raw_overview).unwrap_or_default();

        // Ground the narrative in real graph signal instead of just raw
        // directory/file counts — previously the prompt only saw "app/api has
        // 50 files and 200 symbols" with no sense of which symbols actually
        // anchor the architecture, so the model fell back to generic
        // boilerplate ("showcases a well-structured web application...")
        // that added nothing beyond what the counts already implied
        // (benchmark run_001's finding on `project_overview_ai`). Hotspots
        // and entrypoints are exactly the two signals a human architect would
        // actually look at first.
        let hotspots = query::graph::architectural_hotspots(self.db, 10, None)
            .map(|h| h.top_hotspots)
            .unwrap_or_default();
        let hotspot_summary = hotspots
            .iter()
            .map(|h| {
                format!(
                    "- {} ({}) in {} — {} incoming / {} outgoing edges [{}]",
                    h.name,
                    h.kind,
                    h.file_path,
                    h.incoming_edges,
                    h.outgoing_edges,
                    h.classification
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let entrypoints = query::subsystem::list_entrypoints(self.db, None).unwrap_or_default();
        let entrypoint_summary = {
            let routes = entrypoints
                .routes
                .iter()
                .take(15)
                .map(|e| format!("- {} ({}) in {}", e.name, e.kind, e.file_path))
                .collect::<Vec<_>>()
                .join("\n");
            let pages = entrypoints
                .pages
                .iter()
                .take(15)
                .map(|e| format!("- {} ({}) in {}", e.name, e.kind, e.file_path))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "Routes/API endpoints ({} total):\n{}\n\nPage/layout entrypoints ({} total):\n{}",
                entrypoints.routes.len(),
                if routes.is_empty() {
                    "(none indexed)"
                } else {
                    &routes
                },
                entrypoints.pages.len(),
                if pages.is_empty() {
                    "(none indexed)"
                } else {
                    &pages
                }
            )
        };

        let prompt = format!(
            "You are a Principal Systems Architect. Analyze the following raw topological metrics, architectural hotspots, and entrypoints for this repository, and generate a highly professional architectural overview mapping out what this project likely does and what its major subsystems are responsible for. Ground every subsystem claim in the hotspots and entrypoints below — do not infer subsystem boundaries from directory/file counts alone. Do not list file paths explicitly, summarize their conceptual role.\n\nRaw Topology:\n{}\n\nArchitectural Hotspots (highest in/out-degree symbols):\n{}\n\nEntrypoints:\n{}",
            overview_json, hotspot_summary, entrypoint_summary
        );

        let (summary, _token_count) = self.provider.generate_summary(&prompt, 60, 4096)?;

        let _ = self
            .db
            .save_repository_overview(&repo_hash, model_name, &summary);

        Ok((summary, false))
    }
}
