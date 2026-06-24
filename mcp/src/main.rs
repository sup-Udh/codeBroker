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
                    eprintln!(
                        "Active workspace '{}' has no index yet at {}",
                        project_path, db_path
                    );
                }
                return ResolvedWorkspace {
                    db_path,
                    project_root: project_path,
                    exists,
                };
            }
        }
    }

    // No active_project pointer has ever been set: fall back to CWD.
    let cwd_db = ".codebroker/codebroker.db".to_string();
    let exists = std::path::Path::new(&cwd_db).exists();
    let project_root = std::env::current_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    ResolvedWorkspace {
        db_path: cwd_db,
        project_root,
        exists,
    }
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
    arguments
        .get("file_path")
        .or_else(|| arguments.get("path_scope"))
        .and_then(|s| s.as_str())
}

/// Decides whether impact_analysis should take the cheap, deterministic path
/// (no LLM): true when the symbol's total dependency count is below the
/// threshold, OR when no model is available at all. Kept as a standalone fn so
/// the cheap/LLM boundary is unit-testable without the full MCP request loop.
fn use_cheap_impact_path(
    total_dependencies: usize,
    risk_threshold: usize,
    has_openai_key: bool,
) -> bool {
    total_dependencies < risk_threshold || !has_openai_key
}

/// Maps a file extension to a Markdown fenced-code-block language tag, so
/// capsule output gets syntax highlighting in agents that render Markdown.
fn lang_from_path(path: &str) -> &'static str {
    if path.ends_with(".rs") {
        "rust"
    } else if path.ends_with(".py") {
        "python"
    } else if path.ends_with(".tsx") {
        "tsx"
    } else if path.ends_with(".ts") {
        "typescript"
    } else if path.ends_with(".jsx") {
        "jsx"
    } else if path.ends_with(".js") {
        "javascript"
    } else if path.ends_with(".vue") {
        "vue"
    } else if path.ends_with(".toml") {
        "toml"
    } else if path.ends_with(".json") {
        "json"
    } else {
        ""
    }
}

/// find_symbol_candidates/search_symbols resolve paths to absolute (via
/// db.resolve_path) before returning, but read_symbol_source_scoped's
/// file_hint is matched against the RAW relative path stored in the `files`
/// table (`files.path LIKE '%hint%'`). An absolute path is never a substring
/// of the relative one it was built from, so passing it straight through as
/// a hint silently matches nothing. Strip the project root prefix back off
/// so the hint lines up with what's actually stored.
fn relative_hint<'a>(db: &storage::Database, absolute_path: &'a str) -> &'a str {
    let prefix = format!("{}/", db.project_root.trim_end_matches('/'));
    absolute_path
        .strip_prefix(prefix.as_str())
        .unwrap_or(absolute_path)
}

/// Collapses a symbol's source down to its signature line(s) plus a
/// `[N lines hidden]` marker, for the "Supporting Context" section of
/// generate_context_capsule — callers/callees are shown structurally
/// (so the agent knows they exist and how they connect) without paying
/// the token cost of their full bodies.
fn signature_skeleton(db: &storage::Database, name: &str, file_path: &str) -> String {
    match query::retrieval::read_symbol_source_scoped(db, name, false, Some(file_path)) {
        Ok(results) => match results.into_iter().next() {
            Some(r) => {
                let lines: Vec<&str> = r.source.lines().collect();
                if lines.is_empty() {
                    return format!("// {} (source unavailable)", name);
                }
                let total = lines.len();
                let sig_end = lines.iter().position(|l| l.contains('{')).unwrap_or(0);
                let sig_end = sig_end.min(total.saturating_sub(1));
                let mut out = lines[..=sig_end].join("\n");
                let hidden = total - (sig_end + 1);
                if hidden > 0 {
                    out.push_str(&format!(
                        "\n    ... // [{} lines hidden for token reduction]",
                        hidden
                    ));
                }
                out
            }
            None => format!("// {} (source unavailable)", name),
        },
        Err(_) => format!("// {} (source unavailable)", name),
    }
}

/// Orchestrates the one-shot "Context Capsule" workflow: discover the 1-3
/// pivot symbols matching `query`, fetch their full implementation, expand
/// to immediate (depth=1) callers/callees via the graph, and render
/// everything as a single Markdown document instead of forcing the caller
/// to chain search_codebase -> get_implementation -> explore_graph manually.
fn generate_context_capsule(
    db: &storage::Database,
    query: &str,
    file_hint: Option<&str>,
    semantic_tokens: &[String],
    query_vector: Option<&[f32]>,
) -> String {
    use std::fmt::Write;
    let mut md = String::new();
    let _ = writeln!(md, "# CodeBroker Context Capsule\n");
    let _ = writeln!(md, "**Query:** {}\n", query);

    // Discover: prefer exact-name matches, fall back to fuzzy/semantic search.
    let mut pivots: Vec<(String, String)> = Vec::new();
    if let Ok(candidates) = query::engine::find_symbol_candidates(db, query) {
        for c in candidates.iter() {
            if file_hint.map_or(true, |h| c.file_path.contains(h)) {
                pivots.push((c.name.clone(), c.file_path.clone()));
            }
            if pivots.len() >= 3 {
                break;
            }
        }
    }
    // Concept pass: deterministic, no API key required, and runs BEFORE the
    // blunter text-scan fallback below — not just when pivots are empty.
    // Checks whether the query's own words map to a domain concept (auth,
    // realtime, notifications, database) and pulls in symbols tagged with
    // it. Running this ahead of the text-scan matters specifically when two
    // conceptually distinct queries happen to share a generic literal word:
    // "real-time collaboration between multiple users editing simultaneously"
    // and "notification infrastructure for alerting users of events" both
    // contain "users", which the text-scan would otherwise match to the same
    // generic `User` interface / `user` local variable for both queries —
    // exactly the "two different queries return identical results" bug
    // benchmark run_005 found. Checking concepts first means "collaboration"
    // routes to the realtime concept's symbols and "notification"/"alerting"
    // routes to the notifications concept's symbols, independently, before
    // either query gets a chance to collide on "users".
    if pivots.is_empty() {
        let mut concept_names: Vec<&'static str> = Vec::new();
        for word in query.split_whitespace() {
            for c in query::concepts::concepts_matching_term(word) {
                if !concept_names.contains(&c) {
                    concept_names.push(c);
                }
            }
        }
        'concept_search: for concept in concept_names {
            if let Ok(matches) = query::concepts::symbols_for_concept(db, concept) {
                for m in matches {
                    if file_hint.map_or(true, |h| m.file_path.contains(h)) {
                        pivots.push((m.symbol_name, m.file_path));
                    }
                    if pivots.len() >= 3 {
                        break 'concept_search;
                    }
                }
            }
        }
    }

    if pivots.is_empty() {
        // `Both` falls back to a raw text scan of file content when the
        // symbol-name and concept passes above find nothing — this is still
        // deterministic keyword/substring matching (with light
        // suffix-stripping; see `stem_variants`), not embedding-based
        // semantic search, but it catches natural-language queries whose
        // words show up in code/comments rather than literally in a symbol
        // name or mapping to a known concept.
        if let Ok((results, _reason)) = query::engine::search_symbols(
            db,
            query,
            semantic_tokens,
            query_vector,
            false,
            file_hint,
            query::engine::SearchMode::Both,
            false,
            None,
        ) {
            for r in results.iter().filter(|r| r.kind != "file") {
                pivots.push((r.name.clone(), r.path.clone()));
                if pivots.len() >= 3 {
                    break;
                }
            }
        }
    }

    if pivots.is_empty() {
        let _ = writeln!(
            md,
            "_No matching symbols found for this query. Try search_codebase with mode: \"both\" for a broader sweep._"
        );
        return md;
    }

    let _ = writeln!(md, "## Pivot Symbols (Full Implementation)\n");

    let mut seen_support: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    let mut support_sections: Vec<String> = Vec::new();

    for (name, hint) in &pivots {
        let rel_hint = relative_hint(db, hint);
        let sources = query::retrieval::read_symbol_source_scoped(db, name, false, Some(rel_hint))
            .unwrap_or_default();
        let Some(src) = sources.into_iter().next() else {
            continue;
        };

        let _ = writeln!(md, "### `{}::{}`", src.file_path, src.symbol_name);
        let _ = writeln!(md, "*Why:* Matched search query directly.\n");

        // If the source file is JSON (e.g., package.json), we cannot extract a code body.
        // Emit a placeholder instead of the raw source to avoid byte‑range errors.
        let src_body = if src.file_path.ends_with(".json") {
            "/* JSON symbol – no source body */".to_string()
        } else {
            src.source.trim_end().to_string()
        };

        let _ = writeln!(md, "```{}", lang_from_path(&src.file_path));
        let _ = writeln!(md, "{}", src_body);
        let _ = writeln!(md, "```\n");

        if let Ok(graph) =
            query::graph::explore_graph(db, name, 1, query::graph::GraphDirection::Both, 30)
        {
            let root_id = graph
                .nodes
                .first()
                .map(|n| n.id.clone())
                .unwrap_or_default();
            let root_kind = graph
                .nodes
                .first()
                .map(|n| n.kind.clone())
                .unwrap_or_default();
            let mut scored_nodes = Vec::new();
            for node in graph.nodes.iter().skip(1) {
                let mut score = 0.0;

                let q_lower = query.to_lowercase();
                if node.name.to_lowercase().contains(&q_lower) {
                    score += 5.0;
                }
                if node.file_path.to_lowercase().contains(&q_lower) {
                    score += 3.0;
                }

                let root_dir = std::path::Path::new(&graph.nodes[0].file_path).parent();
                let node_dir = std::path::Path::new(&node.file_path).parent();
                if root_dir.is_some() && root_dir == node_dir {
                    score += 2.0;
                }

                // Centrality: check edge count for the node
                let mut degree = 0;
                if let Ok(id_val) = node.id.replace("s", "").parse::<i64>() {
                    let c: i64 = db.conn.query_row(
                        "SELECT COUNT(*) FROM edges WHERE source_symbol_id = ?1 OR target_symbol_id = ?1",
                        rusqlite::params![id_val],
                        |r| r.get(0)
                    ).unwrap_or(0);
                    degree = c as usize;
                }
                score += (degree as f64) * 0.1;

                scored_nodes.push((node, score));
            }

            scored_nodes.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            for (node, _score) in scored_nodes.into_iter().take(8) {
                let key = (node.file_path.clone(), node.name.clone());
                if seen_support.contains(&key) {
                    continue;
                }
                seen_support.insert(key);

                // A class/interface ("type") on either end of the edge can't
                // actually be CALLED — the edge kind on these is usually a
                // byproduct of name-based linking (e.g. a prop/parameter type
                // annotation), not a real call/reference. Relabel so the
                // capsule doesn't claim a type "CALLS ->" or is "CALLS ->"-ed.
                let is_type_relation = root_kind == "type" || node.kind == "type";
                let display_kind = |kind: &str| -> String {
                    if is_type_relation
                        && (kind == "calls" || kind == "method_call" || kind == "imports")
                    {
                        "USES_TYPE".to_string()
                    } else {
                        kind.to_uppercase()
                    }
                };

                let relation = graph
                    .edges
                    .iter()
                    .find_map(|e| {
                        if e.source == root_id && e.target == node.id {
                            Some(format!("Pivot {} ->", display_kind(&e.kind)))
                        } else if e.target == root_id && e.source == node.id {
                            Some(format!("{} -> Pivot", display_kind(&e.kind)))
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| "RELATED TO Pivot".to_string());

                let skeleton = signature_skeleton(db, &node.name, &node.file_path);
                support_sections.push(format!(
                    "### `{}::{}` — {}\n```{}\n{}\n```\n",
                    node.file_path,
                    node.name,
                    relation,
                    lang_from_path(&node.file_path),
                    skeleton
                ));
            }
        }
    }

    if !support_sections.is_empty() {
        let _ = writeln!(md, "## Supporting Context (Skeletons)\n");
        for section in support_sections {
            let _ = writeln!(md, "{}", section);
        }
    }

    md
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
        obj.insert(
            "response_size_hint".to_string(),
            serde_json::json!({
                "char_count": char_count,
                "approx_tokens": approx_tokens
            }),
        );
    }
}

#[cfg(test)]
mod impact_analysis_tests {
    use super::*;

    // #2 — small symbols take the cheap (no-LLM) path; large ones use the LLM.
    #[test]
    fn cheap_path_below_threshold() {
        // 2 total deps, threshold 5, model available -> cheap path.
        assert!(use_cheap_impact_path(2, 5, true));
    }

    #[test]
    fn llm_path_at_or_above_threshold() {
        // 5 total deps, threshold 5, model available -> LLM path.
        assert!(!use_cheap_impact_path(5, 5, true));
        assert!(!use_cheap_impact_path(9, 5, true));
    }

    #[test]
    fn cheap_path_when_no_model_even_if_large() {
        // No OpenAI key -> always deterministic, regardless of size.
        assert!(use_cheap_impact_path(50, 5, false));
    }

    #[test]
    fn threshold_is_configurable() {
        // Raising the threshold keeps a mid-size symbol on the cheap path.
        assert!(use_cheap_impact_path(9, 20, true));
        // Lowering it (to 0) forces the LLM path for any symbol.
        assert!(!use_cheap_impact_path(1, 0, true));
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
            large["response_size_hint"]["approx_tokens"]
                .as_u64()
                .unwrap()
                > small["response_size_hint"]["approx_tokens"]
                    .as_u64()
                    .unwrap()
        );
    }

    #[test]
    fn get_file_hint_prefers_file_path_but_falls_back_to_path_scope() {
        let mut args = serde_json::Map::new();
        args.insert("file_path".to_string(), serde_json::json!("src/auth.ts"));
        args.insert("path_scope".to_string(), serde_json::json!("src/other.ts"));
        assert_eq!(get_file_hint(&args), Some("src/auth.ts"));

        let mut alias_only = serde_json::Map::new();
        alias_only.insert(
            "path_scope".to_string(),
            serde_json::json!("src/rooms/route.ts"),
        );
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
fn check_symbol_ambiguity(
    db: &storage::Database,
    symbol: &str,
    file_hint: Option<&str>,
) -> Option<String> {
    let candidates = query::engine::find_symbol_candidates(db, symbol).ok()?;
    let filtered: Vec<&query::engine::SymbolCandidate> = match file_hint {
        Some(hint) => candidates
            .iter()
            .filter(|c| c.file_path.contains(hint))
            .collect(),
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

/// Runs `codebroker init` rooted at `project_dir` (via current_dir), rather than
/// inheriting the MCP server process's own CWD. Previously the auto-init hook
/// spawned the indexer without pinning its working directory, so it could index
/// whatever directory the MCP process happened to be launched from instead of
/// the workspace that `set_workspace` / `active_project` actually pointed to,
/// leaving the intended workspace's database empty (0 edges) or never indexed.
fn run_index(project_dir: &str) -> Result<String, String> {
    let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let parent = current_exe
        .parent()
        .ok_or_else(|| "Could not resolve codebroker-mcp binary directory".to_string())?;
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
        Err(format!(
            "Indexer exited with non-zero status ({}) while indexing {}",
            status, project_dir
        ))
    }
}

/// Runs `codebroker reindex-incremental <changed_paths...>` rooted at `project_dir`,
/// re-parsing only the given files instead of paying for a full repository
/// rebuild. See indexer::reindex::reindex_paths for what this intentionally
/// trades away (alias/route/prop-type linking) in exchange for speed.
fn run_incremental_index(project_dir: &str, changed_paths: &[String]) -> Result<String, String> {
    let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let parent = current_exe
        .parent()
        .ok_or_else(|| "Could not resolve codebroker-mcp binary directory".to_string())?;
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

    let status = cmd.status().map_err(|e| {
        format!(
            "Failed to spawn incremental indexer process in {}: {}",
            project_dir, e
        )
    })?;

    if status.success() {
        Ok(format!(
            "Incremental reindex complete for {} file(s) in workspace: {}",
            changed_paths.len(),
            project_dir
        ))
    } else {
        Err(format!(
            "Incremental indexer exited with non-zero status ({}) while indexing {} file(s) in {}",
            status,
            changed_paths.len(),
            project_dir
        ))
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

        if project_dir.is_empty() || project_dir == home_dir || project_path_buf.parent().is_none()
        {
            eprintln!(
                "Refusing to auto-initialize codebroker in home or root directory to prevent massive indexing."
            );
        } else {
            eprintln!(
                "No index found for workspace '{}'. Auto-initializing...",
                project_dir
            );
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
                                },
                                "instructions": default_instructions
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
                                        "name": "generate_context_capsule",
                                        "description": "One-shot discovery tool. Given a symbol, feature, or natural-language query, finds the best matching symbols, retrieves their implementations, and expands to immediate callers/callees. Returns a single token-efficient Markdown context bundle with full pivot implementations and skeletonized supporting code. Use this first for understanding a feature before falling back to lower-level search or graph tools. Matching is deterministic keyword-based first; if that finds nothing, falls back to embedding-based semantic search (requires symbols to have been embedded at index time, i.e. OPENAI_API_KEY set during indexing/reindexing) so purely conceptual queries with no literal vocabulary overlap can still resolve.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "query": {
                                                    "type": "string",
                                                    "description": "Symbol name, feature, or natural-language description of what to find."
                                                },
                                                "file_path": {
                                                    "type": "string",
                                                    "description": "Optional. Substring of the defining file's path, used to disambiguate when 'query' matches multiple definitions. 'path_scope' is also accepted as an alias."
                                                }
                                            },
                                            "required": ["query"]
                                        }
                                    },
                                    {
                                        "name": "project_overview",
                                        "description": "Fast deterministic repository overview. Returns file, symbol, edge, and language counts, per-directory density metrics (with total_directories/directories_truncated so nothing is silently dropped), and repo-wide entrypoints (API routes + pages/layouts). Use for navigation and architectural discovery.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {}
                                        }
                                    },
                                    {
                                        "name": "get_workspace",
                                        "description": "Returns the active CodeBroker workspace, including project root, database path, and indexing status.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {}
                                        }
                                    },
                                    {
                                        "name": "set_workspace",
                                        "description": "Switches the active workspace. Automatically initializes and indexes the target workspace if required.",
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
                                        "description": "Rebuilds repository symbols and dependency graphs. Supports full reindexing or targeted updates for changed files. Use after structural repository changes or when graph data becomes stale.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "absolute_path": { "type": "string", "description": "Optional. Absolute path to re-index. Defaults to the currently active workspace." },
                                                "changed_paths": { "type": "array", "items": { "type": "string" }, "description": "Optional. Specific file paths (absolute or relative to the workspace root) to incrementally re-parse instead of doing a full rebuild." }
                                            }
                                        }
                                    },
                                    {
                                        "name": "subsystem_stats",
                                        "description": "Deterministically discovers a subsystem and reports its files, symbols, dependencies, consumers, routes, and entrypoints. No AI required.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "subsystem_name": { "type": "string", "description": "Subsystem name to discover — e.g. a folder name like 'auth' or 'billing'. Matched as a substring, not an exact path." }
                                            },
                                            "required": ["subsystem_name"]
                                        }
                                    },

                                    {
                                        "name": "list_entrypoints",
                                        "description": "Repo-wide enumeration of every entrypoint (API routes/endpoints and Next.js page/layout files) with no subsystem name required. Use this for 'what are this repo's entrypoints' questions instead of guessing subsystem names to feed into subsystem_stats.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "path_scope": { "type": "string", "description": "Optional substring to restrict results to files whose path contains it." }
                                            }
                                        }
                                    },
                                    {
                                        "name": "subsystem_communication",
                                        "description": "Diffs two subsystems' edge sets to answer 'how do A and B communicate': counts of edges from A's symbols to B's and vice versa, with example symbol pairs per direction. Use instead of manually calling subsystem_stats twice and diffing the results.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "subsystem_a": { "type": "string", "description": "First subsystem name (matched the same way as subsystem_stats)." },
                                                "subsystem_b": { "type": "string", "description": "Second subsystem name." }
                                            },
                                            "required": ["subsystem_a", "subsystem_b"]
                                        }
                                    },
                                    {
                                        "name": "architectural_hotspots",
                                        "description": "Identifies highly connected and heavily depended-on code. Useful for locating critical files, shared infrastructure, and architectural bottlenecks.",
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
                                        "description": "Detects circular dependencies in the repository graph. Reports cross-file cycles by default and can optionally include same-file cycles. Ignores self-recursive functions.",
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
                                        "name": "get_context",
                                        "description": "Returns deterministic graph context for a symbol, including callers, callees, dependencies, siblings, and related symbols. Optionally includes source code.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": { "type": "string", "description": "Symbol name to look up." },
                                                "file_path": { "type": "string", "description": "Optional substring of the file path to disambiguate an ambiguous symbol name." },
                                                "include_source": { "type": "boolean", "description": "Default false. Include the symbol's own source body in the response." },
                                                "format": { "type": "string", "enum": ["json", "markdown"], "description": "Default \"json\". Set to \"markdown\" to return a condensed, token-light bulleted list instead of raw JSON." }
                                            },
                                            "required": ["symbol"]
                                        }
                                    },
                                    {
                                        "name": "impact_analysis",
                                        "description": "Estimates the blast radius of changing a symbol. Returns dependency relationships and risk information, with optional AI-generated analysis for larger dependency graphs.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": { "type": "string", "description": "Symbol name to analyze." },
                                                "file_path": { "type": "string", "description": "Optional substring of the file path to disambiguate an ambiguous symbol name." },
                                                "format": { "type": "string", "enum": ["prose", "structured", "json"], "description": "Default \"prose\". \"structured\"/\"json\" always return the deterministic graph data, skipping the LLM." },
                                                "risk_threshold": { "type": "number", "description": "Default 5. Total dependency count below which the cheap deterministic path is used instead of an LLM call." }
                                            },
                                            "required": ["symbol"]
                                        }
                                    },
                                    {
                                        "name": "search_codebase",
                                        "description": "Deterministic keyword search across symbols, file paths, and optionally file contents. Supports exact, substring, and light stemming matches. When the keyword/token match is empty or all-Low-confidence, automatically falls back to embedding-based semantic search over symbols embedded at index time (requires OPENAI_API_KEY to have been set when the workspace was indexed/reindexed) — results from that fallback are labeled \"Semantic Match\" in the confidence field.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "keyword": { "type": "string", "description": "Search term, symbol name fragment, or natural-language phrase." },
                                                "path_scope": { "type": "string", "description": "Optional substring to restrict results to files whose path contains it." },
                                                "mode": { "type": "string", "enum": ["symbol", "text", "both"], "description": "Default \"symbol\". \"text\" greps indexed file content; \"both\" tries symbol names first and falls back to text." },
                                                "whole_word": { "type": "boolean", "description": "Default false. Require a whole-word match rather than substring, for text/both modes." },
                                                "min_confidence": { "type": "string", "description": "Optional minimum confidence tier (\"Low\"/\"Medium\"/\"High\") to filter results by." },
                                                "include_source": { "type": "boolean", "description": "Default false. If true, fetches and embeds the source code for the top 1-2 matches." }
                                            },
                                            "required": ["keyword"]
                                        }
                                    },
                                    {
                                        "name": "find_symbol",
                                        "description": "Finds symbol definitions using exact, prefix, substring, or fuzzy matching. Returns locations, metadata, and optional source previews.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": { "type": "string", "description": "Symbol name to find." },
                                                "context_lines": { "type": "number", "description": "Default 3. Lines of source preview around each match; 0 disables previews." },
                                                "path_scope": { "type": "string", "description": "Optional substring to restrict matches to files whose path contains it." }
                                            },
                                            "required": ["symbol"]
                                        }
                                    },
                                    {
                                        "name": "repository_stats",
                                        "description": "Returns file, symbol, edge, and language statistics for a repository or repository subsection, plus entrypoints (API routes + pages/layouts) scoped the same way as the stats themselves.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "path_scope": { "type": "string", "description": "Optional substring to restrict the stats to files whose path contains it. Omit for repo-wide stats (same as project_overview)." }
                                            }
                                        }
                                    },
                                    {
                                        "name": "read_symbol_source",
                                        "description": "Reads the exact source code for one or more symbols. Supports dependency expansion and batched symbol retrieval.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": { "type": "string", "description": "Single symbol name to read." },
                                                "symbols": { "type": "array", "items": { "type": "string" }, "description": "Batch form: multiple symbol names to read in one call." },
                                                "file_path": { "type": "string", "description": "Optional substring of the file path to disambiguate an ambiguous symbol name." },
                                                "include_dependencies": { "type": "boolean", "description": "Default false. Also include source for immediate forward dependencies." }
                                            }
                                        }
                                    },
                                    {
                                        "name": "read_file_skeleton",
                                        "description": "Returns a structure-only view of a file with implementations collapsed. Use for fast file comprehension without reading full source.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "file_path": { "type": "string", "description": "Path (or substring) of the file to skeletonize." },
                                                "target_symbol": { "type": "string", "description": "Optional symbol name within the file to leave fully expanded." }
                                            },
                                            "required": ["file_path"]
                                        }
                                    },
                                    {
                                        "name": "explore_graph",
                                        "description": "Performs breadth-first traversal of the dependency graph from a symbol. Use to understand nearby relationships and dependency neighborhoods.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": { "type": "string", "description": "Root symbol name to explore from." },
                                                "depth": { "type": "number", "description": "Default 2, capped at 5. How many hops to traverse." },
                                                "direction": { "type": "string", "enum": ["both", "incoming", "outgoing"], "description": "Default \"both\". Restrict traversal to incoming (callers) or outgoing (callees) edges only." },
                                                "max_nodes": { "type": "number", "description": "Default 100, capped at 200. Caps the returned node count." },
                                                "format": { "type": "string", "enum": ["json", "markdown"], "description": "Default \"json\". Set to \"markdown\" to return a condensed, token-light bulleted list instead of raw JSON." }
                                            },
                                            "required": ["symbol"]
                                        }
                                    },
                                    {
                                        "name": "shortest_path",
                                        "description": "Finds the shortest dependency path between two symbols. Useful for understanding how components are connected. If `from` or `to` matches multiple symbols, returns an ambiguity response listing candidates instead of guessing.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "from": { "type": "string", "description": "Starting symbol name." },
                                                "to": { "type": "string", "description": "Target symbol name." },
                                                "from_file_path": { "type": "string", "description": "Optional substring of the defining file's path, used to disambiguate when 'from' matches multiple definitions." },
                                                "to_file_path": { "type": "string", "description": "Optional substring of the defining file's path, used to disambiguate when 'to' matches multiple definitions." }
                                            },
                                            "required": ["from", "to"]
                                        }
                                    },
                                    {
                                        "name": "find_duplicate_logic",
                                        "description": "Detects exact and near-duplicate code across symbols. Useful for identifying copy-pasted logic and refactoring opportunities.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "min_length": { "type": "number", "description": "Default 80. Minimum character length of a block to be considered for duplicate detection." },
                                                "path_scope": { "type": "string", "description": "Optional substring to restrict the scan to files whose path contains it." }
                                            }
                                        }
                                    },
                                    {
                                        "name": "graph_subtree",
                                        "description": "Returns the downstream dependency subtree rooted at a symbol. Use when you need the full dependency hierarchy rather than a local graph neighborhood.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "root_symbol": { "type": "string", "description": "Root symbol name." },
                                                "depth": { "type": "number", "description": "Default 3, capped at 5. How many hops to traverse outward. Start at 1-2 for a hub symbol (e.g. a widely-imported helper) to avoid truncation." },
                                                "max_nodes": { "type": "number", "description": "Default 100, capped at 500. Caps the returned node count (edges are capped at 3x this)." },
                                                "format": { "type": "string", "enum": ["json", "markdown"], "description": "Default \"json\". Set to \"markdown\" to return a condensed, token-light bulleted list instead of raw JSON." }
                                            },
                                            "required": ["root_symbol"]
                                        }
                                    },
                                    {
                                        "name": "read_file_snippet",
                                        "description": "Reads a raw line range directly from a file. Useful when exact file locations are already known.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "path": { "type": "string", "description": "File path (resolved against the workspace root)." },
                                                "start_line": { "type": "number", "description": "Default 1. 1-indexed start line." },
                                                "end_line": { "type": "number", "description": "Default 1. 1-indexed end line (inclusive)." }
                                            },
                                            "required": ["path"]
                                        }
                                    },
                                    {
                                        "name": "get_implementation",
                                        "description": "Returns a symbol's source code together with its full deterministic graph context. Equivalent to `read_symbol_source` and `get_context` combined.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": { "type": "string", "description": "Symbol name to look up." },
                                                "file_path": { "type": "string", "description": "Optional substring of the file path to disambiguate an ambiguous symbol name." }
                                            },
                                            "required": ["symbol"]
                                        }
                                    },
                                    {
                                        "name": "get_edit_context",
                                        "description": "Returns the source code and dependency context required to safely modify a symbol. Includes edit boundaries, dependencies, and related callers. Read-only.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "symbol": { "type": "string", "description": "Symbol name you intend to edit." },
                                                "file_path": { "type": "string", "description": "Optional substring of the file path to disambiguate an ambiguous symbol name." }
                                            },
                                            "required": ["symbol"]
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
                        let arguments = params
                            .get("arguments")
                            .and_then(|a| a.as_object())
                            .unwrap_or(default_args.as_object().unwrap());

                        eprintln!("AI Agent requested tool execution: {}", tool_name);

                        // Re-resolve active workspace for each call
                        let db_path = resolve_db_path();
                        let mut estimated_raw_context_tokens = 0;
                        let mut cache_hit = false;
                        let start_time = std::time::Instant::now();
                        let model_used = "Claude Desktop";

                        let tool_result = match std::panic::catch_unwind(
                            std::panic::AssertUnwindSafe(|| {
                                match tool_name {
                            "get_context" => {
                                let symbol = arguments.get("symbol").and_then(|s| s.as_str()).unwrap_or("");
                                let file_hint = get_file_hint(&arguments);
                                // Bundles the symbol's own source body inline on request, so the
                                // common "show me this function and what touches it" case is one
                                // call instead of get_context + a separate read_symbol_source.
                                let include_source = arguments.get("include_source").and_then(|s| s.as_bool()).unwrap_or(false);
                                let format_opt = arguments.get("format").and_then(|s| s.as_str()).unwrap_or("json");
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        if let Some(amb) = check_symbol_ambiguity(&db, symbol, file_hint) {
                                            amb
                                        } else {
                                            estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_graph_context(&db);
                                            // Self-heals a stale defining file (incrementally
                                            // reindexes it, then re-assembles) instead of serving
                                            // or silently flagging stale data — see
                                            // semantic::staleness for why this matters for the
                                            // "edit, then immediately ask about it" workflow.
                                            match query::context::ContextObject::assemble_scoped(&db, symbol, file_hint).map_err(|e| e.to_string())
                                                .and_then(|opt| match opt {
                                                    Some(c) if c.stale => semantic::staleness::assemble_context_self_healing(&db, symbol, file_hint),
                                                    other => Ok(other),
                                                }) {
                                                Ok(Some(context)) => {
                                                    if format_opt == "markdown" {
                                                        let mut md = format!("### Context for {}\n\n```json\n{}\n```\n", symbol, serde_json::to_string_pretty(&context).unwrap_or_default());
                                                        if include_source {
                                                            let source = query::retrieval::read_symbol_source_scoped(&db, symbol, false, file_hint).unwrap_or_default();
                                                            if !source.is_empty() {
                                                                let source_strs: Vec<String> = source.into_iter().map(|s| s.source).collect();
                                                                md.push_str("\n#### Source\n```\n");
                                                                md.push_str(&source_strs.join("\n"));
                                                                md.push_str("\n```\n");
                                                            }
                                                        }
                                                        md
                                                    } else {
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
                                // "structured" always skips the LLM and returns the full get_context
                                // graph JSON. Default ("prose") now also has a cheap path: for a
                                // low-fan-in/out symbol (total dependencies below `risk_threshold`)
                                // the LLM adds little over the deterministic graph, so we return a
                                // compact {risk_level, callers, callees, reason} instead — instant
                                // and zero model cost. Larger symbols still get the LLM narrative.
                                let format = arguments.get("format").and_then(|s| s.as_str()).unwrap_or("prose");
                                let risk_threshold = arguments.get("risk_threshold").and_then(|n| n.as_u64()).unwrap_or(5) as usize;
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        if let Some(amb) = check_symbol_ambiguity(&db, symbol, file_hint) {
                                            amb
                                        } else {
                                            // Self-heals a stale defining file instead of just
                                            // warning: `get_edit_context` already refuses/self-heals
                                            // on stale boundaries, and serving a
                                            // "LOW risk, 2 dependencies" cheap-path verdict built on
                                            // stale byte offsets is actively misleading even with a
                                            // warning attached if the caller doesn't act on it. If
                                            // the incremental reindex itself fails, falls back to
                                            // the stale context with a loud warning rather than
                                            // erroring outright.
                                            let first_pass = query::context::ContextObject::assemble_scoped(&db, symbol, file_hint).unwrap_or_default();
                                            let was_stale = first_pass.as_ref().map(|c| c.stale).unwrap_or(false);
                                            let context = if was_stale {
                                                semantic::staleness::assemble_context_self_healing(&db, symbol, file_hint)
                                                    .unwrap_or_else(|_| first_pass.clone())
                                            } else {
                                                first_pass
                                            };
                                            let still_stale = context.as_ref().map(|c| c.stale).unwrap_or(false);
                                            let stale_warning = (was_stale && still_stale).then(|| format!(
                                                "WARNING: '{}' has been modified on disk since it was indexed, and the automatic incremental reindex failed. The dependency data below may be computed against stale symbol boundaries. Run reindex_workspace before trusting this analysis.\n\n",
                                                symbol
                                            ));
                                            if format == "structured" || format == "json" {
                                                let mut out = serde_json::to_string_pretty(&context).unwrap_or_default();
                                                if let Some(w) = &stale_warning {
                                                    out = format!("{}{}", w, out);
                                                }
                                                out
                                            } else {
                                                let fwd = context.as_ref().map(|c| c.forward_dependencies.len()).unwrap_or(0);
                                                let rev = context.as_ref().map(|c| c.reverse_dependencies.len()).unwrap_or(0);
                                                let total_dependencies = fwd + rev;
                                                let openai_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();

                                                // Cheap path: below threshold (or no model available) ->
                                                // deterministic graph-derived output, no LLM call.
                                                if use_cheap_impact_path(total_dependencies, risk_threshold, !openai_key.is_empty()) {
                                                    let mut reason = if total_dependencies < risk_threshold {
                                                        format!("Dependency count ({}) below threshold ({}); returned deterministic graph data instead of an LLM analysis. Raise risk_threshold or use format:\"structured\" for the full graph.", total_dependencies, risk_threshold)
                                                    } else {
                                                        "OPENAI_API_KEY not set; returned deterministic graph data instead of an LLM analysis.".to_string()
                                                    };
                                                    if still_stale {
                                                        reason = format!("STALE DATA WARNING: this symbol's file has changed on disk since indexing, and the automatic incremental reindex failed; the dependency counts below may be wrong. Reindex before trusting this. {}", reason);
                                                    }
                                                    let payload = serde_json::json!({
                                                        "symbol": symbol,
                                                        "stale": still_stale,
                                                        "risk_level": "LOW",
                                                        "total_dependencies": total_dependencies,
                                                        "threshold": risk_threshold,
                                                        "callers": context.as_ref().map(|c| c.callers.clone()).unwrap_or_default(),
                                                        "callees": context.as_ref().map(|c| c.callees.clone()).unwrap_or_default(),
                                                        "forward_dependencies": context.as_ref().map(|c| c.forward_dependencies.clone()).unwrap_or_default(),
                                                        "reverse_dependencies": context.as_ref().map(|c| c.reverse_dependencies.clone()).unwrap_or_default(),
                                                        "llm_used": false,
                                                        "reason": reason,
                                                    });
                                                    serde_json::to_string_pretty(&payload).unwrap_or_default()
                                                } else {
                                                    // Complex symbol: existing LLM workflow.
                                                    estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_graph_context(&db);
                                                    let provider = Box::new(semantic::openai::OpenAiProvider::new(openai_key));
                                                    let generator = semantic::generator::SummaryGenerator::new(&db, provider);
                                                    match generator.generate_scoped(symbol, file_hint) {
                                                        Ok((summary, hit)) => {
                                                            cache_hit = hit;
                                                            match &stale_warning {
                                                                Some(w) => format!("{}{}", w, summary),
                                                                None => summary,
                                                            }
                                                        },
                                                        Err(e) => format!("Error generating impact analysis: {}", e),
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "search_codebase" => {
                                let keyword = arguments.get("keyword").and_then(|s| s.as_str()).unwrap_or("");
                                let path_scope = arguments.get("path_scope").and_then(|s| s.as_str());
                                let mode_str = arguments.get("mode").and_then(|s| s.as_str()).unwrap_or("symbol");
                                let mode = query::engine::SearchMode::from(mode_str);
                                let whole_word = arguments.get("whole_word").and_then(|b| b.as_bool()).unwrap_or(false);
                                let min_confidence = arguments.get("min_confidence").and_then(|s| s.as_str());
                                let include_source = arguments.get("include_source").and_then(|b| b.as_bool()).unwrap_or(false);

                                let openai_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
                                let mut llm_used = false;
                                let mut query_vector = None;
                                let semantic_tokens = if !openai_key.is_empty() {
                                    use semantic::provider::LlmProvider;
                                    let provider = semantic::openai::OpenAiProvider::new(openai_key.clone());
                                    if let Ok(mut vecs) = provider.embed_texts(&[keyword.to_string()]) {
                                        query_vector = vecs.pop();
                                    }
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
                                        match query::engine::search_symbols(&db, keyword, &semantic_tokens, query_vector.as_deref(), llm_used, path_scope, mode, whole_word, min_confidence) {
                                            Ok((mut results, mut reason)) => {
                                                // Semantic search removed per architectural upgrade instructions.
                                                // Concept augmentation: independent of how weak/strong
                                                // the literal keyword match was, also check whether the
                                                // query term maps to a domain concept (auth, realtime,
                                                // notifications, database) and pull in symbols tagged
                                                // with it that the literal match missed entirely — e.g.
                                                // "auth" previously found nothing for
                                                // createClient/createAdminClient/signInWithOAuth since
                                                // none of those names or their utils/supabase/*.ts path
                                                // contain the substring "auth" (benchmark run_005).
                                                // Capped and dedup'd against existing results by
                                                // (name, path) so an already-found symbol isn't repeated.
                                                let mut seen: std::collections::HashSet<(String, String)> = results
                                                    .iter()
                                                    .map(|r| (r.name.clone(), r.path.clone()))
                                                    .collect();
                                                let mut concept_added = 0usize;
                                                for concept in query::concepts::concepts_matching_term(keyword) {
                                                    if concept_added >= 10 {
                                                        break;
                                                    }
                                                    if let Ok(matches) = query::concepts::symbols_for_concept(&db, concept) {
                                                        for m in matches {
                                                            if concept_added >= 10 {
                                                                break;
                                                            }
                                                            let key = (m.symbol_name.clone(), m.file_path.clone());
                                                            if !seen.insert(key) {
                                                                continue;
                                                            }
                                                            if let Some(scope) = path_scope {
                                                                if !m.file_path.contains(scope) {
                                                                    continue;
                                                                }
                                                            }
                                                            results.push(query::engine::SearchResult {
                                                                path: m.file_path,
                                                                name: m.symbol_name,
                                                                kind: m.symbol_kind,
                                                                score: 150,
                                                                confidence: format!("Concept Match ({})", m.concept),
                                                                line: None,
                                                            });
                                                            concept_added += 1;
                                                        }
                                                    }
                                                }

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
                                                // per-match source previews for fuzzy matches to save
                                                // context, but always keep previews for exact matches.
                                                const PREVIEW_SUPPRESSION_THRESHOLD: usize = 5;
                                                let compact = results.len() > PREVIEW_SUPPRESSION_THRESHOLD && path_scope.is_none();
                                                let query_lower = symbol.to_lowercase();

                                                let mut matches = Vec::new();
                                                let mut exact_matches = Vec::new();
                                                let mut fuzzy_matches = Vec::new();
                                                for (name, path, kind, line, preview, score) in results.iter() {
                                                    let is_exact = name.to_lowercase() == query_lower;
                                                    let show_preview = context_lines > 0 && (is_exact || !compact);

                                                    let mut entry = serde_json::json!({
                                                        "name": name,
                                                        "path": path,
                                                        "kind": kind,
                                                        "line": line,
                                                    });
                                                    if show_preview {
                                                        entry["preview"] = serde_json::Value::String(preview.to_string());
                                                    }

                                                    let mut full = entry.clone();
                                                    full["score"] = serde_json::json!(score);
                                                    matches.push(full);
                                                    if is_exact {
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

                                                if compact && !fuzzy_matches.is_empty() {
                                                    payload["compact"] = serde_json::Value::Bool(true);
                                                    payload["hint"] = serde_json::Value::String(format!(
                                                        "{} total matches for \"{}\". Previews for fuzzy matches suppressed. Narrow with path_scope to see all previews.",
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
                                                // Repo-wide entrypoints (API routes + pages/layouts),
                                                // surfaced directly here instead of requiring a
                                                // separate list_entrypoints call to answer "what are
                                                // this repo's entrypoints" — previously only ever
                                                // discoverable once a subsystem name was already known
                                                // (benchmark run_001's gap #1).
                                                if let Ok(report) = query::subsystem::list_entrypoints(&db, None) {
                                                    output["entrypoints"] = serde_json::to_value(&report).unwrap_or(serde_json::Value::Null);
                                                }
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
                                                let embedded_symbols: i64 = db.conn.query_row(
                                                    "SELECT COUNT(*) FROM symbol_embeddings",
                                                    [],
                                                    |r| r.get(0),
                                                ).unwrap_or(0);
                                                let entrypoints = query::subsystem::list_entrypoints(&db, path_scope).ok();
                                                let stats = serde_json::json!({
                                                    "path_scope": path_scope,
                                                    "files": overview.files,
                                                    "symbols": overview.symbols,
                                                    "edges": overview.edges,
                                                    "languages": overview.languages,
                                                    "embedded_symbols": embedded_symbols,
                                                    "semantic_search_available": embedded_symbols > 0,
                                                    "entrypoints": entrypoints,
                                                });
                                                serde_json::to_string_pretty(&stats).unwrap_or_default()
                                            }
                                            Err(e) => format!("Error fetching stats: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
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
                                let (stale_files, files_checked) = if resolved.exists {
                                    storage::Database::new(&resolved.db_path)
                                        .ok()
                                        .and_then(|db| db.count_stale_files(2000).ok())
                                        .unwrap_or((0, 0))
                                } else {
                                    (0, 0)
                                };
                                serde_json::json!({
                                    "project_root": resolved.project_root,
                                    "db_path": resolved.db_path,
                                    "indexed": resolved.exists,
                                    "stale_files": stale_files,
                                    "stale_files_scan_limit": files_checked,
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
                                                let staleness_note = match storage::Database::new(&new_db_path) {
                                                    Ok(db) => match db.count_stale_files(2000) {
                                                        Ok((stale, _checked)) if stale > 0 => format!(
                                                            " Warning: {} indexed file(s) have changed on disk since the last index — call reindex_workspace before relying on line numbers, get_edit_context, or impact_analysis.",
                                                            stale
                                                        ),
                                                        _ => String::new(),
                                                    },
                                                    Err(_) => String::new(),
                                                };
                                                format!("Workspace successfully updated to {}. All subsequent tools will query this database.{}", path, staleness_note)
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
                                let format_opt = arguments.get("format").and_then(|s| s.as_str()).unwrap_or("json");

                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_graph_context(&db);
                                        match query::graph::explore_graph(&db, symbol, depth, direction, max_nodes) {
                                            Ok(res) => {
                                                if format_opt == "markdown" {
                                                    res.to_markdown()
                                                } else {
                                                    serde_json::to_string_pretty(&res).unwrap_or_else(|_| "Error serializing graph".to_string())
                                                }
                                            },
                                            Err(e) => format!("Error exploring graph: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "shortest_path" => {
                                let from_symbol = arguments.get("from").and_then(|s| s.as_str()).unwrap_or("");
                                let to_symbol = arguments.get("to").and_then(|s| s.as_str()).unwrap_or("");
                                let from_file_hint = arguments.get("from_file_path").and_then(|s| s.as_str());
                                let to_file_hint = arguments.get("to_file_path").and_then(|s| s.as_str());

                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        // Ambiguity on either endpoint silently mis-resolves to
                                        // whichever same-named symbol the DB happens to return
                                        // first — a real path could then look like `found: false`
                                        // simply because the wrong node was searched from/to.
                                        if let Some(amb) = check_symbol_ambiguity(&db, from_symbol, from_file_hint) {
                                            amb
                                        } else if let Some(amb) = check_symbol_ambiguity(&db, to_symbol, to_file_hint) {
                                            amb
                                        } else {
                                            match query::graph::shortest_path(&db, from_symbol, to_symbol, from_file_hint, to_file_hint) {
                                                Ok(res) => {
                                                    estimated_raw_context_tokens = res.nodes.len() * 50; // rough estimation
                                                    serde_json::to_string_pretty(&res).unwrap_or_else(|_| "Error serializing path".to_string())
                                                },
                                                Err(e) => format!("Error finding shortest path: {}", e),
                                            }
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "list_entrypoints" => {
                                let path_scope = arguments.get("path_scope").and_then(|s| s.as_str());
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        match query::subsystem::list_entrypoints(&db, path_scope) {
                                            Ok(res) => serde_json::to_string_pretty(&res).unwrap_or_else(|_| "Error serializing entrypoints".to_string()),
                                            Err(e) => format!("Error listing entrypoints: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "subsystem_communication" => {
                                let subsystem_a = arguments.get("subsystem_a").and_then(|s| s.as_str()).unwrap_or("");
                                let subsystem_b = arguments.get("subsystem_b").and_then(|s| s.as_str()).unwrap_or("");
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        match query::subsystem::subsystem_communication(&db, subsystem_a, subsystem_b) {
                                            Ok(res) => serde_json::to_string_pretty(&res).unwrap_or_else(|_| "Error serializing subsystem communication".to_string()),
                                            Err(e) => format!("Error computing subsystem communication: {}", e),
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
                                            Err(e) => {
                                                let msg = e.to_string();
                                                if msg.contains("no such column") || msg.contains("no such table") {
                                                    format!(
                                                        "Error detecting dependency cycles: the index schema is out of date ({}). Run `reindex_workspace` and retry.",
                                                        msg
                                                    )
                                                } else {
                                                    format!("Error detecting dependency cycles: {}", msg)
                                                }
                                            }
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
                                let format_opt = arguments.get("format").and_then(|s| s.as_str()).unwrap_or("json");

                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        match query::graph::graph_subtree(&db, root_symbol, depth, max_nodes) {
                                            Ok(res) => {
                                                estimated_raw_context_tokens = res.node_count * 50; // rough estimation
                                                if format_opt == "markdown" {
                                                    res.to_markdown()
                                                } else {
                                                    serde_json::to_string_pretty(&res).unwrap_or_else(|_| "Error serializing graph subtree".to_string())
                                                }
                                            },
                                    Err(e) => format!("Error exploring graph subtree: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
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
                                            // Self-heal on a stale defining file before reading
                                            // source/dependencies — see semantic::staleness. Source
                                            // is re-read AFTER any reindex too, since edit context
                                            // is exactly the case where stale start_line/end_line
                                            // boundaries are most dangerous to act on.
                                            let first_pass = query::context::ContextObject::assemble_scoped(&db, symbol, file_hint).unwrap_or_default();
                                            let was_stale = first_pass.as_ref().map(|c| c.stale).unwrap_or(false);
                                            let context = if was_stale {
                                                semantic::staleness::assemble_context_self_healing(&db, symbol, file_hint)
                                                    .unwrap_or_else(|_| first_pass.clone())
                                            } else {
                                                first_pass
                                            };
                                            let still_stale = context.as_ref().map(|c| c.stale).unwrap_or(false);
                                            let source = query::retrieval::read_symbol_source_scoped(&db, symbol, false, file_hint).unwrap_or_default();
                                            let mut edit_context = serde_json::json!({
                                                "target_implementation": source,
                                                "forward_dependencies": context.as_ref().map(|c| c.forward_dependencies.clone()).unwrap_or_default(),
                                                "reverse_dependencies": context.as_ref().map(|c| c.reverse_dependencies.clone()).unwrap_or_default(),
                                                "same_file_callers": context.as_ref().map(|c| c.same_file_callers.clone()).unwrap_or_default(),
                                                "suggested_edit_boundaries": "Use start_line and end_line from target_implementation"
                                            });
                                            if still_stale {
                                                edit_context["warning"] = serde_json::Value::String(format!(
                                                    "'{}' has been modified on disk since indexing, and the automatic incremental reindex failed. start_line/end_line and dependency data above may not match the current file. Run reindex_workspace before trusting this.",
                                                    symbol
                                                ));
                                            }
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
                                        let openai_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
                                        let mut query_vector = None;
                                        let semantic_tokens = if !openai_key.is_empty() {
                                            use semantic::provider::LlmProvider;
                                            let provider = semantic::openai::OpenAiProvider::new(openai_key.clone());
                                            if let Ok(mut vecs) = provider.embed_texts(&[name.to_string()]) {
                                                query_vector = vecs.pop();
                                            }
                                            match provider.expand_query(name, 5) {
                                                Ok((tokens, _)) => tokens,
                                                Err(_) => vec![],
                                            }
                                        } else {
                                            vec![]
                                        };
                                        match query::subsystem::discover_subsystem(&db, name, &semantic_tokens, query_vector.as_deref()) {
                                            Ok(stats) => serde_json::to_string_pretty(&stats).unwrap_or_default(),
                                            Err(e) => format!("Error discovering subsystem: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }

                            "generate_context_capsule" => {
                                let query_str = arguments.get("query").and_then(|s| s.as_str()).unwrap_or("");
                                let file_hint = get_file_hint(&arguments);
                                if query_str.is_empty() {
                                    "Error: 'query' is required".to_string()
                                } else {
                                    match storage::Database::new(&db_path) {
                                        Ok(db) => {
                                            estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_graph_context(&db);
                                            let openai_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
                                            let mut query_vector = None;
                                            let semantic_tokens = if !openai_key.is_empty() {
                                                use semantic::provider::LlmProvider;
                                                let provider = semantic::openai::OpenAiProvider::new(openai_key.clone());
                                                if let Ok(mut vecs) = provider.embed_texts(&[query_str.to_string()]) {
                                                    query_vector = vecs.pop();
                                                }
                                                match provider.expand_query(query_str, 5) {
                                                    Ok((tokens, _)) => tokens,
                                                    Err(_) => vec![],
                                                }
                                            } else {
                                                vec![]
                                            };
                                            generate_context_capsule(&db, query_str, file_hint, &semantic_tokens, query_vector.as_deref())
                                        }
                                        Err(_) => "Error connecting to db".to_string(),
                                    }
                                }
                            }
                            _ => {
                                serde_json::json!({"success": false, "error": format!("Tool {} not recognized", tool_name)}).to_string()
                            }
                        }
                            }),
                        ) {
                            Ok(res) => res,
                            Err(panic_err) => {
                                let msg = if let Some(s) = panic_err.downcast_ref::<&str>() {
                                    s.to_string()
                                } else if let Some(s) = panic_err.downcast_ref::<String>() {
                                    s.clone()
                                } else {
                                    "Unknown panic".to_string()
                                };
                                serde_json::json!({ "success": false, "error": format!("Tool execution panicked: {}", msg) }).to_string()
                            }
                        };

                        // Backstop: regardless of per-tool pagination/limit params, never hand the
                        // client a payload so large it blows the MCP transport's token limit.
                        const MAX_TOOL_RESULT_CHARS: usize = 90_000;
                        let tool_result = if tool_result.len() > MAX_TOOL_RESULT_CHARS {
                            let mut truncated = tool_result
                                .chars()
                                .take(MAX_TOOL_RESULT_CHARS)
                                .collect::<String>();
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
                        let delivered_token_count =
                            analytics::accounting::TokenAccounting::estimate_tokens(
                                tool_result.len(),
                            );
                        let source_lines_returned = tool_result.lines().count();

                        eprintln!(
                            "[Analytics] Tool: {}, Exec Time: {}ms, Lines: {}, Tokens: {}, Cache Hit: {}",
                            tool_name,
                            execution_time_ms,
                            source_lines_returned,
                            delivered_token_count,
                            cache_hit
                        );

                        if let Ok(db) = storage::Database::new(&db_path) {
                            let collector = analytics::collector::MetricsCollector::new(&db);
                            collector.log_comprehensive_event(
                                tool_name,
                                execution_time_ms,
                                delivered_token_count,
                                estimated_raw_context_tokens,
                                cache_hit,
                                model_used,
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
                        let name = request
                            .params
                            .as_ref()
                            .and_then(|p| p.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("");

                        let message_text = match name {
                            "analyze_project" => {
                                "Please use your CodeBroker tools (such as `project_overview` and `architectural_hotspots`) to analyze the local repository connected via MCP. Give me a high-level summary of what this codebase does and how the architecture is laid out."
                            }
                            "explore_subsystem" => {
                                "Please use your CodeBroker `graph_subtree` and `subsystem_stats` tools to map out the core components of the local repository. Let me know what you find."
                            }
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
