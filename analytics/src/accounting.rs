use std::collections::HashSet;
use storage::Database;

pub struct TokenAccounting;

impl TokenAccounting {
    /// Blended char+word estimate of how many LLM tokens a piece of text
    /// costs. A pure chars/4 heuristic undercounts punctuation-dense text
    /// (JSON, code full of short symbols/braces) and a pure word-count
    /// heuristic overcounts it; averaging a char-based and a word-based
    /// estimate tracks real BPE tokenizers noticeably closer than either
    /// alone across both code and prose payloads, without pulling in a full
    /// tokenizer dependency.
    pub fn estimate_tokens_from_text(text: &str) -> usize {
        if text.is_empty() {
            return 0;
        }
        let chars = text.chars().count() as f64;
        let words = text.split_whitespace().count().max(1) as f64;
        let char_based = chars / 4.0;
        let word_based = words / 0.75;
        ((char_based + word_based) / 2.0).round() as usize
    }

    /// Byte-only fallback for contexts where only a file's size on disk is
    /// known (e.g. summing many files for a baseline) and re-reading +
    /// decoding every one of them just to run the text-based estimate isn't
    /// worth the I/O.
    pub fn estimate_tokens(bytes: usize) -> usize {
        (bytes as f64 / 4.0).round() as usize
    }

    /// Real on-disk size of one file, converted to a token estimate. This is
    /// the token cost a naive agent actually pays by `Read`-ing the whole
    /// file instead of getting CodeBroker's precise slice — the honest
    /// baseline for single-file tools (`read_file_skeleton`,
    /// `read_file_snippet`, single-target `read_symbol_source`).
    pub fn real_file_tokens(db: &Database, path: &str) -> usize {
        let abs = db.resolve_path(path);
        match std::fs::metadata(&abs) {
            Ok(m) if m.is_file() => Self::estimate_tokens(m.len() as usize),
            _ => 0,
        }
    }

    /// Walks a tool's already-built JSON response for "file_path"/"path"
    /// string fields, keeps only the ones that resolve to a real file on
    /// disk, sums their real byte sizes, and converts to tokens.
    ///
    /// This is the baseline for every tool whose answer names specific
    /// files (`get_context`, `explore_graph`, `get_edit_context`,
    /// `shortest_path`, `search_codebase`, `architectural_hotspots`,
    /// `dependency_cycles`, `find_duplicate_logic`,
    /// `subsystem_communication`, multi-target `read_symbol_source`): the
    /// naive alternative to CodeBroker's precise answer is opening every one
    /// of those files in full via `Read`/grep, so the baseline scales with
    /// exactly what the query actually touched instead of a flat,
    /// query-independent constant.
    pub fn real_baseline_from_json(db: &Database, value: &serde_json::Value) -> usize {
        let mut paths = HashSet::new();
        Self::collect_file_paths(value, &mut paths);
        let total_bytes: u64 = paths
            .iter()
            .filter_map(|p| {
                let abs = db.resolve_path(p);
                let meta = std::fs::metadata(&abs).ok()?;
                meta.is_file().then_some(meta.len())
            })
            .sum();
        Self::estimate_tokens(total_bytes as usize)
    }

    fn collect_file_paths(value: &serde_json::Value, out: &mut HashSet<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    if k == "file_path" || k == "path" {
                        if let Some(s) = v.as_str() {
                            out.insert(s.to_string());
                        }
                    }
                    Self::collect_file_paths(v, out);
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    Self::collect_file_paths(v, out);
                }
            }
            _ => {}
        }
    }

    /// Sum of real on-disk bytes for every indexed file in scope, converted
    /// to tokens. The honest baseline for `repository_stats`: its answer
    /// genuinely characterizes the whole scope rather than a filtered
    /// subset of files, so the fair comparison is "how many tokens would it
    /// cost to read every file in this scope in full." Capped at 20k files
    /// so a huge monorepo can't turn one stats call into tens of thousands
    /// of stat() syscalls.
    pub fn real_scope_bytes_tokens(db: &Database, path_scope: Option<&str>) -> usize {
        const MAX_FILES: usize = 20_000;
        let mut stmt = match db.conn.prepare("SELECT path FROM files LIMIT ?1") {
            Ok(s) => s,
            Err(_) => return 0,
        };
        let rows = match stmt.query_map(rusqlite::params![MAX_FILES as i64], |r| {
            r.get::<_, String>(0)
        }) {
            Ok(r) => r,
            Err(_) => return 0,
        };
        let mut total: u64 = 0;
        for path in rows.flatten() {
            if let Some(scope) = path_scope {
                if !path.contains(scope) {
                    continue;
                }
            }
            let abs = db.resolve_path(&path);
            if let Ok(meta) = std::fs::metadata(&abs) {
                if meta.is_file() {
                    total += meta.len();
                }
            }
        }
        Self::estimate_tokens(total as usize)
    }
}

pub struct CostAccounting;

impl CostAccounting {
    /// Converts token savings into estimated US Cents, using each model
    /// family's public per-token input pricing as a per-million-token rate.
    pub fn calculate_cents_saved(tokens_saved: usize, model: &str) -> f64 {
        let cost_per_million_cents = match model.to_lowercase().as_str() {
            m if m.contains("claude") => 300.0,
            m if m.contains("gpt") => 250.0,
            m if m.contains("gemini") => 150.0,
            m if m.contains("qwen") => 50.0,
            _ => 300.0,
        };
        (tokens_saved as f64 / 1_000_000.0) * cost_per_million_cents
    }
}
