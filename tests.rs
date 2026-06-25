#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a throwaway project at a temp dir with `route.ts`'s exact
    /// content, indexes its two symbols by hand (no parser dependency needed
    /// — the byte ranges are computed directly off the known fixture text),
    /// and returns an open `Database` pointed at it.
    fn setup_fixture() -> (Database, std::path::PathBuf) {
        let unique = format!(
            "codebroker_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        );
        let project_root = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&project_root).unwrap();
        std::fs::create_dir_all(project_root.join(".codebroker")).unwrap();

        let source = "function generateRoomId(): string {\n  return \"x\";\n}\n\nexport async function GET(request: Request) {\n  const id = generateRoomId();\n  return id;\n}\n";
        std::fs::write(project_root.join("route.ts"), source).unwrap();

        let db_path = project_root.join(".codebroker").join("codebroker.db");
        let db = Database::new(db_path.to_str().unwrap()).unwrap();
        db.init_schema().unwrap();

        let content_hash = storage::hash_content(source.as_bytes());
        let file_id = db.insert_file("route.ts", &content_hash).unwrap();

        let gen_id = db.insert_symbol(file_id, &graph::SymbolNode {
            name: "generateRoomId".to_string(),
            kind: "function".to_string(),
            start_line: 1,
            end_line: 3,
            start_byte: 0,
            end_byte: 46,
            signature: None,
            attributes: Vec::new(),
            metadata: None,
        }).unwrap();

        let get_id = db.insert_symbol(file_id, &graph::SymbolNode {
            name: "GET".to_string(),
            kind: "function".to_string(),
            start_line: 5,
            end_line: 8,
            start_byte: 0,
            end_byte: 145,
            signature: None,
            attributes: Vec::new(),
            metadata: None,
        }).unwrap();

        db.insert_edge_attributed(file_id, Some(get_id), gen_id, "CALL").unwrap();

        (db, project_root)
    }

    /// Regression test: a private helper called only by a sibling function
    /// in the same file must NOT report empty dependents everywhere — that's
    /// indistinguishable from dead code and an agent trusting it could
    /// delete load-bearing logic. `reverse_dependencies` (cross-file/imports
    /// only) staying empty is correct; `same_file_callers` must catch it.
    #[test]
    fn same_file_caller_is_not_reported_as_dead_code() {
        let (db, project_root) = setup_fixture();

        let context = ContextObject::assemble(&db, "generateRoomId").unwrap().unwrap();

        assert!(
            context.reverse_dependencies.is_empty(),
            "reverse_dependencies is import-based and cross-file only; this fixture has no importers"
        );
        assert_eq!(
            context.same_file_callers,
            vec!["GET".to_string()],
            "GET calls generateRoomId() in the same file — must show up here even though reverse_dependencies is empty"
        );

        std::fs::remove_dir_all(&project_root).ok();
    }

    #[test]
    fn unreferenced_symbol_has_no_same_file_callers() {
        let (db, project_root) = setup_fixture();

        let context = ContextObject::assemble(&db, "GET").unwrap().unwrap();

        assert!(context.same_file_callers.is_empty(), "nothing in this fixture calls GET");

        std::fs::remove_dir_all(&project_root).ok();
    }
}