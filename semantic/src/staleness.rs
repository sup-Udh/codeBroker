use query::context::ContextResponseBuilder;
use query::response::ResponseProfile;
use storage::Database;

/// Assembles context for `symbol`, transparently incrementally reindexing
/// the defining file and re-assembling once if the first assembly reports
/// `stale: true`.
///
/// Before this existed, editing a file and then immediately calling
/// `impact_analysis`/`get_context`/`get_edit_context` on a symbol in it
/// produced a `stale: true` flag the caller had to notice and act on by
/// separately calling `reindex_workspace` — easy to miss, and `impact_analysis`
/// in particular used to serve the (possibly fan-out-corrupted) stale data
/// anyway rather than refusing. This makes the common "edit, then
/// immediately ask about it" workflow correct by default instead of relying
/// on the caller to remember a manual step.
pub fn assemble_context_self_healing<'a>(
    db: &'a Database,
    symbol: &str,
    file_hint: Option<&str>,
    profile: ResponseProfile,
) -> Result<Option<ContextResponseBuilder<'a>>, String> {
    assemble_context_self_healing_by_id(db, None, symbol, file_hint, profile)
}

/// Same as `assemble_context_self_healing`, but when `symbol_id` is supplied
/// (a resolver-confirmed id, e.g. `resolver::resolve_symbol`'s
/// `ResolvedSymbol.id`) the initial assembly looks that row up directly
/// instead of re-running the name/file_hint match — so this can never
/// silently operate on a different same-named symbol than the one that was
/// actually resolved. The post-reindex refresh always re-resolves by name,
/// since reindexing can change row ids.
pub fn assemble_context_self_healing_by_id<'a>(
    db: &'a Database,
    symbol_id: Option<i64>,
    symbol: &str,
    file_hint: Option<&str>,
    profile: ResponseProfile,
) -> Result<Option<ContextResponseBuilder<'a>>, String> {
    let context = match symbol_id {
        Some(id) => ContextResponseBuilder::new_by_id(db, id, profile),
        None => ContextResponseBuilder::new(db, symbol, file_hint, profile),
    }
    .map_err(|e| e.to_string())?;
    let context = match context {
        Some(c) => c,
        None => return Ok(None),
    };

    if !context.stale {
        return Ok(Some(context));
    }

    // Best-effort: if the incremental reindex itself fails (e.g. the file
    // was deleted since indexing), fall back to returning the stale context
    // as-is rather than erroring outright — the caller still sees
    // `stale: true` on the returned object and can decide what to do, which
    // is strictly better than this helper hiding a real symbol behind a
    // hard error.
    if indexer::reindex::reindex_paths(db, &db.project_root, &[context.defining_file.clone()])
        .is_err()
    {
        return Ok(Some(context));
    }

    let refreshed =
        ContextResponseBuilder::new(db, symbol, file_hint, profile).map_err(|e| e.to_string())?;
    Ok(refreshed.or(Some(context)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshes_a_stale_symbol_without_caller_action() {
        let unique = format!(
            "codebroker_test_staleness_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let project_root = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&project_root).unwrap();
        std::fs::create_dir_all(project_root.join(".codebroker")).unwrap();

        let original_source = "export function greet(name) {\n  return name;\n}\n";
        std::fs::write(project_root.join("greet.ts"), original_source).unwrap();

        let db_path = project_root.join(".codebroker").join("codebroker.db");
        let db = Database::new(db_path.to_str().unwrap()).unwrap();
        db.init_schema().unwrap();

        let project_root_str = project_root.to_str().unwrap();
        indexer::reindex::reindex_paths(&db, project_root_str, &["greet.ts".to_string()]).unwrap();

        // Edit the file on disk WITHOUT going through reindex — this is what
        // makes the indexed copy stale.
        let edited_source =
            "export function greet(name) {\n  return `Hello, ${name}`;\n}\n\nfunction extra() {}\n";
        std::fs::write(project_root.join("greet.ts"), edited_source).unwrap();

        let stale_check = ContextResponseBuilder::new(&db, "greet", None, ResponseProfile::Verbose)
            .unwrap()
            .unwrap();
        assert!(
            stale_check.stale,
            "expected the unreindexed edit to be flagged stale"
        );

        let healed = assemble_context_self_healing(&db, "greet", None, ResponseProfile::Verbose)
            .unwrap()
            .expect("symbol should still resolve after self-heal");
        assert!(
            !healed.stale,
            "self-healing must reindex and return a fresh context"
        );

        std::fs::remove_dir_all(&project_root).ok();
    }
}
