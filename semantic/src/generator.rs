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
        self.generate_scoped(symbol_name, None)
    }

    /// Like `generate`, but when `file_hint` is given, only resolves a symbol
    /// defined in a file whose path contains that substring.
    pub fn generate_scoped(&self, symbol_name: &str, file_hint: Option<&str>) -> Result<(String, bool), String> {
        // 1. Get the symbol from the database
        let (symbol_id, file_id) = if let Some(hint) = file_hint {
            let mut stmt = self.db.conn.prepare(
                "SELECT symbols.id, symbols.file_id FROM symbols JOIN files ON symbols.file_id = files.id WHERE symbols.name = ?1 AND files.path LIKE ?2 LIMIT 1"
            ).map_err(|e| e.to_string())?;
            let pattern = format!("%{}%", hint);
            stmt.query_row(rusqlite::params![symbol_name, pattern], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            }).map_err(|_| format!("Symbol '{}' not found in a file matching '{}'.", symbol_name, hint))?
        } else {
            let mut stmt = self.db.conn.prepare("SELECT id, file_id FROM symbols WHERE name = ?1 LIMIT 1")
                .map_err(|e| e.to_string())?;
            stmt.query_row([symbol_name], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            }).map_err(|_| format!("Symbol '{}' not found in database.", symbol_name))?
        };

        // 2. Get the file path and read the source code
        let file_path: String = self.db.conn.query_row(
            "SELECT path FROM files WHERE id = ?1",
            [file_id],
            |row| row.get(0)
        ).map_err(|e| e.to_string())?;

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

        // 3. Assemble the ContextObject
        let context = ContextObject::assemble_scoped(self.db, symbol_name, file_hint)
            .map_err(|e| e.to_string())?
            .ok_or("Failed to assemble context")?;
        let graph_indexed = context.graph_indexed;

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

/// Result of a single generate_patch call. `introduced_identifiers` is a
/// best-effort grounding check, not a guarantee: any identifier in the
/// diff's added lines that doesn't appear anywhere in the target file or its
/// known graph context (siblings/dependencies/callees/imports) gets flagged
/// here. A flagged name is either a deliberately new identifier the
/// instruction asked for, or a hallucinated reference to something that
/// doesn't exist — the caller must check which before trusting the patch.
pub struct PatchOutput {
    pub diff: String,
    pub introduced_identifiers: Vec<String>,
}

pub struct PatchGenerator<'a> {
    db: &'a Database,
    provider: Box<dyn LlmProvider>,
}

/// Identifier-like tokens common enough across JS/TS/Python/Rust that
/// flagging them as "introduced" would be pure noise.
const IDENTIFIER_STOPLIST: &[&str] = &[
    "const", "let", "var", "function", "return", "if", "else", "for", "while", "import", "export",
    "default", "class", "interface", "type", "async", "await", "try", "catch", "finally", "throw",
    "new", "this", "super", "extends", "implements", "public", "private", "protected", "static",
    "void", "null", "undefined", "true", "false", "from", "as", "of", "in", "do", "switch", "case",
    "break", "continue", "typeof", "instanceof", "delete", "yield", "string", "number", "boolean",
    "String", "Number", "Boolean", "Array", "Object", "Promise", "Error", "Map", "Set", "console",
    "def", "self", "None", "True", "False", "lambda", "with", "global", "nonlocal", "pass", "elif",
    "fn", "pub", "mut", "impl", "struct", "enum", "use", "mod", "match", "loop", "trait", "dyn",
    "req", "res", "props", "params", "args", "kwargs", "data", "value", "values", "item", "items",
    "index", "key", "result", "results", "error", "err", "name", "id", "type", "options", "config",
];

fn extract_identifiers(text: &str) -> std::collections::HashSet<String> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|s| s.len() >= 3 && !s.chars().next().unwrap_or('0').is_numeric())
        .map(|s| s.to_string())
        .collect()
}

/// Blanks out the contents of '...'/"..."/`...` string literals (best-effort,
/// no escape handling) before identifier extraction. Without this, words
/// inside a human-readable error message like `throw new Error("Division by
/// zero")` get flagged as "introduced identifiers" even though they're just
/// English prose, not code references — pure noise on the one signal this
/// tool exists to provide.
fn strip_string_literals(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut quote: Option<char> = None;
    for c in line.chars() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else {
                    out.push(' ');
                }
            }
            None => {
                if c == '\'' || c == '"' || c == '`' {
                    quote = Some(c);
                } else {
                    out.push(c);
                }
            }
        }
    }
    out
}

impl<'a> PatchGenerator<'a> {
    pub fn new(db: &'a Database, provider: Box<dyn LlmProvider>) -> Self {
        Self { db, provider }
    }

    pub fn generate_patch(&self, symbol_name: &str, instruction: &str) -> Result<PatchOutput, String> {
        self.generate_patch_scoped(symbol_name, instruction, None)
    }

    /// Like `generate_patch`, but when `file_hint` is given, only resolves a
    /// symbol defined in a file whose path contains that substring.
    pub fn generate_patch_scoped(&self, symbol_name: &str, instruction: &str, file_hint: Option<&str>) -> Result<PatchOutput, String> {
        let (file_id, _start_byte, _end_byte) = if let Some(hint) = file_hint {
            let mut stmt = self.db.conn.prepare(
                "SELECT symbols.file_id, symbols.start_byte, symbols.end_byte FROM symbols JOIN files ON symbols.file_id = files.id WHERE symbols.name = ?1 AND files.path LIKE ?2 LIMIT 1"
            ).map_err(|e| e.to_string())?;
            let pattern = format!("%{}%", hint);
            stmt.query_row(rusqlite::params![symbol_name, pattern], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
            }).map_err(|_| format!("Symbol '{}' not found in a file matching '{}'.", symbol_name, hint))?
        } else {
            let mut stmt = self.db.conn.prepare("SELECT file_id, start_byte, end_byte FROM symbols WHERE name = ?1 LIMIT 1")
                .map_err(|e| e.to_string())?;
            stmt.query_row([symbol_name], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
            }).map_err(|_| format!("Symbol '{}' not found in database.", symbol_name))?
        };

        let file_path: String = self.db.conn.query_row(
            "SELECT path FROM files WHERE id = ?1",
            [file_id],
            |row| row.get(0)
        ).map_err(|e| e.to_string())?;

        let abs_file_path = self.db.resolve_path(&file_path);
        let content = fs::read(&abs_file_path).map_err(|e| e.to_string())?;
        // Ground the LLM in the FULL enclosing file, not just the target
        // symbol's own slice. Previously only the symbol's isolated source
        // was shown, so the model had no way to know what helpers/imports/
        // types actually exist elsewhere in the file and would invent
        // plausible-looking calls to things that don't exist. Showing the
        // whole file also gives correct line context for the diff hunks.
        let full_file_source = String::from_utf8_lossy(&content).to_string();

        let context = ContextObject::assemble(self.db, symbol_name)
            .map_err(|e| e.to_string())?
            .ok_or("Failed to assemble context")?;

        let prompt = build_patch_prompt(symbol_name, &full_file_source, &context, instruction);
        let (patch, _) = self.provider.generate_summary(&prompt, 20)?;

        // Grounding check: collect every identifier that's actually known —
        // from the file itself plus the graph context — and flag anything
        // added by the diff that isn't in that set.
        let mut known: std::collections::HashSet<String> = extract_identifiers(&full_file_source);
        for s in &context.siblings { known.extend(extract_identifiers(s)); }
        for s in &context.forward_dependencies { known.extend(extract_identifiers(s)); }
        for s in &context.callees { known.extend(extract_identifiers(s)); }
        for s in &context.external_imports { known.extend(extract_identifiers(s)); }
        for s in &context.renders_components { known.extend(extract_identifiers(s)); }
        for s in &context.consumes_hooks { known.extend(extract_identifiers(s)); }
        known.extend(extract_identifiers(symbol_name));

        let mut introduced: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for line in patch.lines() {
            if !line.starts_with('+') || line.starts_with("+++") {
                continue;
            }
            for token in extract_identifiers(&strip_string_literals(&line[1..])) {
                if IDENTIFIER_STOPLIST.contains(&token.as_str()) {
                    continue;
                }
                if !known.contains(&token) && seen.insert(token.clone()) {
                    introduced.push(token);
                }
            }
        }

        Ok(PatchOutput { diff: patch, introduced_identifiers: introduced })
    }

    /// Resolves the absolute file path a symbol lives in, scoped by an
    /// optional file_hint for disambiguation when the name is ambiguous.
    pub fn resolve_file_path_scoped(&self, symbol_name: &str, file_hint: Option<&str>) -> Result<String, String> {
        let file_id: i64 = if let Some(hint) = file_hint {
            let pattern = format!("%{}%", hint);
            self.db.conn.query_row(
                "SELECT symbols.file_id FROM symbols JOIN files ON symbols.file_id = files.id WHERE symbols.name = ?1 AND files.path LIKE ?2 LIMIT 1",
                rusqlite::params![symbol_name, pattern],
                |row| row.get(0),
            ).map_err(|_| format!("Symbol '{}' not found in a file matching '{}'.", symbol_name, hint))?
        } else {
            self.db.conn.query_row(
                "SELECT file_id FROM symbols WHERE name = ?1 LIMIT 1",
                [symbol_name],
                |row| row.get(0),
            ).map_err(|_| format!("Symbol '{}' not found in database.", symbol_name))?
        };

        let file_path: String = self.db.conn.query_row(
            "SELECT path FROM files WHERE id = ?1",
            [file_id],
            |row| row.get(0),
        ).map_err(|e| e.to_string())?;

        Ok(self.db.resolve_path(&file_path))
    }

    /// Resolves the absolute file path a symbol lives in, so callers (e.g. the
    /// `apply: true` path in generate_patch) know which file to run the diff
    /// against without re-deriving it from generate_patch's internals.
    pub fn resolve_file_path(&self, symbol_name: &str) -> Result<String, String> {
        self.resolve_file_path_scoped(symbol_name, None)
    }
}
