use graph::models::{FileMetadata, RelationshipNode, SemanticBinding, SymbolNode};
use std::collections::HashMap;

/// Immutable result of parsing+extracting a single file, produced inside a
/// worker (rayon) thread with no database access. The caller collects a
/// `Vec<ParsedFile>`, sorts it by `path`, and only then persists it — that
/// sort is what keeps output deterministic regardless of which worker
/// finished first.
pub struct ParsedFile {
    pub path: String,
    pub content_hash: String,
    pub metadata: FileMetadata,
    pub symbols: Vec<SymbolNode>,
    pub relationships: Vec<RelationshipNode>,
    pub semantic_bindings: Vec<SemanticBinding>,
}

/// Result of checking a single file against the previous index during an
/// incremental `codebroker init`.
pub enum FileStatus {
    /// Content hash matches the previous index; nothing to reparse or
    /// re-insert — its existing rows (copied from the old database) stay
    /// exactly as they are.
    Unchanged,
    /// New or content-changed file; carries the fresh parse output ready to
    /// be inserted (after the caller deletes any stale old row for the same
    /// path, if one exists).
    Changed(ParsedFile),
}

/// Reads, hashes, and parses+extracts a single file using the matching
/// `LanguageFrontend`. Fused into one pass (rather than separate hash/parse
/// stages) so each file is only read from disk once. Returns `None` for
/// files that can't be read or that carry no matching frontend, or whose
/// frontend returns no output — mirrors the previous sequential loop's
/// `if let Ok(source) = fs::read_to_string(...) { if let Some(frontend) =
/// ... }` skip behavior exactly.
///
/// `read_path` is a filesystem-openable path (may be absolute); `logical_path`
/// is the path stored in the DB and matched against `LanguageFrontend::can_handle`
/// (always the `./`-relative form `codebroker init`'s walker produces). The
/// two differ when the caller isn't running with the project root as its
/// working directory — e.g. `reindex_paths` called from the MCP server.
pub fn read_hash_parse(
    read_path: &std::path::Path,
    logical_path: &str,
    frontends: &[Box<dyn parser::frontend::LanguageFrontend>],
) -> Option<ParsedFile> {
    let matched_frontend = frontends.iter().find(|f| f.can_handle(logical_path))?;
    let source_code = std::fs::read_to_string(read_path).ok()?;
    parse_source(matched_frontend.as_ref(), read_path, logical_path, source_code)
}

/// Like `read_hash_parse`, but first checks the file's content hash against
/// `old_hashes` (path -> content_hash from the previous index) and returns
/// `FileStatus::Unchanged` without ever calling into the parser if it
/// matches. `trust_hashes` is the `PIPELINE_VERSION` gate: when the previous
/// index was built by a different parser/schema version, hash equality can't
/// be trusted to mean "would still parse to the same output", so every file
/// is treated as changed regardless of its hash.
pub fn classify_and_parse(
    logical_path: &str,
    old_hashes: &HashMap<String, (i64, String)>,
    trust_hashes: bool,
    frontends: &[Box<dyn parser::frontend::LanguageFrontend>],
) -> Option<FileStatus> {
    let matched_frontend = frontends.iter().find(|f| f.can_handle(logical_path))?;
    let read_path = std::path::Path::new(logical_path);
    let source_code = std::fs::read_to_string(read_path).ok()?;
    let content_hash = storage::hash_content(source_code.as_bytes());

    if trust_hashes {
        if let Some((_, old_hash)) = old_hashes.get(logical_path) {
            if old_hash == &content_hash {
                return Some(FileStatus::Unchanged);
            }
        }
    }

    parse_source(matched_frontend.as_ref(), read_path, logical_path, source_code)
        .map(FileStatus::Changed)
}

fn parse_source(
    frontend: &dyn parser::frontend::LanguageFrontend,
    read_path: &std::path::Path,
    logical_path: &str,
    source_code: String,
) -> Option<ParsedFile> {
    let content_hash = storage::hash_content(source_code.as_bytes());
    let (metadata, symbols, mut relationships, semantic_bindings) =
        frontend.parse_and_extract(&source_code, logical_path)?;

    // Angular split-file components: an event binding in the companion
    // `.component.html` (e.g. `(click)="save()"`) is a real call edge into
    // the `.component.ts` handler, but tree-sitter never sees it since it's
    // parsing the `.ts` file alone. Same regex-scan the original sequential
    // loop did, folded in here so it stays outside the DB-write phase.
    if logical_path.ends_with(".component.ts") {
        let html_path = read_path.with_extension("html");
        if let Ok(html_content) = std::fs::read_to_string(&html_path) {
            if let Ok(re) =
                regex::Regex::new(r#"\([a-zA-Z0-9_\-]+\)="([a-zA-Z0-9_]+)(?:\(|")"#)
            {
                for (line_idx, line_str) in html_content.lines().enumerate() {
                    for cap in re.captures_iter(line_str) {
                        if let Some(handler) = cap.get(1) {
                            relationships.push(graph::models::RelationshipNode {
                                name: handler.as_str().to_string(),
                                source: None,
                                line_number: line_idx + 1,
                                kind: Some("calls".to_string()),
                            });
                        }
                    }
                }
            }
        }
    }

    Some(ParsedFile {
        path: logical_path.to_string(),
        content_hash,
        metadata,
        symbols,
        relationships,
        semantic_bindings,
    })
}
