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

/// Reads the path-disambiguation hint for a symbol lookup. Accepts both
/// `file_path` (this tool's documented param name) and `path_scope` (the name
/// every scoping param uses on search_codebase/find_symbol/impact_analysis/
/// architectural_hotspots/dependency_cycles/find_duplicate_logic) so a caller
/// that reasonably guesses the wrong one based on the rest of the tool
/// surface still gets it applied, instead of the hint being silently dropped
/// and every candidate coming back unfiltered.
fn get_file_hint<'a>(arguments: &'a serde_json::Map<String, serde_json::Value>) -> Option<&'a str> {
    arguments.get("file_path")
        .or_else(|| arguments.get("path_scope"))
        .and_then(|s| s.as_str())
}

/// Adds an approximate `response_size_hint` field to a JSON object response,
/// so a calling agent can decide whether to drill deeper or stop without
/// guessing from raw JSON length. Deliberately cheap (char_count / 4, the
/// same heuristic `TokenAccounting::estimate_tokens` already uses for the
/// delivered_token_count analytics field) — not a real tokenizer, just an
/// order-of-magnitude estimate. Computed from the response as it stands
/// before this field is added, so the hint doesn't include its own size.
fn add_response_size_hint(value: &mut serde_json::Value) {
    let char_count = serde_json::to_string(value).map(|s| s.len()).unwrap_or(0);
    let approx_tokens = analytics::accounting::TokenAccounting::estimate_tokens(char_count);
    if let Some(obj) = value.as_object_mut() {
        obj.insert("response_size_hint".to_string(), serde_json::json!({
            "char_count": char_count,
            "approx_tokens": approx_tokens
        }));
    }
}

#[cfg(test)]
mod response_helpers_tests {
    use super::*;

    #[test]
    fn add_response_size_hint_is_additive_and_keeps_existing_fields() {
        let mut value = serde_json::json!({"results": ["a", "b", "c"]});
        add_response_size_hint(&mut value);

        // Existing field untouched — this must only ever add, never rename/remove.
        assert_eq!(value["results"], serde_json::json!(["a", "b", "c"]));

        let hint = &value["response_size_hint"];
        assert!(hint["char_count"].as_u64().unwrap() > 0);
        // approx_tokens is the cheap char_count/4 heuristic, not a real tokenizer.
        let char_count = hint["char_count"].as_u64().unwrap();
        let approx_tokens = hint["approx_tokens"].as_u64().unwrap();
        assert_eq!(approx_tokens, char_count / 4);
    }

    #[test]
    fn add_response_size_hint_grows_with_payload_size() {
        let mut small = serde_json::json!({"results": ["a"]});
        let mut large = serde_json::json!({"results": ["a", "b", "c", "d", "e", "f", "g", "h"]});
        add_response_size_hint(&mut small);
        add_response_size_hint(&mut large);
        assert!(
            large["response_size_hint"]["approx_tokens"].as_u64().unwrap()
                > small["response_size_hint"]["approx_tokens"].as_u64().unwrap()
        );
    }

    #[test]
    fn get_file_hint_prefers_file_path_but_falls_back_to_path_scope() {
        let mut args = serde_json::Map::new();
        args.insert("file_path".to_string(), serde_json::json!("src/auth.ts"));
        args.insert("path_scope".to_string(), serde_json::json!("src/other.ts"));
        assert_eq!(get_file_hint(&args), Some("src/auth.ts"));

        let mut alias_only = serde_json::Map::new();
        alias_only.insert("path_scope".to_string(), serde_json::json!("src/rooms/route.ts"));
        assert_eq!(get_file_hint(&alias_only), Some("src/rooms/route.ts"));

        let empty = serde_json::Map::new();
        assert_eq!(get_file_hint(&empty), None);
    }
}

/// Pre-flight ambiguity check for tools that take a bare symbol name and
/// would otherwise silently pick whichever DB row comes back first when a
/// common name (e.g. "GET", a Next.js route handler exported from dozens of
/// files) matches many definitions. Returns Some(json_response) when the
/// caller should stop and ask for disambiguation instead of guessing; the
/// response carries the full candidate list (location + kind) so the caller
/// can pick one and retry with `file_path` set, the same UX `find_symbol`
/// already provides. Returns None when it's safe to proceed (0 matches, or
/// exactly 1 match after applying file_hint).
fn check_symbol_ambiguity(db: &storage::Database, symbol: &str, file_hint: Option<&str>) -> Option<String> {
    let candidates = query::engine::find_symbol_candidates(db, symbol).ok()?;
    let filtered: Vec<&query::engine::SymbolCandidate> = match file_hint {
        Some(hint) => candidates.iter().filter(|c| c.file_path.contains(hint)).collect(),
        None => candidates.iter().collect(),
    };
    if filtered.len() > 1 {
        Some(serde_json::json!({
            "ambiguous": true,
            "symbol": symbol,
            "match_count": filtered.len(),
            "candidates": filtered.iter().take(15).map(|c| serde_json::json!({
                "kind": c.kind,
                "file_path": c.file_path,
                "start_line": c.start_line,
            })).collect::<Vec<_>>(),
            "hint": "Multiple symbols share this name. Re-run with `file_path` set to a substring of the file you mean (see `candidates` above) to disambiguate."
        }).to_string())
    } else {
        None
    }
}

/// Applies an LLM-generated unified diff to `file_path` using the system `patch`
/// binary, passing the target file explicitly as the "originalfile" argument so
/// the patch lands on the right file regardless of whatever path headers the
/// LLM put in the diff text. Returns Err (no changes made) if `patch` rejects
/// the diff as malformed/non-applying — the caller still has the raw `diff`
/// text to apply manually in that case.
fn apply_unified_diff(file_path: &str, diff_text: &str) -> Result<(), String> {
    use std::io::Write;
    let mut tmp_path = std::env::temp_dir();
    tmp_path.push(format!("codebroker_patch_{}_{}.diff", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));

    {
        let mut f = std::fs::File::create(&tmp_path).map_err(|e| format!("Failed to write temp diff file: {}", e))?;
        f.write_all(diff_text.as_bytes()).map_err(|e| format!("Failed to write temp diff file: {}", e))?;
    }

    // Same reasoning as run_index/run_incremental_index: `patch`'s own stdout
    // would otherwise land on this process's stdout, corrupting the JSON-RPC
    // transport.
    let status = std::process::Command::new("patch")
        .arg("--no-backup-if-mismatch")
        .arg("-p0")
        .arg(file_path)
        .arg("-i")
        .arg(&tmp_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    let _ = std::fs::remove_file(&tmp_path);

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!(
            "`patch` exited with status {}. The generated diff likely doesn't apply cleanly to the current file content — review the `diff` field and apply manually.",
            s
        )),
        Err(e) => Err(format!("Failed to invoke `patch`: {}. Is GNU patch installed?", e)),
    }
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

    // Stdio must NOT be inherited here: the MCP transport is JSON-RPC framed
    // over this same process's stdout, and a child's plain `println!` output
    // landing on that stream corrupts every subsequent response.
    let status = std::process::Command::new(&cli_path)
        .arg("init")
        .current_dir(project_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("Failed to spawn indexer process in {}: {}", project_dir, e))?;

    if status.success() {
        Ok(format!("Indexing complete for workspace: {}", project_dir))
    } else {
        Err(format!("Indexer exited with non-zero status ({}) while indexing {}", status, project_dir))
    }
}

/// Runs `codebroker reindex-incremental <changed_paths...>` rooted at `project_dir`,
/// re-parsing only the given files instead of paying for a full repository
/// rebuild. See indexer::reindex::reindex_paths for what this intentionally
/// trades away (alias/route/prop-type linking) in exchange for speed.
fn run_incremental_index(project_dir: &str, changed_paths: &[String]) -> Result<String, String> {
    let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let parent = current_exe.parent().ok_or_else(|| "Could not resolve codebroker-mcp binary directory".to_string())?;
    let cli_path = parent.join("codebroker");

    let mut cmd = std::process::Command::new(&cli_path);
    cmd.arg("reindex-incremental")
        .current_dir(project_dir)
        // Same reasoning as run_index: must not inherit stdout/stderr, or the
        // child's println! output corrupts the JSON-RPC stream on this fd.
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    for p in changed_paths {
        cmd.arg(p);
    }

    let status = cmd.status().map_err(|e| format!("Failed to spawn incremental indexer process in {}: {}", project_dir, e))?;

    if status.success() {
        Ok(format!("Incremental reindex complete for {} file(s) in workspace: {}", changed_paths.len(), project_dir))
    } else {
        Err(format!("Incremental indexer exited with non-zero status ({}) while indexing {} file(s) in {}", status, changed_paths.len(), project_dir))
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
                                        "description": "Shorthand for explore_graph(symbol, depth: 1, direction: 'both') with no depth/max_nodes knobs — immediate dependencies + dependents only, in a fixed response shape. Use explore_graph instead if you need depth > 1 or a single-direction (incoming/outgoing-only) traversal. If 'symbol' is ambiguous (defined in multiple files, e.g. a Next.js route handler named 'GET'), this returns a candidate list instead of guessing — pass 'file_path' to pick one. Set include_source: true to bundle the symbol's own source body (same data read_symbol_source returns) into this same response, for the common 'show me this function and what touches it' case in one call instead of two.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": {
                                                    "type": "string",
                                                    "description": "The exact name of the struct, function, or trait."
                                                },
                                                "file_path": {
                                                    "type": "string",
                                                    "description": "Optional. Substring of the defining file's path, used to disambiguate when 'symbol' matches multiple definitions. 'path_scope' is also accepted as an alias."
                                                },
                                                "include_source": {
                                                    "type": "boolean",
                                                    "description": "Optional, default false. When true, bundles the symbol's own source code into the response under a 'source' field, equivalent to also calling read_symbol_source."
                                                }
                                            },
                                            "required": ["symbol"]
                                        }
                                    },
                                    {
                                        "name": "impact_analysis",
                                        "description": "AI-generated (Qwen2.5-Coder), ~5-10s, cache-able. Adds prose explaining WHY a change to this symbol is risky (e.g. which call sites would break and how) on top of the raw dependency list — it does not just restate get_context's graph in different words. Set format: \"structured\" to skip the LLM call and get that same raw dependency data (callers/callees/signature) back as instant JSON instead, with no AI latency or token overhead — equivalent to calling get_context separately, just without the second round trip. If 'symbol' is ambiguous (defined in multiple files), returns a candidate list instead of analyzing the wrong file — pass 'file_path' to disambiguate.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": {
                                                    "type": "string",
                                                    "description": "The exact name of the struct, function, or trait to analyze."
                                                },
                                                "file_path": {
                                                    "type": "string",
                                                    "description": "Optional. Substring of the defining file's path, used to disambiguate when 'symbol' matches multiple definitions. 'path_scope' is also accepted as an alias."
                                                },
                                                "format": {
                                                    "type": "string",
                                                    "enum": ["prose", "structured"],
                                                    "description": "Optional, default 'prose'. 'prose': AI-generated risk explanation (default, ~5-10s). 'structured' (alias: 'json'): skip the LLM call, return get_context's raw dependency JSON instantly."
                                                }
                                            },
                                            "required": ["symbol"]
                                        }
                                    },
                                    {
                                        "name": "search_codebase",
                                        "description": "Discovery tool to find where a keyword or concept is mentioned. By default only matches indexed symbol/file names (fast); use mode 'text' or 'both' to also grep raw file content for string literals, comments, and config values that have no symbol name. Text-mode matching is substring-based by default, which WILL match short keywords inside longer identifiers (e.g. searching 'PORT' matches inside 'export'/'import') — set whole_word: true to require non-identifier boundaries around the match if that's not what you want. On a miss, the response includes a 'reason' field explaining why (e.g. how many symbols are in scope) instead of a bare empty array, so you can self-correct without a second round trip. If you already suspect an exact/near-exact name and a generic query like 'room' is returning a flat list mixing one relevant hit with many unrelated Medium/Low matches (e.g. 10 components named *Room*), set min_confidence: \"high\" to skip the manual filtering pass — this shrinks result count, it does not change ranking. Response includes a 'response_size_hint' field ({char_count, approx_tokens}, approx_tokens = char_count/4 — a cheap estimate, not a real token count) so you can gauge response size without guessing from raw JSON length.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "keyword": {
                                                    "type": "string",
                                                    "description": "The concept or name to search for (e.g. 'AuthService')."
                                                },
                                                "mode": {
                                                    "type": "string",
                                                    "enum": ["symbol", "text", "both"],
                                                    "description": "Optional. 'symbol' (default): match indexed symbol/file names only, fast. 'text': grep raw file content for the literal keyword (catches string literals/comments, slower). 'both': try symbol matching first, fall back to text search if that finds nothing."
                                                },
                                                "whole_word": {
                                                    "type": "boolean",
                                                    "description": "Optional, default false. Only applies to 'text'/'both' modes. When true, requires non-alphanumeric/underscore boundaries around the match — fixes false positives like 'PORT' matching inside 'export'. When false (default), plain substring matching, which is sometimes exactly what you want (e.g. finding all 'use' prefixed hooks)."
                                                },
                                                "path_scope": {
                                                    "type": "string",
                                                    "description": "Optional. Restrict results to files whose path contains this substring (e.g. 'src/auth' or 'mcp/'). Narrowing scope reduces result size and token cost."
                                                },
                                                "min_confidence": {
                                                    "type": "string",
                                                    "enum": ["high", "medium", "low"],
                                                    "description": "Optional, default unset (no filtering — all matches returned, unchanged from before this param existed). Drops every result below the given confidence tier (e.g. 'high' keeps only Exact/Prefix matches, dropping Medium/Low noise). Use this when you already suspect an exact name and just want to skip reading past weaker matches."
                                                }
                                            },
                                            "required": ["keyword"]
                                        }
                                    },
                                    {
                                        "name": "find_symbol",
                                        "description": "Lookup tool to find the definition file, line number, and kind of a symbol. Response includes a boolean 'found' field, plus results split into 'exact_matches' (the symbol's name equals your query, case-insensitive) and 'fuzzy_matches' (prefix/substring/typo hits) so you don't have to disambiguate by score — 'matches' is also present (all results, with score) for completeness. For a common name like 'GET' that matches many route files, previews are suppressed and 'compact': true is set with a 'hint' to narrow via path_scope; passing path_scope re-enables previews for the scoped subset. Honors context_lines (0 = no previews).",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": {
                                                    "type": "string",
                                                    "description": "The exact name of the symbol."
                                                },
                                                "context_lines": {
                                                    "type": "number",
                                                    "description": "Optional. Number of lines of context to fetch above and below the symbol. Defaults to 3. Pass 0 for 'existence-only' mode: skips reading source entirely and returns just found/path/line/kind — use this for a yes/no location check when you don't need the snippet."
                                                },
                                                "path_scope": {
                                                    "type": "string",
                                                    "description": "Optional. Restrict matches to files whose path contains this substring. Narrowing scope reduces result size and token cost."
                                                }
                                            },
                                            "required": ["symbol"]
                                        }
                                    },
                                    {
                                        "name": "project_overview",
                                        "description": "Raw, deterministic, <1s. Returns a topological map of the repository (file/symbol/edge counts, languages, per-directory file AND symbol density). Use this for navigation decisions — e.g. a directory with many files but near-zero symbols (assets, generated output) isn't worth a follow-up search_codebase/find_symbol call. Prefer this over project_overview_ai unless you specifically need prose. Response includes a 'response_size_hint' field ({char_count, approx_tokens}, approx_tokens = char_count/4 — a cheap estimate, not a real token count) so you can gauge response size without guessing from raw JSON length.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {}
                                        }
                                    },
                                    {
                                        "name": "project_overview_ai",
                                        "description": "AI-generated (Qwen2.5-Coder), ~5-10s, cache-able by repo topology hash. Returns narrative prose explaining what the project does and how subsystems relate. Use only when you need an explanation in words, not raw metrics — call project_overview first for the fast, free version, and only escalate to this one if you need the narrative.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {}
                                        }
                                    },
                                    {
                                        "name": "repository_stats",
                                        "description": "Raw, deterministic, <1s. Returns counts of files, symbols, edges, and languages in the repository, optionally scoped to a path. Call this before search_codebase/find_symbol on a suspect subdirectory to check whether it's worth querying at all (e.g. 0 symbols indexed there) without inferring it from project_overview's directory list.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "path_scope": {
                                                    "type": "string",
                                                    "description": "Optional. Restrict counts to files whose path contains this substring (e.g. 'src/auth')."
                                                }
                                            },
                                            "required": []
                                        }
                                    },
                                    {
                                        "name": "read_symbol_source",
                                        "description": "Read exact source code for one or more symbols without returning the entire file. Lower-level than get_implementation/get_edit_context: it returns ONLY the symbol's own source (plus directly related dependencies if include_dependencies is set), with no graph metadata. Prefer this when you already know exactly which symbol(s) you want and don't need dependents/dependencies bundled in. Batches: pass symbols: [\"GET\", \"POST\"] to fetch multiple symbols in ONE call — if you're about to make 2+ sequential read_symbol_source calls against the same file_path (e.g. fetching GET then POST from the same route file), batch them instead; it's the same total data for one round trip instead of several. Use the single 'symbol' field only when you genuinely want just one. If any name is ambiguous (defined in multiple files), returns a candidate list instead of guessing — pass 'file_path' to disambiguate.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbols": { "type": "array", "items": { "type": "string" }, "description": "Preferred. Array of symbol names for batch fetching, e.g. [\"GET\", \"POST\"] — one call instead of one per symbol." },
                                                "symbol": { "type": "string", "description": "Single symbol name. Use 'symbols' instead if you need more than one from the same lookup." },
                                                "include_dependencies": {
                                                    "type": "boolean",
                                                    "description": "If true, also includes directly related dependencies (e.g., inherited parent classes, prop interfaces)."
                                                },
                                                "file_path": {
                                                    "type": "string",
                                                    "description": "Optional. Substring of the defining file's path, applied to every name in 'symbol'/'symbols' to disambiguate matches. 'path_scope' is also accepted as an alias."
                                                }
                                            }
                                        }
                                    },
                                    {
                                        "name": "get_workspace",
                                        "description": "Returns the currently active CodeBroker workspace: its project root, database path, and whether that database has been indexed yet. Call this instead of inferring the active workspace from project_overview's workspace_root field.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {}
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
                                        "description": "Re-index the active (or given) workspace. With no 'changed_paths', forces a full rebuild of the symbol table and dependency graph edges from scratch — use this whenever repository_stats/project_overview reports 0 edges, or after large/structural changes. With 'changed_paths', re-parses only those files instead (much cheaper after a small edit like a single-function change) — but it intentionally skips import-alias/route/prop-type re-linking, so files OTHER than the changed ones that referenced a renamed symbol keep a stale edge until they're reindexed too. Fall back to a full reindex (omit changed_paths) if you need those re-linked.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "absolute_path": { "type": "string", "description": "Optional. Absolute path to re-index. Defaults to the currently active workspace." },
                                                "changed_paths": { "type": "array", "items": { "type": "string" }, "description": "Optional. Specific file paths (absolute or relative to the workspace root) to incrementally re-parse instead of doing a full rebuild." }
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
                                        "description": "Generates unified diff patch(es) to modify one or more symbols, using CodeBroker's onboard AI and semantic context. Each result's `diff` is a RAW unified diff with any Markdown code fences stripped — it pipes straight into `git apply`/`patch` with no further processing; a fenced `rendered_diff` is also included for display only. The model is grounded in the FULL enclosing file (not just the symbol's own slice) and explicitly instructed not to invent helpers/imports that don't exist — but it is still an LLM, so each result also includes `introduced_identifiers`: any name in the diff's added CODE lines (comments and string literals are ignored, so prose words won't be flagged) that wasn't found anywhere in the file or its known graph context. ALWAYS check `introduced_identifiers` before trusting a patch — a non-empty list means either a deliberately new name or a hallucinated reference, and you must tell which by eye. Each symbol is patched independently (one diff per symbol) and returned in a `results` array. If a symbol name is ambiguous (e.g. \"GET\" defined in many route files), that entry comes back as a candidate list instead of a generated patch — pass 'file_path' to disambiguate and retry. By default this only returns diff text for review (dry_run); pass apply: true to have CodeBroker write it to disk via `patch`.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": { "type": "string", "description": "The exact name of a single symbol to modify. Use 'symbols' instead for multiple." },
                                                "symbols": { "type": "array", "items": { "type": "string" }, "description": "Array of symbol names to patch in one call, e.g. a manifest + a background script + a content script that all need the same kind of change." },
                                                "instruction": { "type": "string", "description": "Instructions for what change to make. Applied identically to every symbol in 'symbols'." },
                                                "apply": { "type": "boolean", "description": "Default false: only return the diff text for review (preview/dry-run). Set true to write each diff to disk immediately." },
                                                "file_path": { "type": "string", "description": "Optional. Substring of the defining file's path, applied to every name in 'symbol'/'symbols' to disambiguate matches. 'path_scope' is also accepted as an alias." }
                                            },
                                            "required": ["instruction"]
                                        }
                                    },
                                    {
                                        "name": "get_implementation",
                                        "description": "Equivalent to calling read_symbol_source + get_context together in one round trip: returns the symbol's own source PLUS its forward/reverse graph dependencies, bundled as {symbol_source, context}. Use this (not read_symbol_source) when you need to understand how a symbol works in relation to its callers/callees, not just read its body. If 'symbol' is ambiguous, returns a candidate list instead of guessing — pass 'file_path' to disambiguate.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": { "type": "string" },
                                                "file_path": { "type": "string", "description": "Optional. Substring of the defining file's path, used to disambiguate when 'symbol' matches multiple definitions. 'path_scope' is also accepted as an alias." }
                                            },
                                            "required": ["symbol"]
                                        }
                                    },
                                    {
                                        "name": "get_edit_context",
                                        "description": "Equivalent to calling read_symbol_source + get_context together, reshaped specifically for editing: returns {target_implementation, forward_dependencies, reverse_dependencies, same_file_callers, suggested_edit_boundaries, response_size_hint}. IMPORTANT: reverse_dependencies is cross-file ONLY (it's backed by import edges, and a file never imports its own symbols) — an empty reverse_dependencies does NOT mean a symbol is unused. Check same_file_callers too: a private helper called only by a sibling function below it in the same file will show reverse_dependencies: [] and same_file_callers: [\"callerName\"], and is NOT dead code. Same underlying data as get_implementation, different shape — use this one before making a change (it surfaces what depends on the symbol you're about to edit), use get_implementation when you're just trying to understand existing code. If 'symbol' is ambiguous, returns a candidate list instead of guessing — pass 'file_path' to disambiguate. response_size_hint is {char_count, approx_tokens} (approx_tokens = char_count/4, a cheap estimate not a real token count) so you can gauge response size without guessing from raw JSON length.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": { "type": "string" },
                                                "file_path": { "type": "string", "description": "Optional. Substring of the defining file's path, used to disambiguate when 'symbol' matches multiple definitions. 'path_scope' is also accepted as an alias." }
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
                                        "description": "General-purpose BFS traversal of the code graph from a symbol. This is the one tool to reach for for radius/neighborhood questions: get_context is just depth=1/direction='both' with no knobs, and graph_subtree is just direction='undirected' with a fixed max_nodes=500 — prefer explore_graph directly over either unless you specifically want their simpler fixed-shape output. Only shortest_path is a genuinely different algorithm (two named endpoints, not a radius) and stays separate.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": { "type": "string" },
                                                "depth": { "type": "number", "description": "Traversal depth. Max 5." },
                                                "direction": { "type": "string", "enum": ["incoming", "outgoing", "both", "undirected"], "description": "'incoming': callers/dependents only. 'outgoing': dependencies only. 'both' (default): both directions, edges reported with true direction. 'undirected': same node set as 'both' (use this to replace graph_subtree)." },
                                                "max_nodes": { "type": "number", "description": "Cap on returned nodes (1-200). Default 100. Lower this for hotspot symbols with large fan-in/out to avoid oversized responses." }
                                            },
                                            "required": ["symbol", "depth", "direction"]
                                        }
                                    },
                                    {
                                        "name": "shortest_path",
                                        "description": "Find the shortest relationship path between two NAMED symbols in the repository graph. Distinct from explore_graph/graph_subtree/get_context, which all explore a radius around a single symbol rather than connecting two specific endpoints.",
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
                                                "limit": { "type": "number", "description": "Max number of hotspots to return. Default 20." },
                                                "path_scope": { "type": "string", "description": "Optional. Restrict scoring to symbols whose file path contains this substring (e.g. 'src/auth'). Use this on a large repo to avoid repo-wide noise when you only care about one subsystem." }
                                            }
                                        }
                                    },
                                    {
                                        "name": "dependency_cycles",
                                        "description": "Detect architectural circular dependencies within the repository graph. Each cycle is classified as cross_file (spans multiple files — a real cross-module import cycle) or same-file (mutual recursion within one file, usually benign). By default only cross_file cycles are returned, since narrowing path_scope otherwise tends to leave mostly same-file recursion noise as scope shrinks; same_file_cycles_found is still reported so you know how much was filtered out. Set include_same_file: true to get the old behavior back.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "limit": { "type": "number", "description": "Max number of cycles to return in detail (1-500). Default 25. `cycles_found`/`cross_file_cycles_found`/`same_file_cycles_found` in the response always report the true totals." },
                                                "path_scope": { "type": "string", "description": "Optional. Restrict the scanned edge set to symbols whose file path contains this substring." },
                                                "include_same_file": { "type": "boolean", "description": "Default false: only return cross_file cycles. Set true to also include same-file mutual-recursion cycles." }
                                            }
                                        }
                                    },
                                    {
                                        "name": "find_duplicate_logic",
                                        "description": "Scan the repository for near-duplicate function/component bodies that live in different files (copy-pasted logic). Unlike graph tools, this catches duplication even when there is no shared call/import edge between the copies.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "min_length": { "type": "number", "description": "Minimum normalized source length (characters) to consider. Default 80. Lower values surface more, noisier matches." },
                                                "path_scope": { "type": "string", "description": "Optional. Restrict the scan to symbols whose file path contains this substring." }
                                            }
                                        }
                                    },
                                    {
                                        "name": "graph_subtree",
                                        "description": "DEPRECATED alias, kept for backward compatibility: equivalent to explore_graph(root_symbol, depth, direction: 'undirected'), with a response shape that reports per-node depth and string-named edges instead of ids. Prefer calling explore_graph directly with direction: 'undirected'.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "root_symbol": { "type": "string", "description": "The root symbol name" },
                                                "depth": { "type": "number", "description": "Traversal depth. Max 5." },
                                                "max_nodes": { "type": "number", "description": "Cap on returned nodes (1-500). Default 100 (was a fixed 500 with no escape hatch). Lower this for hub symbols with large fan-in/out." }
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
                                let file_hint = get_file_hint(&arguments);
                                // Bundles the symbol's own source body inline on request, so the
                                // common "show me this function and what touches it" case is one
                                // call instead of get_context + a separate read_symbol_source.
                                let include_source = arguments.get("include_source").and_then(|s| s.as_bool()).unwrap_or(false);
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        if let Some(amb) = check_symbol_ambiguity(&db, symbol, file_hint) {
                                            amb
                                        } else {
                                            estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_graph_context(&db);
                                            match query::context::ContextObject::assemble_scoped(&db, symbol, file_hint) {
                                                Ok(Some(context)) => {
                                                    if include_source {
                                                        let source = query::retrieval::read_symbol_source_scoped(&db, symbol, false, file_hint).unwrap_or_default();
                                                        let mut value = serde_json::to_value(&context).unwrap_or_default();
                                                        if let Some(obj) = value.as_object_mut() {
                                                            obj.insert("source".to_string(), serde_json::to_value(&source).unwrap_or_default());
                                                        }
                                                        serde_json::to_string_pretty(&value).unwrap_or_else(|_| "Error serializing context JSON".to_string())
                                                    } else {
                                                        serde_json::to_string_pretty(&context).unwrap_or_else(|_| "Error serializing context JSON".to_string())
                                                    }
                                                }
                                                Ok(None) => format!("Symbol '{}' not found in database.", symbol),
                                                Err(e) => format!("Error assembling context: {}", e),
                                            }
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "impact_analysis" => {
                                let symbol = arguments.get("symbol").and_then(|s| s.as_str()).unwrap_or("");
                                let file_hint = get_file_hint(&arguments);
                                // "structured" skips the LLM call entirely and returns the same
                                // graph data get_context exposes (callers/callees/signature) as
                                // plain JSON — for callers that just want the dependency list and
                                // would otherwise pay ~3x the tokens re-reading it out of markdown
                                // prose. Default stays "prose": the AI-generated risk explanation
                                // is this tool's actual value-add over get_context.
                                let format = arguments.get("format").and_then(|s| s.as_str()).unwrap_or("prose");
                                if format == "structured" || format == "json" {
                                    match storage::Database::new(&db_path) {
                                        Ok(db) => {
                                            if let Some(amb) = check_symbol_ambiguity(&db, symbol, file_hint) {
                                                amb
                                            } else {
                                                let context = query::context::ContextObject::assemble_scoped(&db, symbol, file_hint).unwrap_or_default();
                                                serde_json::to_string_pretty(&context).unwrap_or_default()
                                            }
                                        }
                                        Err(_) => "Error connecting to db".to_string(),
                                    }
                                } else {
                                    let hf_token = std::env::var("HF_API_TOKEN").unwrap_or_default();
                                    if hf_token.is_empty() {
                                        "Error: HF_API_TOKEN environment variable is not set. Pass format: \"structured\" to get the raw dependency data without an LLM call.".to_string()
                                    } else {
                                        match storage::Database::new(&db_path) {
                                            Ok(db) => {
                                                // Same ambiguity gate as get_context/get_implementation: don't
                                                // burn an AI call analyzing the wrong file's symbol when the
                                                // name matches multiple definitions.
                                                if let Some(amb) = check_symbol_ambiguity(&db, symbol, file_hint) {
                                                    amb
                                                } else {
                                                    estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_graph_context(&db);
                                                    let provider = Box::new(semantic::huggingface::HuggingFaceProvider::new(hf_token));
                                                    let generator = semantic::generator::SummaryGenerator::new(&db, provider);
                                                    match generator.generate_scoped(symbol, file_hint) {
                                                        Ok((summary, hit)) => {
                                                            cache_hit = hit;
                                                            summary
                                                        },
                                                        Err(e) => format!("Error generating impact analysis: {}", e),
                                                    }
                                                }
                                            }
                                            Err(_) => "Error connecting to db".to_string(),
                                        }
                                    }
                                }
                            }
                            "search_codebase" => {
                                let keyword = arguments.get("keyword").and_then(|s| s.as_str()).unwrap_or("");
                                let path_scope = arguments.get("path_scope").and_then(|s| s.as_str());
                                let mode_str = arguments.get("mode").and_then(|s| s.as_str()).unwrap_or("symbol");
                                let mode = query::engine::SearchMode::from(mode_str);
                                let whole_word = arguments.get("whole_word").and_then(|b| b.as_bool()).unwrap_or(false);
                                let min_confidence = arguments.get("min_confidence").and_then(|s| s.as_str());

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
                                        match query::engine::search_symbols(&db, keyword, &semantic_tokens, llm_used, path_scope, mode, whole_word, min_confidence) {
                                            Ok((results, reason)) => {
                                                let mut payload = serde_json::json!({
                                                    "workspace_root": db.project_root,
                                                    "results": results
                                                });
                                                if let Some(r) = reason {
                                                    payload["reason"] = serde_json::Value::String(r);
                                                }
                                                add_response_size_hint(&mut payload);
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
                                let path_scope = arguments.get("path_scope").and_then(|s| s.as_str());

                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_find_symbol_context(&db, symbol);
                                        match query::engine::find_symbol(&db, symbol, context_lines, path_scope) {
                                            Ok(results) => {
                                                // Disambiguation (#4): a common name like "GET" matches
                                                // many route files. Past a threshold, suppress the
                                                // per-match source previews and return a compact
                                                // candidate list instead — the caller is meant to narrow
                                                // with path_scope, not read 9+ full previews. A narrowed
                                                // path_scope opts back into previews.
                                                const PREVIEW_SUPPRESSION_THRESHOLD: usize = 5;
                                                let compact = results.len() > PREVIEW_SUPPRESSION_THRESHOLD && path_scope.is_none();
                                                let show_preview = context_lines > 0 && !compact;

                                                let query_lower = symbol.to_lowercase();
                                                let to_entry = |name: &str, path: &str, kind: &str, line: &i64, preview: &str| {
                                                    let mut m = serde_json::json!({
                                                        "name": name,
                                                        "path": path,
                                                        "kind": kind,
                                                        "line": line,
                                                    });
                                                    if show_preview {
                                                        m["preview"] = serde_json::Value::String(preview.to_string());
                                                    }
                                                    m
                                                };

                                                let mut matches = Vec::new();
                                                let mut exact_matches = Vec::new();
                                                let mut fuzzy_matches = Vec::new();
                                                for (name, path, kind, line, preview, score) in results.iter() {
                                                    // Exact/fuzzy split (#3): exact == the symbol's own
                                                    // name equals the query (case-insensitive). Everything
                                                    // else (prefix/substring/levenshtein hits) is fuzzy.
                                                    let entry = to_entry(name, path, kind, line, preview);
                                                    let mut full = entry.clone();
                                                    full["score"] = serde_json::json!(score);
                                                    matches.push(full);
                                                    if name.to_lowercase() == query_lower {
                                                        exact_matches.push(entry);
                                                    } else {
                                                        fuzzy_matches.push(entry);
                                                    }
                                                }

                                                let mut payload = serde_json::json!({
                                                    "workspace_root": db.project_root,
                                                    "query": symbol,
                                                    "found": !matches.is_empty(),
                                                    "exact_matches": exact_matches,
                                                    "fuzzy_matches": fuzzy_matches,
                                                    "matches": matches,
                                                });
                                                if compact {
                                                    payload["compact"] = serde_json::Value::Bool(true);
                                                    payload["hint"] = serde_json::Value::String(format!(
                                                        "{} matches for \"{}\" — previews suppressed. Narrow with path_scope (e.g. a directory or file substring) to get source previews for a specific match.",
                                                        results.len(), symbol
                                                    ));
                                                }
                                                serde_json::to_string_pretty(&payload).unwrap_or_default()
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
                                                add_response_size_hint(&mut output);
                                                serde_json::to_string_pretty(&output).unwrap_or_default()
                                            },
                                            Err(e) => format!("Error building overview: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "repository_stats" => {
                                let path_scope = arguments.get("path_scope").and_then(|s| s.as_str());
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_graph_context(&db);
                                        match query::engine::build_project_overview_scoped(&db, path_scope) {
                                            Ok(overview) => {
                                                let stats = serde_json::json!({
                                                    "path_scope": path_scope,
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
                                let file_hint = get_file_hint(&arguments);

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
                                            let mut ambiguous: Vec<serde_json::Value> = Vec::new();
                                            for symbol in &targets {
                                                if let Some(amb) = check_symbol_ambiguity(&db, symbol, file_hint) {
                                                    ambiguous.push(serde_json::from_str(&amb).unwrap_or(serde_json::Value::Null));
                                                }
                                            }
                                            if !ambiguous.is_empty() {
                                                serde_json::json!({ "ambiguous": true, "results": ambiguous }).to_string()
                                            } else {
                                                let mut combined_results = Vec::new();
                                                let mut has_error = false;
                                                let mut err_msg = String::new();
                                                for symbol in targets {
                                                    match query::retrieval::read_symbol_source_scoped(&db, &symbol, include_deps, file_hint) {
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
                                        }
                                        Err(_) => "Error connecting to db".to_string(),
                                    }
                                }
                            }
                            "get_workspace" => {
                                let resolved = resolve_workspace();
                                serde_json::json!({
                                    "project_root": resolved.project_root,
                                    "db_path": resolved.db_path,
                                    "indexed": resolved.exists,
                                }).to_string()
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
                                let changed_paths: Vec<String> = arguments.get("changed_paths")
                                    .and_then(|a| a.as_array())
                                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                                    .unwrap_or_default();
                                let project_dir = match target {
                                    Some(p) if !p.is_empty() => p.to_string(),
                                    _ => resolve_workspace().project_root,
                                };
                                if !std::path::Path::new(&project_dir).exists() {
                                    format!("Error: Path '{}' does not exist.", project_dir)
                                } else {
                                    let index_result = if changed_paths.is_empty() {
                                        run_index(&project_dir)
                                    } else {
                                        run_incremental_index(&project_dir, &changed_paths)
                                    };
                                    match index_result {
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
                                let path_scope = arguments.get("path_scope").and_then(|s| s.as_str());

                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        match query::graph::architectural_hotspots(&db, limit, path_scope) {
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
                                let path_scope = arguments.get("path_scope").and_then(|s| s.as_str());
                                let include_same_file = arguments.get("include_same_file").and_then(|b| b.as_bool()).unwrap_or(false);
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        match query::graph::dependency_cycles(&db, limit, path_scope, include_same_file) {
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
                                let path_scope = arguments.get("path_scope").and_then(|s| s.as_str());
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        match query::duplicates::find_duplicate_logic(&db, min_length, path_scope) {
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
                                let max_nodes = arguments.get("max_nodes").and_then(|n| n.as_u64()).map(|n| n as usize);

                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        match query::graph::graph_subtree(&db, root_symbol, depth, max_nodes) {
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
                                let mut symbols: Vec<String> = Vec::new();
                                if let Some(s) = arguments.get("symbol").and_then(|s| s.as_str()) {
                                    if !s.is_empty() {
                                        symbols.push(s.to_string());
                                    }
                                }
                                if let Some(arr) = arguments.get("symbols").and_then(|a| a.as_array()) {
                                    for v in arr {
                                        if let Some(s) = v.as_str() {
                                            symbols.push(s.to_string());
                                        }
                                    }
                                }
                                let instruction = arguments.get("instruction").and_then(|s| s.as_str()).unwrap_or("");
                                let file_hint = get_file_hint(&arguments);
                                // dry_run by default: a generated diff is only a preview until the
                                // caller explicitly opts into writing it to disk.
                                let apply = arguments.get("apply").and_then(|b| b.as_bool()).unwrap_or(false);

                                if symbols.is_empty() {
                                    "Error: Must provide 'symbol' or 'symbols'".to_string()
                                } else {
                                    let hf_token = std::env::var("HF_API_TOKEN").unwrap_or_default();
                                    if hf_token.is_empty() {
                                        "Error: HF_API_TOKEN environment variable is not set.".to_string()
                                    } else {
                                        match storage::Database::new(&db_path) {
                                            Ok(db) => {
                                                let provider = Box::new(semantic::huggingface::HuggingFaceProvider::new(hf_token));
                                                let patch_gen = semantic::generator::PatchGenerator::new(&db, provider);
                                                let mut results = Vec::new();
                                                for symbol in &symbols {
                                                    // Don't waste an AI call patching the wrong file: if the name is
                                                    // ambiguous (e.g. "GET" defined in many route files), stop and
                                                    // ask the caller to disambiguate with file_path first.
                                                    if let Some(amb) = check_symbol_ambiguity(&db, symbol, file_hint) {
                                                        let amb_val: serde_json::Value = serde_json::from_str(&amb).unwrap_or(serde_json::Value::Null);
                                                        results.push(amb_val);
                                                        continue;
                                                    }
                                                    match patch_gen.generate_patch_scoped(symbol, instruction, file_hint) {
                                                        Ok(output) => {
                                                            let mut entry = serde_json::json!({
                                                                "symbol": symbol,
                                                                "diff": output.diff,
                                                                "rendered_diff": output.rendered_diff,
                                                                "introduced_identifiers": output.introduced_identifiers,
                                                                "applied": false,
                                                            });
                                                            if !output.introduced_identifiers.is_empty() {
                                                                entry["warning"] = serde_json::Value::String(
                                                                    "introduced_identifiers were not found in the target file or its known graph context. Verify each one actually exists (or is intentionally new) before trusting/applying this patch.".to_string()
                                                                );
                                                            }
                                                            if apply {
                                                                match patch_gen.resolve_file_path_scoped(symbol, file_hint) {
                                                                    Ok(file_path) => match apply_unified_diff(&file_path, &output.diff) {
                                                                        Ok(_) => { entry["applied"] = serde_json::Value::Bool(true); }
                                                                        Err(e) => { entry["apply_error"] = serde_json::Value::String(e); }
                                                                    },
                                                                    Err(e) => { entry["apply_error"] = serde_json::Value::String(e); }
                                                                }
                                                            }
                                                            results.push(entry);
                                                        }
                                                        Err(e) => {
                                                            results.push(serde_json::json!({ "symbol": symbol, "error": e }));
                                                        }
                                                    }
                                                }
                                                serde_json::json!({ "apply": apply, "results": results }).to_string()
                                            }
                                            Err(_) => "Error connecting to db".to_string(),
                                        }
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
                                let file_hint = get_file_hint(&arguments);
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        if let Some(amb) = check_symbol_ambiguity(&db, symbol, file_hint) {
                                            amb
                                        } else {
                                            let source = query::retrieval::read_symbol_source_scoped(&db, symbol, false, file_hint).unwrap_or_default();
                                            let context = query::context::ContextObject::assemble_scoped(&db, symbol, file_hint).unwrap_or_default();
                                            let implementation = serde_json::json!({
                                                "symbol_source": source,
                                                "context": context
                                            });
                                            serde_json::to_string_pretty(&implementation).unwrap_or_default()
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "get_edit_context" => {
                                let symbol = arguments.get("symbol").and_then(|s| s.as_str()).unwrap_or("");
                                let file_hint = get_file_hint(&arguments);
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        if let Some(amb) = check_symbol_ambiguity(&db, symbol, file_hint) {
                                            amb
                                        } else {
                                            let source = query::retrieval::read_symbol_source_scoped(&db, symbol, false, file_hint).unwrap_or_default();
                                            let context = query::context::ContextObject::assemble_scoped(&db, symbol, file_hint).unwrap_or_default();
                                            let mut edit_context = serde_json::json!({
                                                "target_implementation": source,
                                                "forward_dependencies": context.as_ref().map(|c| c.forward_dependencies.clone()).unwrap_or_default(),
                                                "reverse_dependencies": context.as_ref().map(|c| c.reverse_dependencies.clone()).unwrap_or_default(),
                                                "same_file_callers": context.as_ref().map(|c| c.same_file_callers.clone()).unwrap_or_default(),
                                                "suggested_edit_boundaries": "Use start_line and end_line from target_implementation"
                                            });
                                            add_response_size_hint(&mut edit_context);
                                            serde_json::to_string_pretty(&edit_context).unwrap_or_default()
                                        }
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