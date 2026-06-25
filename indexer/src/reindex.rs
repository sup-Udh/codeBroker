use std::fs;
use storage::{Database, GENERIC_SYMBOL_NAMES};

/// Result of an incremental reindex pass over a subset of files.
pub struct IncrementalStats {
    pub files_processed: usize,
    pub symbols_inserted: usize,
    pub edges_created: usize,
    /// Paths that couldn't be read or matched no language frontend.
    pub skipped: Vec<String>,
    /// ids of every symbol (re)inserted into a touched file this pass. A
    /// caller that also wants semantic-search embeddings up to date can pass
    /// this straight to `semantic::embeddings::backfill_missing_embeddings`'s
    /// `only_symbol_ids` to embed exactly what changed, instead of rescanning
    /// the whole repo for missing embeddings after every small edit.
    pub touched_symbol_ids: Vec<i64>,
}

/// Re-parses only `changed_paths` and re-links edges originating from them,
/// instead of the full `codebroker init` walk-and-rebuild. This trades
/// completeness for speed: it intentionally skips tsconfig/vite alias
/// resolution, route-push linking, and prop-type/layout linking (all of which
/// `codebroker init` does on a full pass), falling back to plain symbol-name
/// matching. For anything beyond a single-function edit, prefer a full
/// `reindex_workspace` call (no `changed_paths`).
///
/// Correctness note: `delete_file_data` (called per touched file below)
/// deletes every edge whose `target_symbol_id` points at one of that file's
/// symbols — necessary, since those symbols are about to be deleted and
/// reinserted with new auto-increment ids, making the old ids dangling.
/// Pass 2 below therefore must re-link not just the touched files' own
/// outgoing edges, but every OTHER file's `raw_imports` row that references
/// a symbol name just (re)inserted into a touched file — otherwise those
/// cross-file consumers' edges are deleted here and never recreated,
/// silently losing real reverse-dependency data (the untouched consumer
/// files are never revisited, so nothing re-triggers their linking). This is
/// tracked via `touched_symbol_names` and was previously missing entirely:
/// pass 2 only considered `source_file_id`s in `touched_file_ids`.
pub fn reindex_paths(
    db: &Database,
    project_root: &str,
    changed_paths: &[String],
) -> Result<IncrementalStats, String> {
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

    let mut stats = IncrementalStats {
        files_processed: 0,
        symbols_inserted: 0,
        edges_created: 0,
        skipped: Vec::new(),
        touched_symbol_ids: Vec::new(),
    };
    let mut touched_file_ids: Vec<i64> = Vec::new();
    // Names of every symbol (re)inserted into a touched file this pass. Any
    // raw_import row anywhere in the repo whose import_name matches one of
    // these — not just rows belonging to touched_file_ids — must be
    // re-linked, since `delete_file_data` just deleted that symbol's
    // incoming edges along with the symbol's old id.
    let mut touched_symbol_names: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for raw_path in changed_paths {
        let abs = std::path::Path::new(raw_path);
        let abs = if abs.is_absolute() {
            abs.to_path_buf()
        } else {
            std::path::Path::new(project_root).join(abs)
        };
        let rel = match abs.strip_prefix(project_root) {
            Ok(r) => r.to_string_lossy().to_string(),
            Err(_) => raw_path.trim_start_matches("./").to_string(),
        };
        // Match the "./relative/path" format `codebroker init` stores (it walks
        // from "." so every stored path carries that prefix).
        let stored_path = format!("./{}", rel.trim_start_matches("./"));

        if let Ok(Some(existing_id)) = db.get_file_id_by_path(&stored_path) {
            db.delete_file_data(existing_id)
                .map_err(|e| e.to_string())?;
        }

        let source_code = match fs::read_to_string(&abs) {
            Ok(s) => s,
            Err(_) => {
                stats.skipped.push(raw_path.clone());
                continue;
            }
        };

        let matched_frontend = frontends.iter().find(|f| f.can_handle(&stored_path));
        let frontend = match matched_frontend {
            Some(f) => f,
            None => {
                stats.skipped.push(raw_path.clone());
                continue;
            }
        };

        let content_hash = storage::hash_content(source_code.as_bytes());
        let file_id = db
            .insert_file(&stored_path, &content_hash)
            .map_err(|e| e.to_string())?;
        touched_file_ids.push(file_id);
        stats.files_processed += 1;

        if let Some((metadata, symbols, imports)) =
            frontend.parse_and_extract(&source_code, &stored_path)
        {
            let _ = db.update_file_metadata(file_id, metadata.metadata.as_deref());
            for symbol in symbols {
                if let Ok(symbol_id) = db.insert_symbol(file_id, &symbol) {
                    stats.symbols_inserted += 1;
                    stats.touched_symbol_ids.push(symbol_id);
                    touched_symbol_names.insert(symbol.name.clone());
                }
            }
            for import in imports {
                let _ = db.insert_raw_import(file_id, &import);
            }
        }
    }

    // Scoped pass 2: re-link raw_imports belonging to files we just touched
    // (their own outgoing edges), PLUS any other file's raw_import that
    // references a symbol name we just re-inserted (their now-dangling
    // incoming edges — see the correctness note on this function). Re-running
    // the full linker unconditionally would re-insert duplicate edges for
    // every untouched file in the repo; this keeps the scope to exactly the
    // touched files plus their known consumers.
    let raw_imports = db
        .get_all_raw_imports_with_lines()
        .map_err(|e| e.to_string())?;
    for (_raw_id, source_file_id, import_name, _import_source, import_kind, line_number) in
        raw_imports
    {
        let is_touched_source = touched_file_ids.contains(&source_file_id);
        let references_touched_symbol = touched_symbol_names.contains(&import_name);
        if !is_touched_source && !references_touched_symbol {
            continue;
        }
        let edge_kind = import_kind.unwrap_or_else(|| "imports".to_string());
        // Attribute the edge to its enclosing symbol (by line) so cycle
        // detection has a symbol-level graph, matching the full init linker.
        let src_sym = db
            .enclosing_symbol_id(source_file_id, line_number)
            .unwrap_or(None);

        // Call edges use case-sensitive, scope-aware resolution (no global
        // bare-name fallback), matching the full `codebroker init` linker —
        // otherwise a member call like `query.delete()` re-links to an
        // exported `DELETE` on every incremental reindex. See resolve_call_edge.
        if edge_kind == "calls" || edge_kind == "method_call" || edge_kind == "MEMBER_ACCESS" {
            if let Ok(Some(local_id)) =
                db.find_symbol_id_in_file_exact(source_file_id, &import_name)
            {
                if src_sym != Some(local_id)
                    && db
                        .insert_edge_attributed(source_file_id, src_sym, local_id, &edge_kind)
                        .is_ok()
                {
                    stats.edges_created += 1;
                }
            } else if edge_kind == "calls" || edge_kind == "MEMBER_ACCESS" {
                if !GENERIC_SYMBOL_NAMES.contains(&import_name.as_str()) {
                    if let Ok(Some((target_id, target_file_id))) =
                        db.find_symbol_exact_with_file(&import_name)
                    {
                        if target_file_id != source_file_id
                            && db
                                .insert_edge_attributed(
                                    source_file_id,
                                    src_sym,
                                    target_id,
                                    &edge_kind,
                                )
                                .is_ok()
                        {
                            stats.edges_created += 1;
                        }
                    }
                }
            }
            continue;
        }

        let words: Vec<&str> = import_name.split(|c: char| !c.is_alphanumeric()).collect();
        for word in words {
            if word.is_empty() {
                continue;
            }
            // See GENERIC_SYMBOL_NAMES: a global bare-name match against these
            // is a coincidence, not a real reference.
            if GENERIC_SYMBOL_NAMES.contains(&word) {
                continue;
            }
            if let Ok(Some(target_symbol_id)) = db.find_symbol_id_by_name(word) {
                if db
                    .insert_edge_attributed(source_file_id, src_sym, target_symbol_id, &edge_kind)
                    .is_ok()
                {
                    stats.edges_created += 1;
                }
            }
        }
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(project_root: &std::path::Path, rel: &str, content: &str) {
        let path = project_root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn edge_count_into(db: &Database, symbol_name: &str) -> i64 {
        db.conn
            .query_row(
                "SELECT COUNT(*) FROM edges WHERE target_symbol_id = (SELECT id FROM symbols WHERE name = ?1)",
                [symbol_name],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }

    // Regression for a reported correctness bug: incrementally reindexing
    // ONLY the file defining a helper silently lost every OTHER file's edge
    // into that helper. `delete_file_data` deletes a touched file's incoming
    // edges (correct — the symbol is about to get a new id), but the old
    // pass 2 only re-linked raw_imports whose source_file_id was itself a
    // touched file, so untouched consumer files were never revisited and
    // their now-dangling edges were never recreated.
    #[test]
    fn incremental_reindex_of_helper_preserves_untouched_consumers_edges() {
        let unique = format!(
            "codebroker_test_reindex_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let project_root = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&project_root).unwrap();
        std::fs::create_dir_all(project_root.join(".codebroker")).unwrap();

        write(
            &project_root,
            "lib/timeFormat.ts",
            "export function timeAgo(d) {\n  return d;\n}\n",
        );
        write(
            &project_root,
            "pages/dashboard.ts",
            "import { timeAgo } from \"../lib/timeFormat\";\nfunction render() {\n  return timeAgo(1);\n}\n",
        );

        let db_path = project_root.join(".codebroker").join("codebroker.db");
        let db = Database::new(db_path.to_str().unwrap()).unwrap();
        db.init_schema().unwrap();

        let project_root_str = project_root.to_str().unwrap();

        // Initial "full" index: both files reindexed together.
        reindex_paths(
            &db,
            project_root_str,
            &[
                "lib/timeFormat.ts".to_string(),
                "pages/dashboard.ts".to_string(),
            ],
        )
        .unwrap();

        assert!(
            edge_count_into(&db, "timeAgo") > 0,
            "dashboard.ts's reference to timeAgo must produce an edge after the initial index"
        );

        // Incremental reindex of ONLY the helper file, simulating an edit to
        // timeFormat.ts. dashboard.ts is untouched.
        reindex_paths(&db, project_root_str, &["lib/timeFormat.ts".to_string()]).unwrap();

        assert!(
            edge_count_into(&db, "timeAgo") > 0,
            "incrementally reindexing only the helper's file must not lose the untouched \
             consumer's edge into it"
        );

        std::fs::remove_dir_all(&project_root).ok();
    }
}
