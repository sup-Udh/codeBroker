use crate::provider::LlmProvider;
use crate::prompt::{build_prompt, build_patch_prompt};
use storage::Database;
use query::context::ContextObject;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::time::Instant;
pub struct SummaryGenerator<'a> {
    db: &'a Database,
    provider: Box<dyn LlmProvider>,
}

impl<'a> SummaryGenerator<'a> {
    pub fn new(db: &'a Database, provider: Box<dyn LlmProvider>) -> Self {
        Self { db, provider }
    }

    pub fn generate(&self, symbol_name: &str) -> Result<(String, bool), String> {
        // 1. Get the symbol from the database
        let mut stmt = self.db.conn.prepare("SELECT id, file_id FROM symbols WHERE name = ?1 LIMIT 1")
            .map_err(|e| e.to_string())?;
        
        let (symbol_id, file_id) = stmt.query_row([symbol_name], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        }).map_err(|_| format!("Symbol '{}' not found in database.", symbol_name))?;

        // 2. Get the file path and read the source code
        let file_path: String = self.db.conn.query_row(
            "SELECT path FROM files WHERE id = ?1",
            [file_id],
            |row| row.get(0)
        ).map_err(|e| e.to_string())?;
        
        let source_code = fs::read_to_string(&file_path).unwrap_or_default();

        // 3. Assemble the ContextObject
        let context = ContextObject::assemble(self.db, symbol_name)
            .map_err(|e| e.to_string())?
            .ok_or("Failed to assemble context")?;
        let graph_indexed = context.graph_indexed;

        // 3.5. Fetch config files
        let mut config_text = String::new();
        if let Ok(mut cfg_stmt) = self.db.conn.prepare("SELECT path FROM files WHERE path LIKE '%package.json' OR path LIKE '%Dockerfile' OR path LIKE '%Cargo.toml' OR path LIKE '%tsconfig.json' OR path LIKE '%pyproject.toml' OR path LIKE '%requirements.txt' OR path LIKE '%docker-compose.yml'") {
            if let Ok(cfg_iter) = cfg_stmt.query_map([], |row| row.get::<_, String>(0)) {
                for cfg_path in cfg_iter.flatten() {
                    if let Ok(content) = fs::read_to_string(&cfg_path) {
                        config_text.push_str(&format!("--- {} ---\n{}\n\n", cfg_path, content));
                    }
                }
            }
        }

        // 4. Calculate Hashes
        let source_hash = calculate_hash(&source_code);
        let context_json = serde_json::to_string(&context).unwrap_or_default();
        let context_hash = calculate_hash(&context_json);
        let model_name = self.provider.model_name();

        // Without graph edges, the LLM is reading only this one symbol's source
        // and guessing about callers/dependents from local context alone. Make
        // that explicit so the result isn't mistaken for a real graph traversal.
        const UNINDEXED_DISCLAIMER: &str = "⚠️ GRAPH NOT INDEXED: This repository's dependency graph has 0 edges. The analysis below is a source-only guess based on reading this symbol's code in isolation — it is NOT based on real caller/dependent traversal. Run `reindex_workspace` to build the graph before trusting blast-radius claims.\n\n";

        // 5. Check Cache
        if let Ok(Some(cached_summary)) = self.db.get_cached_summary(symbol_id, &source_hash, &context_hash, model_name) {
            let prefix = if graph_indexed { "" } else { UNINDEXED_DISCLAIMER };
            return Ok((format!("(Cached)\n{}{}", prefix, cached_summary), true));
        }

        // 6. Build Prompt & Call AI (with latency tracking!)
        let prompt = build_prompt(symbol_name, &source_code, &context, &config_text);

        let start_time = std::time::Instant::now();
        let (summary, token_count) = self.provider.generate_summary(&prompt, 20)?;
        let elapsed_ms = start_time.elapsed().as_millis();

        // 7. Save to Cache with all our rich metadata
        let _ = self.db.save_semantic_summary(
            symbol_id,
            &summary,
            &source_hash,
            &context_hash,
            model_name,
            token_count,
            elapsed_ms
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
        let repo_hash = self.db.get_repository_topology_hash().map_err(|e| e.to_string())?;
        let model_name = self.provider.model_name();

        if let Ok(Some(cached_overview)) = self.db.get_cached_repository_overview(&repo_hash, model_name) {
            return Ok((format!("(Cached Overview)\n{}", cached_overview), true));
        }

        let raw_overview = query::engine::build_project_overview(self.db).map_err(|e| e.to_string())?;
        
        let overview_json = serde_json::to_string_pretty(&raw_overview).unwrap_or_default();
        
        let prompt = format!(
            "You are a Principal Systems Architect. Analyze the following raw topological metrics and subsystem distribution for this repository, and generate a highly professional architectural overview mapping out what this project likely does and what its major subsystems are responsible for. Do not list file paths explicitly, summarize their conceptual role.\n\nRaw Data:\n{}",
            overview_json
        );

        let (summary, _token_count) = self.provider.generate_summary(&prompt, 45)?;

        let _ = self.db.save_repository_overview(&repo_hash, model_name, &summary);

        Ok((summary, false))
    }
}

pub struct PatchGenerator<'a> {
    db: &'a Database,
    provider: Box<dyn LlmProvider>,
}

impl<'a> PatchGenerator<'a> {
    pub fn new(db: &'a Database, provider: Box<dyn LlmProvider>) -> Self {
        Self { db, provider }
    }

    pub fn generate_patch(&self, symbol_name: &str, instruction: &str) -> Result<String, String> {
        let mut stmt = self.db.conn.prepare("SELECT file_id, start_byte, end_byte FROM symbols WHERE name = ?1 LIMIT 1")
            .map_err(|e| e.to_string())?;
        
        let (file_id, start_byte, end_byte) = stmt.query_row([symbol_name], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
        }).map_err(|_| format!("Symbol '{}' not found in database.", symbol_name))?;

        let file_path: String = self.db.conn.query_row(
            "SELECT path FROM files WHERE id = ?1",
            [file_id],
            |row| row.get(0)
        ).map_err(|e| e.to_string())?;
        
        let content = fs::read(&file_path).map_err(|e| e.to_string())?;
        let mut source_code = String::new();
        
        let start = start_byte as usize;
        let end = end_byte as usize;
        if start < end && end <= content.len() {
            source_code = String::from_utf8_lossy(&content[start..end]).to_string();
        } else {
            source_code = query::retrieval::read_symbol_source(self.db, symbol_name, false)?
                .into_iter()
                .next()
                .map(|r| r.source)
                .unwrap_or_default();
        }

        let context = ContextObject::assemble(self.db, symbol_name)
            .map_err(|e| e.to_string())?
            .ok_or("Failed to assemble context")?;

        let prompt = build_patch_prompt(symbol_name, &source_code, &context, instruction);
        let (patch, _) = self.provider.generate_summary(&prompt, 20)?;

        Ok(patch)
    }
}
