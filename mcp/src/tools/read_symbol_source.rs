use crate::contracts::{GraphPrimitive, ToolManifest};
use resolver::ResolvedEntity;
use storage::Database;

pub struct ReadSymbolSource;

impl ReadSymbolSource {
    pub fn manifest() -> ToolManifest {
        ToolManifest::new(
            "read_symbol_source",
            vec![GraphPrimitive::SemanticNode, GraphPrimitive::Implementation],
        )
    }

    pub fn execute(
        db: &Database,
        symbol: &str,
        file_hint: Option<&str>,
        line_hint: Option<i64>,
        include_deps: bool,
    ) -> String {
        // Step 1: Universal Resolver Pipeline
        let resolved = resolver::resolve_symbol(db, symbol, file_hint, line_hint);
        let s = match resolved {
            ResolvedEntity::Symbol(s) => s,
            other => return other.to_json_string(),
        };

        let mut results = Vec::new();

        // Step 2: Fetch Target Source
        let mut target_obj = fetch_source_as_json(&s.file_path, s.start_byte, s.end_byte);
        target_obj.insert("resolved_as".to_string(), serde_json::json!("unique"));
        target_obj.insert("symbol_name".to_string(), serde_json::json!(s.name));
        target_obj.insert("kind".to_string(), serde_json::json!(s.kind));
        target_obj.insert("file_path".to_string(), serde_json::json!(s.file_path));
        target_obj.insert("start_line".to_string(), serde_json::json!(s.start_line));
        target_obj.insert("end_line".to_string(), serde_json::json!(s.end_line));
        target_obj.insert("confidence".to_string(), serde_json::json!(s.confidence.score));
        target_obj.insert("is_dependency".to_string(), serde_json::json!(false));

        if !include_deps {
            results.push(target_obj);
            return serde_json::to_string_pretty(&results).unwrap_or_else(|_| "[]".to_string());
        }

        // Step 3: Fetch Dependencies
        let mut omitted_deps = Vec::new();
        let mut deps_count = 0;
        let max_deps = 10;

        // 3a. Internal Dependencies (Ranked by Edge Count)
        let internal_deps_sql = "
            SELECT symbols.name, symbols.kind 
            FROM edges 
            JOIN symbols ON edges.target_symbol_id = symbols.id 
            WHERE edges.source_symbol_id = ?1 
            GROUP BY symbols.id 
            ORDER BY COUNT(*) DESC
        ";
        if let Ok(mut stmt) = db.conn.prepare(internal_deps_sql) {
            if let Ok(mut rows) = stmt.query(rusqlite::params![s.id]) {
                while let Ok(Some(row)) = rows.next() {
                    let dep_name: String = row.get(0).unwrap_or_default();
                    let dep_kind: String = row.get(1).unwrap_or_default();

                    if dep_name.is_empty() {
                        continue;
                    }

                    // Skip type-only dependencies
                    if dep_kind == "type" || dep_kind == "interface" {
                        omitted_deps.push(dep_name);
                        continue;
                    }

                    if deps_count >= max_deps {
                        omitted_deps.push(dep_name);
                        continue;
                    }

                    // Resolve the dependency to get its file boundaries
                    let dep_resolved = resolver::resolve_symbol(db, &dep_name, None, None);
                    match dep_resolved {
                        ResolvedEntity::Symbol(ds) => {
                            let mut dep_obj = fetch_source_as_json(&ds.file_path, ds.start_byte, ds.end_byte);
                            dep_obj.insert("resolved_as".to_string(), serde_json::json!("unique"));
                            dep_obj.insert("symbol_name".to_string(), serde_json::json!(ds.name));
                            dep_obj.insert("kind".to_string(), serde_json::json!(ds.kind));
                            dep_obj.insert("file_path".to_string(), serde_json::json!(ds.file_path));
                            dep_obj.insert("start_line".to_string(), serde_json::json!(ds.start_line));
                            dep_obj.insert("end_line".to_string(), serde_json::json!(ds.end_line));
                            dep_obj.insert("confidence".to_string(), serde_json::json!(ds.confidence.score));
                            dep_obj.insert("is_dependency".to_string(), serde_json::json!(true));
                            results.push(dep_obj);
                            deps_count += 1;
                        }
                        _ => {
                            // Stub for unresolvable internal symbol
                            let mut stub = serde_json::Map::new();
                            stub.insert("symbol_name".to_string(), serde_json::json!(dep_name));
                            stub.insert("resolved".to_string(), serde_json::json!(false));
                            stub.insert("reason".to_string(), serde_json::json!("unresolved or ambiguous"));
                            stub.insert("is_dependency".to_string(), serde_json::json!(true));
                            results.push(stub);
                            deps_count += 1;
                        }
                    }
                }
            }
        }

        // 3b. External Dependencies (from relationships where is_local = 0)
        // Note: We don't cap external dependencies since they are just small stubs
        let file_id: i64 = db.conn.query_row(
            "SELECT file_id FROM symbols WHERE id = ?1 LIMIT 1",
            rusqlite::params![s.id],
            |r| r.get(0),
        ).unwrap_or(-1);

        if file_id != -1 {
            if let Ok(mut ext_stmt) = db.conn.prepare("SELECT name FROM relationships WHERE file_id = ?1 AND is_local = 0") {
                if let Ok(mut ext_rows) = ext_stmt.query(rusqlite::params![file_id]) {
                    let mut ext_names = std::collections::HashSet::new();
                    while let Ok(Some(row)) = ext_rows.next() {
                        if let Ok(name) = row.get::<_, String>(0) {
                            ext_names.insert(name);
                        }
                    }
                    let mut ext_names: Vec<_> = ext_names.into_iter().collect();
                    ext_names.sort();
                    
                    for name in ext_names {
                        let mut stub = serde_json::Map::new();
                        stub.insert("symbol_name".to_string(), serde_json::json!(name));
                        stub.insert("resolved".to_string(), serde_json::json!(false));
                        stub.insert("reason".to_string(), serde_json::json!(format!("external module '{}'", name)));
                        stub.insert("is_dependency".to_string(), serde_json::json!(true));
                        results.push(stub);
                    }
                }
            }
        }

        // Target object is inserted first
        if !omitted_deps.is_empty() {
            target_obj.insert("dependencies_truncated".to_string(), serde_json::json!(true));
            target_obj.insert("omitted".to_string(), serde_json::json!(omitted_deps));
        }
        
        let mut final_results = vec![serde_json::Value::Object(target_obj)];
        for res in results {
            final_results.push(serde_json::Value::Object(res));
        }

        serde_json::to_string_pretty(&final_results).unwrap_or_else(|_| "[]".to_string())
    }
}

fn fetch_source_as_json(abs_path: &str, start_byte: i64, end_byte: i64) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();
    let mut stale = false;
    
    let source_body = if abs_path.ends_with(".json") {
        "/* JSON symbol – no source body */".to_string()
    } else if let Ok(content) = std::fs::read(abs_path) {
        let start_b = start_byte as usize;
        let end_b = end_byte as usize;
        
        if start_b <= end_b && end_b <= content.len() {
            String::from_utf8_lossy(&content[start_b..end_b]).to_string()
        } else {
            stale = true;
            "/* Source bytes out of bounds (file modified since indexing) */".to_string()
        }
    } else {
        stale = true;
        "/* Error reading file from disk */".to_string()
    };

    map.insert("source".to_string(), serde_json::json!(source_body));
    map.insert("stale_index".to_string(), serde_json::json!(stale));
    map
}
