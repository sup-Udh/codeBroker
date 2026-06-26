use std::collections::{HashMap, HashSet};
use std::fs;
use std::time::Instant;
use storage::Database;

// Very basic inference using file content scanning for now,
// matching the request to infer generic interactions.
pub fn infer_interactions(db: &Database) -> Result<(), String> {
    let t_total = Instant::now();

    // 1. Gather all files and their contents
    let t0 = Instant::now();
    let mut stmt = db
        .conn
        .prepare("SELECT id, path FROM files")
        .map_err(|e| e.to_string())?;
    let mut files_rows = stmt.query([]).map_err(|e| e.to_string())?;

    let mut file_contents = HashMap::new();
    while let Ok(Some(row)) = files_rows.next() {
        let file_id: i64 = row.get(0).unwrap();
        let path: String = row.get(1).unwrap();
        let resolved = db.resolve_path(&path);
        if let Ok(content) = fs::read_to_string(&resolved) {
            file_contents.insert(file_id, content);
        }
    }
    eprintln!(
        "[TIMING:interactions] Re-read {} files from disk: {}ms",
        file_contents.len(),
        t0.elapsed().as_millis()
    );

    // 2. Gather entrypoints (e.g., routes)
    let t1 = Instant::now();
    let mut route_stmt = db.conn.prepare("SELECT id, name, file_id, attributes FROM symbols WHERE kind IN ('route', 'endpoint', 'handler')").map_err(|e| e.to_string())?;
    let mut route_rows = route_stmt.query([]).map_err(|e| e.to_string())?;

    struct Entrypoint {
        id: i64,
        name: String,
        file_id: i64,
        attributes: Option<String>,
    }

    let mut entrypoints = Vec::new();
    while let Ok(Some(row)) = route_rows.next() {
        entrypoints.push(Entrypoint {
            id: row.get(0).unwrap(),
            name: row.get(1).unwrap(),
            file_id: row.get(2).unwrap(),
            attributes: row.get(3).unwrap_or(None),
        });
    }
    eprintln!(
        "[TIMING:interactions] Load entrypoints: {}ms ({} entrypoints)",
        t1.elapsed().as_millis(),
        entrypoints.len()
    );

    // 3. Scan for interactions
    let t2 = Instant::now();
    let mut interaction_edges_inserted = 0usize;
    for (file_id, content) in &file_contents {
        // Find all fetch/post/get
        let fetches = extract_literals(content, "fetch(");
        let posts = extract_literals(content, "post(");
        let gets = extract_literals(content, "get(");
        let puts = extract_literals(content, "put(");
        let deletes = extract_literals(content, "delete(");

        let mut all_literals = Vec::new();
        for lit in fetches {
            all_literals.push((lit, "fetch"));
        }
        for lit in posts {
            all_literals.push((lit, "http_post"));
        }
        for lit in gets {
            all_literals.push((lit, "http_get"));
        }
        for lit in puts {
            all_literals.push((lit, "http_put"));
        }
        for lit in deletes {
            all_literals.push((lit, "http_delete"));
        }

        for (lit, method) in all_literals {
            for ep in &entrypoints {
                // If the literal matches the entrypoint name (e.g. "/api/v1/auth" matches Route("/api/v1/auth"))
                let mut matches = lit.contains(&ep.name) && ep.name.len() > 3;

                // Or if it matches attributes (e.g. @app.post("/api/v1/auth"))
                if let Some(attrs) = &ep.attributes {
                    if attrs.contains(&lit) {
                        matches = true;
                    }
                }

                if matches {
                    let metadata = format!(
                        "{{\"kind\":\"http\",\"method\":\"{}\",\"evidence\":\"Found {}('{}')\",\"resolved_target\":\"{}\"}}",
                        method, method, lit, ep.name
                    );

                    // Find enclosing symbol for the file
                    let mut src_sym = None;
                    // Approximating source_symbol_id by finding the first symbol in the file (or just leaving it file-level for now)
                    // We can refine this by getting line numbers of matches.

                    let _ = db.insert_interaction_edge(
                        *file_id, src_sym, ep.id, &metadata, 0.9, // high confidence if matched
                    );
                    interaction_edges_inserted += 1;
                }
            }
        }
    }
    eprintln!(
        "[TIMING:interactions] Scan + edge insertion ({} edges): {}ms",
        interaction_edges_inserted,
        t2.elapsed().as_millis()
    );
    eprintln!("[TIMING:interactions] Total infer_interactions: {}ms", t_total.elapsed().as_millis());

    Ok(())
}

fn extract_literals(source: &str, keyword: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;
    while let Some(rel) = source[i..].find(keyword) {
        let start = i + rel + keyword.len();
        let mut j = start;
        while j < bytes.len() && (bytes[j] as char).is_whitespace() {
            j += 1;
        }
        if j < bytes.len() && matches!(bytes[j] as char, '"' | '\'' | '`') {
            let quote = bytes[j] as char;
            let lit_start = j + 1;
            if let Some(end_rel) = source[lit_start..].find(quote) {
                let literal = &source[lit_start..lit_start + end_rel];
                if literal.len() > 1 {
                    out.push(literal.to_string());
                }
            }
        }
        i = start;
    }
    out
}
