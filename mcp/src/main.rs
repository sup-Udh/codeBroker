pub mod contracts;
pub mod tools;

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
    if let Some(active_file) = runtime::environment::active_project_path() {
        if let Ok(project_path) = std::fs::read_to_string(&active_file) {
            let project_path = project_path.trim().to_string();
            if !project_path.is_empty() {
                let db_path = std::path::Path::new(&project_path)
                    .join(".codebroker")
                    .join("codebroker.db")
                    .to_string_lossy()
                    .to_string();
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
    let project_root = std::env::var("CODEBROKER_WORKSPACE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().expect("Failed to get current dir"));

    let project_root = project_root
        .canonicalize()
        .unwrap_or(project_root)
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
/// every scoping param uses on search_codebase/find_symbol/get_context/
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

/// Decides whether get_context should take the cheap, deterministic path
/// (no LLM): true when the symbol's total dependency count is below the
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

/// `path_scope` arguments are matched against stored (indexed) paths with a
/// plain substring check in the query layer — unlike symbol/file resolution,
/// they never went through any path normalization at all. A caller passing
/// Windows-style backslashes, a leading `./`, or an absolute path prefixed
/// with the project root would silently get zero matches instead of the
/// expected results. Normalizing once here, at the dispatch boundary, fixes
/// every `path_scope`-consuming tool without touching their internal
/// `path.contains(scope)` matching.
fn normalized_path_scope(db: &storage::Database, raw: Option<&str>) -> Option<String> {
    raw.map(|s| resolver::CanonicalNameResolver::normalize_path(db, s))
}

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

fn to_string_with_hint<T: serde::Serialize>(val: &T) -> Result<String, serde_json::Error> {
    let mut value = serde_json::to_value(val)?;
    add_response_size_hint(&mut value);
    serde_json::to_string_pretty(&value)
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
/// Resolves `symbol` (optionally scoped by `file_hint`) through the shared
/// Universal Resolver (`resolver::resolve_symbol`). Every symbol-name-keyed
/// tool calls this ONE function instead of separately reimplementing
/// ambiguity detection — that duplication (each tool re-querying
/// `find_symbol_candidates` and rendering its own "ambiguous" JSON) is
/// exactly what the resolver architecture exists to remove. On a confident
/// match, returns the resolved symbol (with its now-unambiguous absolute
/// `file_path`) for the caller to act on. On `Ambiguous`/`NotFound`, returns
/// the resolver's own JSON rendering — identical across every tool — as the
/// final tool response.
fn resolve_symbol_for_tool(
    db: &storage::Database,
    symbol: &str,
    file_hint: Option<&str>,
    line_hint: Option<i64>,
) -> Result<resolver::ResolvedSymbol, String> {
    match resolver::resolve_symbol(db, symbol, file_hint, line_hint) {
        resolver::ResolvedEntity::Symbol(s) => Ok(s),
        other => Err(other.to_json_string()),
    }
}

/// Runs `codebroker init` rooted at `project_dir` (via current_dir), rather than
/// inheriting the MCP server process's own CWD. Previously the auto-init hook
/// spawned the indexer without pinning its working directory, so it could index
/// whatever directory the MCP process happened to be launched from instead of
/// the workspace that `set_workspace` / `active_project` actually pointed to,
/// leaving the intended workspace's database empty (0 edges) or never indexed.
fn run_index(project_dir: &str) -> Result<String, String> {
    // Stdio must NOT be inherited here: the MCP transport is JSON-RPC framed
    // over this same process's stdout, and a child's plain `println!` output
    // landing on that stream corrupts every subsequent response.
    let cli_path = runtime::executables::find_cli_binary().map_err(|e| e.to_string())?;
    runtime::process::run_detached(&cli_path, &["init"], std::path::Path::new(project_dir))?;
    Ok(format!("Indexing complete for workspace: {}", project_dir))
}

/// Runs `codebroker reindex-incremental <changed_paths...>` rooted at `project_dir`,
/// re-parsing only the given files instead of paying for a full repository
/// rebuild. See indexer::reindex::reindex_paths for what this intentionally
/// trades away (alias/route/prop-type linking) in exchange for speed.
fn run_incremental_index(project_dir: &str, changed_paths: &[String]) -> Result<String, String> {
    // Same reasoning as run_index: must not inherit stdout/stderr, or the
    // child's println! output corrupts the JSON-RPC stream on this fd.
    let cli_path = runtime::executables::find_cli_binary().map_err(|e| e.to_string())?;
    let mut args: Vec<&str> = vec!["reindex-incremental"];
    args.extend(changed_paths.iter().map(|p| p.as_str()));
    runtime::process::run_detached(&cli_path, &args, std::path::Path::new(project_dir))?;

    Ok(format!(
        "Incremental reindex complete for {} file(s) in workspace: {}",
        changed_paths.len(),
        project_dir
    ))
}

fn main() {
    // Resolve the active workspace (db path + the project root it belongs to)
    let resolved = resolve_workspace();

    // AUTO-INIT HOOK: If the resolved workspace has no index yet, index its
    // actual project root (not the MCP process's ambient CWD).
    if !resolved.exists {
        let project_dir = resolved.project_root.clone();
        let home_dir = runtime::environment::home_dir();
        let project_path_buf = std::path::PathBuf::from(&project_dir);

        if project_dir.is_empty()
            || home_dir.as_deref() == Some(project_path_buf.as_path())
            || project_path_buf.parent().is_none()
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
* subsystem_communication (Note: this only works with an exact leaf path from its own known_subsystems list. It is brittle and will return not_found for partial paths like "app/api" if they are not exact known leaves. Verify exact subsystem names first.)

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
* get_context
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
                                        }
                                    },
                                    {
                                        "name": "search_codebase",
                                        "description": "Keyword search across symbols, file paths, and optionally file contents (exact, substring, and light stemming matches), plus embedding-based semantic retrieval for conceptual queries. Mode \"both\" fuses keyword and semantic rankings; if embeddings are unavailable it degrades to keyword results and reports semantic_degraded_reason.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "query": { "type": "string", "description": "Search term, symbol name fragment, or natural-language phrase. (Previously 'keyword')" },
                                                "keyword": { "type": "string", "description": "Legacy alias for 'query'." },
                                                "path_scope": { "type": "string", "description": "Optional substring to restrict results to files whose path contains it." },
                                                "mode": { "type": "string", "enum": ["symbol", "text", "both", "semantic"], "description": "Default \"symbol\". \"text\" greps indexed file content; \"both\" fuses symbol+text keyword matching with semantic retrieval; \"semantic\" is embedding-only retrieval for conceptual queries." },
                                                "whole_word": { "type": "boolean", "description": "Default false. Require a whole-word match rather than substring, for text/both modes." },
                                                "include_source": { "type": "boolean", "description": "Default false. If true, fetches and embeds the source code for the top 1-2 matches." },
                                                "limit": { "type": "number", "description": "Default 15. Maximum number of search results to return." },
                                                "include_concepts": { "type": "boolean", "description": "Default false. If true, includes matches from domain concepts (e.g. 'auth' returning createClient)." }
                                            },
                                            "required": ["query"]
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
                                                "line": { "type": "number", "description": "Optional 1-based line number. When a file contains multiple definitions with the same name, picks the definition whose start_line is closest to (and not after) this line." },
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
                                                "format": { "type": "string", "enum": ["json", "markdown"], "description": "Default \"json\". Set to \"markdown\" to return a condensed, token-light bulleted list instead of raw JSON." },
                                                "file_path": { "type": "string", "description": "Optional substring of the file path to disambiguate an ambiguous root symbol name." }
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
                                    },
                                    {
                                        "name": "find_duplicate_logic",
                                        "description": "Detects exact and near-duplicate code across symbols. Useful for identifying copy-pasted logic and refactoring opportunities.",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {
                                                "min_length": { "type": "number", "description": "Default 15. Minimum AST node count a symbol's body must have to be considered — not character length. Low enough to catch small copy-pasted functions, high enough to skip trivial one-liners." },
                                                "path_scope": { "type": "string", "description": "Optional substring to restrict the scan to files whose path contains it." },
                                                "limit": { "type": "number", "description": "Optional limit to the number of duplicate groups returned." }
                                            }
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

                        if tool_name != "set_workspace" && tool_name != "reindex_workspace" {
                            if let Ok(db) = storage::Database::new(&db_path) {
                                if let Ok(stale_paths) = db.get_stale_files(2000) {
                                    if !stale_paths.is_empty() {
                                        eprintln!("Auto-reindexing {} stale file(s) before running {}", stale_paths.len(), tool_name);
                                        let _ = run_incremental_index(&db.project_root, &stale_paths);
                                    }
                                }
                            }
                        }

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
                                        match resolve_symbol_for_tool(&db, symbol, file_hint, None) {
                                        Err(resp) => resp,
                                        Ok(resolved) => {
                                            let symbol_id = resolved.id;
                                            let symbol = resolved.name.as_str();
                                            let file_hint = Some(relative_hint(&db, &resolved.file_path));
                                            estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_graph_context(&db);
                                            // Self-heals a stale defining file (incrementally
                                            // reindexes it, then re-assembles) instead of serving
                                            // or silently flagging stale data — see
                                            // semantic::staleness for why this matters for the
                                            // "edit, then immediately ask about it" workflow.
                                            let profile_str = arguments.get("profile").and_then(|s| s.as_str()).unwrap_or("standard");
                                            let profile = match profile_str {
                                                "compact" => query::response::ResponseProfile::Compact,
                                                "verbose" => query::response::ResponseProfile::Verbose,
                                                _ => query::response::ResponseProfile::Standard,
                                            };
                                            match semantic::staleness::assemble_context_self_healing_by_id(&db, Some(symbol_id), symbol, file_hint, profile).map_err(|e| e.to_string()) {
                                                Ok(Some(context)) => {
                                                    if format_opt == "markdown" {
                                                        let mut md = context.build_markdown().unwrap_or_else(|_| "Error building markdown".to_string());
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
                                                        let mut value = context.build_json().unwrap_or(serde_json::json!({}));
                                                        if include_source {
                                                            let source = query::retrieval::read_symbol_source_scoped(&db, symbol, false, file_hint).unwrap_or_default();
                                                            if let Some(obj) = value.as_object_mut() {
                                                                obj.insert("source".to_string(), serde_json::to_value(&source).unwrap_or_default());
                                                            }
                                                        }
                                                        to_string_with_hint(&value).unwrap_or_else(|_| "Error serializing context JSON".to_string())
                                                    }
                                                }
                                                Ok(None) => format!("Symbol '{}' not found in database.", symbol),
                                                Err(e) => format!("Error assembling context: {}", e),
                                            }
                                        }
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "search_codebase" => {
                                let keyword = arguments.get("query").or_else(|| arguments.get("keyword")).and_then(|s| s.as_str()).unwrap_or("");
                                let path_scope = arguments.get("path_scope").and_then(|s| s.as_str());
                                let mode_str = arguments.get("mode").and_then(|s| s.as_str()).unwrap_or("symbol");
                                let mode = query::engine::SearchMode::from(mode_str);
                                let whole_word = arguments.get("whole_word").and_then(|b| b.as_bool()).unwrap_or(false);
                                let _include_source = arguments.get("include_source").and_then(|b| b.as_bool()).unwrap_or(false);
                                let limit = arguments.get("limit").and_then(|n| n.as_u64()).unwrap_or(15) as usize;
                                let include_concepts = arguments.get("include_concepts").and_then(|b| b.as_bool()).unwrap_or(false);

                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_search_context(&db);
                                        let outcome = semantic::search::resolve_search_semantic(
                                            &db, keyword, path_scope, mode, whole_word, limit, include_concepts
                                        );
                                        let mut payload = serde_json::json!({
                                            "workspace_root": db.project_root,
                                            "total_occurrences": outcome.total_occurrences,
                                            "files_matched": outcome.files_matched,
                                            "truncated": outcome.truncated,
                                            "results": outcome.results
                                        });
                                        if let Some(r) = outcome.reason {
                                            payload["reason"] = serde_json::Value::String(r);
                                        }
                                        if let Some(available) = outcome.semantic_search_available {
                                            payload["semantic_search_available"] = serde_json::Value::Bool(available);
                                        }
                                        if let Some(why) = outcome.semantic_degraded_reason {
                                            payload["semantic_degraded_reason"] = serde_json::Value::String(why);
                                        }
                                        add_response_size_hint(&mut payload);
                                        serde_json::to_string_pretty(&payload).unwrap_or_default()
                                    }
                                    Err(_) => serde_json::json!({"success": false, "error": "Error connecting to db"}).to_string(),
                                }
                            }

                            "repository_stats" => {
                                let path_scope_raw = arguments.get("path_scope").and_then(|s| s.as_str());
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        let path_scope = normalized_path_scope(&db, path_scope_raw);
                                        let path_scope = path_scope.as_deref();
                                        estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_graph_context(&db);
                                        match query::engine::build_project_overview_scoped(&db, path_scope) {
                                            Ok(overview) => {
                                                // Real state, not a hardcoded flag: rows stored for
                                                // the ACTIVE model only — vectors left behind by a
                                                // previous model don't count as available.
                                                let embedding_model = semantic::config::EmbeddingsConfig::load(&db.project_root).model_id();
                                                let embedded_symbols = db.count_symbol_embeddings(&embedding_model).unwrap_or(0);
                                                let entrypoints = query::subsystem::list_entrypoints(&db, path_scope).ok();
                                                let stats = serde_json::json!({
                                                    "path_scope": path_scope,
                                                    "files": overview.files,
                                                    "symbols": overview.symbols,
                                                    "edges": overview.edges,
                                                    "languages": overview.languages,
                                                    "embedding_model": embedding_model,
                                                    "embedded_symbols": embedded_symbols,
                                                    "semantic_search_available": embedded_symbols > 0,
                                                    "entrypoints": entrypoints,
                                                });
                                                to_string_with_hint(&stats).unwrap_or_default()
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
                                let line_hint = arguments.get("line").and_then(|n| n.as_i64());

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
                                            // The new Phase 20 architecture:
                                            let mut combined_results = Vec::new();
                                            for symbol in &targets {
                                                let result = tools::read_symbol_source::ReadSymbolSource::execute(
                                                    &db,
                                                    symbol,
                                                    file_hint,
                                                    line_hint,
                                                    include_deps,
                                                );
                                                // If it's a JSON array (from our tool), accumulate it. Otherwise, it's an error.
                                                if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&result) {
                                                    if let Some(arr) = json_val.as_array() {
                                                        combined_results.extend(arr.clone());
                                                    } else {
                                                        // It's an error object (like Ambiguous or NotFound)
                                                        combined_results.push(json_val);
                                                    }
                                                }
                                            }
                                            to_string_with_hint(&combined_results).unwrap_or_default()
                                        }
                                        Err(_) => "Error connecting to db".to_string(),
                                    }
                                }
                            }
                            "set_workspace" => {
                                let path = arguments.get("absolute_path").and_then(|s| s.as_str()).unwrap_or("");
                                if path.is_empty() {
                                    "Error: absolute_path is required".to_string()
                                } else if !std::path::Path::new(path).exists() {
                                    format!("Error: Path '{}' does not exist.", path)
                                } else {
                                    if let Some(codebroker_dir) = runtime::environment::codebroker_dir() {
                                        let active_file = codebroker_dir.join("active_project");
                                        std::fs::create_dir_all(&codebroker_dir).unwrap_or_default();
                                        if let Err(e) = std::fs::write(&active_file, path) {
                                            format!("Error saving workspace: {}", e)
                                        } else {
                                            let new_db_path = std::path::Path::new(path)
                                                .join(".codebroker")
                                                .join("codebroker.db")
                                                .to_string_lossy()
                                                .to_string();
                                            if std::path::Path::new(&new_db_path).exists() {
                                                let staleness_note = match storage::Database::new(&new_db_path) {
                                                    Ok(db) => match db.count_stale_files(2000) {
                                                        Ok((stale, _checked)) if stale > 0 => format!(
                                                            " Warning: {} indexed file(s) have changed on disk since the last index — call reindex_workspace before relying on line numbers, get_edit_context, or get_context.",
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
                                        "Error: Could not determine the user's home directory (checked HOME, USERPROFILE, HOMEDRIVE+HOMEPATH).".to_string()
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
                                            let reindexed_db_path = std::path::Path::new(&project_dir)
                                                .join(".codebroker")
                                                .join("codebroker.db")
                                                .to_string_lossy()
                                                .to_string();
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
                                        // Path resolution (file vs. directory vs. ambiguous vs.
                                        // not-found) is the resolver's job — this tool no longer
                                        // reimplements its own "is this a directory" matching.
                                        match resolver::resolve_path(&db, path) {
                                            resolver::ResolvedEntity::File(f) => {
                                                match query::retrieval::skeletonize_file(&db, &f.file_path, target_symbol) {
                                                    Ok(res) => res,
                                                    Err(e) => format!("Error reading file skeleton: {}", e),
                                                }
                                            }
                                            resolver::ResolvedEntity::Directory(d) => format!(
                                                "Error reading file skeleton: '{}' is a directory, not a file. Indexed files in it: {}. Pass one of these as the file path.",
                                                d.directory_path,
                                                d.sample_files.join(", ")
                                            ),
                                            other => format!("Error reading file skeleton: {}", other.to_json_string()),
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
                                let file_hint = get_file_hint(&arguments);

                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        // Previously had no ambiguity guard at all (flagged in
                                        // the original bug report as a gap to audit) — root
                                        // resolution now goes through the same shared resolver
                                        // every other symbol-keyed tool uses.
                                        match resolve_symbol_for_tool(&db, symbol, file_hint, None) {
                                        Err(resp) => resp,
                                        Ok(resolved) => {
                                        let hint = relative_hint(&db, &resolved.file_path);
                                        estimated_raw_context_tokens = analytics::accounting::TokenAccounting::estimate_graph_context(&db);
                                        match query::graph::explore_graph_scoped(&db, &resolved.name, depth, direction, max_nodes, Some(hint), Some(resolved.id)) {
                                            Ok(res) => {
                                                if format_opt == "markdown" {
                                                    res.to_markdown()
                                                } else {
                                                    let profile_str = arguments.get("profile").and_then(|s| s.as_str()).unwrap_or("compact");
                                                    let profile = query::response::ResponseProfile::from(profile_str);
                                                    to_string_with_hint(&res.build_json(profile)).unwrap_or_else(|_| "Error serializing graph".to_string())
                                                }
                                            },
                                            Err(e) => format!("Error exploring graph: {}", e),
                                        }
                                        }
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
                                        // Both endpoints resolved through the shared resolver
                                        // instead of each silently mis-resolving to whichever
                                        // same-named symbol the DB happened to return first — a
                                        // real path could otherwise look like `found: false`
                                        // purely because the wrong node was searched from/to.
                                        match resolve_symbol_for_tool(&db, from_symbol, from_file_hint, None) {
                                        Err(resp) => resp,
                                        Ok(from_resolved) => {
                                        match resolve_symbol_for_tool(&db, to_symbol, to_file_hint, None) {
                                        Err(resp) => resp,
                                        Ok(to_resolved) => {
                                            let from_hint = relative_hint(&db, &from_resolved.file_path);
                                            let to_hint = relative_hint(&db, &to_resolved.file_path);
                                            match query::graph::shortest_path(&db, &from_resolved.name, &to_resolved.name, Some(from_hint), Some(to_hint), Some(from_resolved.id), Some(to_resolved.id)) {
                                                Ok(res) => {
                                                    estimated_raw_context_tokens = res.nodes.len() * 50; // rough estimation
                                                    to_string_with_hint(&res).unwrap_or_else(|_| "Error serializing path".to_string())
                                                },
                                                Err(e) => format!("Error finding shortest path: {}", e),
                                            }
                                        }
                                        }
                                        }
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
                                            Ok(res) => to_string_with_hint(&res).unwrap_or_else(|_| "Error serializing subsystem communication".to_string()),
                                            Err(e) => to_string_with_hint(&e).unwrap_or_else(|_| "Error in subsystem communication".to_string()),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "architectural_hotspots" => {
                                let limit = arguments.get("limit").and_then(|n| n.as_u64()).unwrap_or(20) as usize;
                                let path_scope_raw = arguments.get("path_scope").and_then(|s| s.as_str());

                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        let path_scope = normalized_path_scope(&db, path_scope_raw);
                                        let path_scope = path_scope.as_deref();
                                        match query::graph::architectural_hotspots(&db, limit, path_scope) {
                                            Ok(res) => {
                                                estimated_raw_context_tokens = res.top_hotspots.len() * 50; // rough estimation
                                                to_string_with_hint(&res).unwrap_or_else(|_| "Error serializing hotspots".to_string())
                                            },
                                            Err(e) => format!("Error calculating architectural hotspots: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "dependency_cycles" => {
                                let limit = arguments.get("limit").and_then(|n| n.as_u64()).unwrap_or(25) as usize;
                                let path_scope_raw = arguments.get("path_scope").and_then(|s| s.as_str());
                                let include_same_file = arguments.get("include_same_file").and_then(|b| b.as_bool()).unwrap_or(false);
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        let path_scope = normalized_path_scope(&db, path_scope_raw);
                                        let path_scope = path_scope.as_deref();
                                        match query::graph::dependency_cycles(&db, limit, path_scope, include_same_file) {
                                            Ok(res) => {
                                                estimated_raw_context_tokens = res.cycles.len() * 100; // rough estimation
                                                to_string_with_hint(&res).unwrap_or_else(|_| "Error serializing cycles".to_string())
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
                                let min_length = arguments.get("min_length").and_then(|n| n.as_u64()).unwrap_or(15) as usize;
                                let path_scope_raw = arguments.get("path_scope").and_then(|s| s.as_str());
                                let limit = arguments.get("limit").and_then(|n| n.as_u64()).map(|n| n as usize);
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        let path_scope = normalized_path_scope(&db, path_scope_raw);
                                        let path_scope = path_scope.as_deref();
                                        match query::duplicates::find_duplicate_logic(&db, min_length, path_scope, limit) {
                                            Ok(res) => {
                                                estimated_raw_context_tokens = res.groups.len() * 60; // rough estimation
                                                to_string_with_hint(&res).unwrap_or_else(|_| "Error serializing duplicate logic report".to_string())
                                            }
                                            Err(e) => format!("Error scanning for duplicate logic: {}", e),
                                        }
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
                                }
                            }
                            "read_file_snippet" => {
                                let path = arguments.get("path").and_then(|s| s.as_str()).unwrap_or("");
                                let start_line = arguments.get("start_line").and_then(|n| n.as_u64()).unwrap_or(1) as usize;
                                let end_line = arguments.get("end_line").and_then(|n| n.as_u64()).unwrap_or(1) as usize;
                                match storage::Database::new(&db_path) {
                                    Ok(db) => {
                                        match resolver::resolve_path(&db, path) {
                                            resolver::ResolvedEntity::File(f) => {
                                                match query::retrieval::read_file_snippet(&f.file_path, start_line, end_line) {
                                                    Ok(res) => to_string_with_hint(&res).unwrap_or_default(),
                                                    Err(e) => format!("Error reading file snippet: {}", e),
                                                }
                                            }
                                            resolver::ResolvedEntity::Directory(d) => format!(
                                                "Error reading file snippet: '{}' is a directory, not a file. Indexed files in it: {}. Pass one of these as the file path.",
                                                d.directory_path,
                                                d.sample_files.join(", ")
                                            ),
                                            resolver::ResolvedEntity::NotFound(_) => {
                                                // Not every readable file is indexed (README,
                                                // configs, ...) — fall back to a direct
                                                // filesystem read rather than refusing outright
                                                // just because the index doesn't know this path.
                                                let resolved_path = db.resolve_path(path);
                                                match query::retrieval::read_file_snippet(&resolved_path, start_line, end_line) {
                                                    Ok(res) => to_string_with_hint(&res).unwrap_or_default(),
                                                    Err(e) => format!("Error reading file snippet: {}", e),
                                                }
                                            }
                                            other => other.to_json_string(),
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
                                        match resolve_symbol_for_tool(&db, symbol, file_hint, None) {
                                        Err(resp) => resp,
                                        Ok(resolved) => {
                                            let symbol_id = resolved.id;
                                            let symbol = resolved.name.as_str();
                                            let file_hint = Some(relative_hint(&db, &resolved.file_path));
                                            // Self-heal on a stale defining file before reading
                                            // source/dependencies — see semantic::staleness. Source
                                            // is re-read AFTER any reindex too, since edit context
                                            // is exactly the case where stale start_line/end_line
                                            // boundaries are most dangerous to act on.
                                            let context = semantic::staleness::assemble_context_self_healing_by_id(&db, Some(symbol_id), symbol, file_hint, query::response::ResponseProfile::Verbose).unwrap_or(None);
                                            let still_stale = context.as_ref().map(|c| c.stale).unwrap_or(false);
                                            let source = query::retrieval::read_symbol_source_by_id(&db, symbol_id, false).unwrap_or_default();
                                            let mut edit_context = serde_json::json!({
                                                "target_implementation": source,
                                                "forward_dependencies": context.as_ref().map(|c| c.fetch_forward_dependencies().unwrap_or_default()).unwrap_or_default(),
                                                "reverse_dependencies": context.as_ref().map(|c| c.fetch_reverse_dependencies().unwrap_or_default()).unwrap_or_default(),
                                                "callers": context.as_ref().map(|c| c.fetch_callers().unwrap_or_default()).unwrap_or_default(),
                                                "callees": context.as_ref().map(|c| c.fetch_callees().unwrap_or_default()).unwrap_or_default(),
                                                "same_file_callers": context.as_ref().map(|c| c.fetch_same_file_callers().unwrap_or_default()).unwrap_or_default(),
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
                                    }
                                    Err(_) => "Error connecting to db".to_string(),
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
                            let prompt_str = serde_json::to_string(&arguments).unwrap_or_default();
                            let is_success = !tool_result.contains("\"success\":false") && 
                                             !tool_result.contains("\"success\": false") && 
                                             !tool_result.starts_with("Error:") && 
                                             !tool_result.contains("\"error\":");

                            let collector = analytics::collector::MetricsCollector::new(&db);
                            collector.log_comprehensive_event(
                                tool_name,
                                Some(&prompt_str),
                                is_success,
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
