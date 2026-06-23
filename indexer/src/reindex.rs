use storage::Database;
use std::fs;

/// Result of an incremental reindex pass over a subset of files.
pub struct IncrementalStats {
    pub files_processed: usize,
    pub symbols_inserted: usize,
    pub edges_created: usize,
    /// Paths that couldn't be read or matched no language frontend.
    pub skipped: Vec<String>,
}

/// Re-parses only `changed_paths` and re-links edges originating from them,
/// instead of the full `codebroker init` walk-and-rebuild. This trades
/// completeness for speed: it intentionally skips tsconfig/vite alias
/// resolution, route-push linking, and prop-type/layout linking (all of which
/// `codebroker init` does on a full pass), falling back to plain symbol-name
/// matching. Symbols renamed or removed inside `changed_paths` correctly drop
/// their stale edges (via `delete_file_data`), but OTHER files that imported
/// those symbols are not re-linked here — they keep stale/missing edges until
/// they're reindexed too. For anything beyond a single-function edit, prefer
/// a full `reindex_workspace` call (no `changed_paths`).
pub fn reindex_paths(db: &Database, project_root: &str, changed_paths: &[String]) -> Result<IncrementalStats, String> {
    use parser::frontend::{LanguageFrontend, RustFrontend};
    use parser::typescript_frontend::{TypeScriptFrontend, TsxFrontend};
    use parser::python_frontend::PythonFrontend;
    use parser::javascript_frontend::JavaScriptFrontend;
    use parser::config_frontend::ConfigFrontend;
    use parser::vue_frontend::VueFrontend;
    use parser::svelte_frontend::SvelteFrontend;

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

    let mut stats = IncrementalStats { files_processed: 0, symbols_inserted: 0, edges_created: 0, skipped: Vec::new() };
    let mut touched_file_ids: Vec<i64> = Vec::new();

    for raw_path in changed_paths {
        let abs = std::path::Path::new(raw_path);
        let abs = if abs.is_absolute() { abs.to_path_buf() } else { std::path::Path::new(project_root).join(abs) };
        let rel = match abs.strip_prefix(project_root) {
            Ok(r) => r.to_string_lossy().to_string(),
            Err(_) => raw_path.trim_start_matches("./").to_string(),
        };
        // Match the "./relative/path" format `codebroker init` stores (it walks
        // from "." so every stored path carries that prefix).
        let stored_path = format!("./{}", rel.trim_start_matches("./"));

        if let Ok(Some(existing_id)) = db.get_file_id_by_path(&stored_path) {
            db.delete_file_data(existing_id).map_err(|e| e.to_string())?;
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
        let file_id = db.insert_file(&stored_path, &content_hash).map_err(|e| e.to_string())?;
        touched_file_ids.push(file_id);
        stats.files_processed += 1;

        if let Some((metadata, symbols, imports)) = frontend.parse_and_extract(&source_code, &stored_path) {
            let _ = db.update_file_metadata(file_id, metadata.directive.as_deref(), None, None);
            for symbol in symbols {
                if db.insert_symbol(file_id, &symbol).is_ok() {
                    stats.symbols_inserted += 1;
                }
            }
            for import in imports {
                let _ = db.insert_raw_import(file_id, &import);
            }
        }
    }

    // Scoped pass 2: only re-link raw_imports belonging to files we just
    // touched. Re-running the full linker would re-insert duplicate edges for
    // every untouched file in the repo.
    let raw_imports = db.get_all_raw_imports().map_err(|e| e.to_string())?;
    for (_raw_id, source_file_id, import_name, _import_source, import_kind) in raw_imports {
        if !touched_file_ids.contains(&source_file_id) {
            continue;
        }
        let edge_kind = import_kind.unwrap_or_else(|| "imports".to_string());

        // Call edges use case-sensitive, scope-aware resolution (no global
        // bare-name fallback), matching the full `codebroker init` linker —
        // otherwise a member call like `query.delete()` re-links to an
        // exported `DELETE` on every incremental reindex. See resolve_call_edge.
        if edge_kind == "calls" || edge_kind == "method_call" {
            if let Ok(Some(local_id)) = db.find_symbol_id_in_file_exact(source_file_id, &import_name) {
                if db.insert_edge(source_file_id, local_id, &edge_kind).is_ok() {
                    stats.edges_created += 1;
                }
            } else if edge_kind == "calls" {
                if let Ok(Some((target_id, target_file_id))) = db.find_symbol_exact_with_file(&import_name) {
                    if target_file_id != source_file_id
                        && db.insert_edge(source_file_id, target_id, &edge_kind).is_ok()
                    {
                        stats.edges_created += 1;
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
            if let Ok(Some(target_symbol_id)) = db.find_symbol_id_by_name(word) {
                if db.insert_edge(source_file_id, target_symbol_id, &edge_kind).is_ok() {
                    stats.edges_created += 1;
                }
            }
        }
    }

    Ok(stats)
}
