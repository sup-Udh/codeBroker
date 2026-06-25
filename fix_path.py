import sys

with open('mcp/src/main.rs', 'r') as f:
    content = f.read()

old_context = """                        let rel_cand_path = relative_hint(db, &cand_src.file_path);
                        if subsystem_files.contains(rel_cand_path) {
                            score += 1500; // High subsystem relevance
                        }

                        if let Some(qv) = query_vector {
                            if let Ok(mut stmt) = db.conn.prepare("SELECT embedding FROM symbol_embeddings WHERE symbol_id = (SELECT id FROM symbols WHERE name = ?1 AND file_id = (SELECT id FROM files WHERE path = ?2) LIMIT 1)") {
                                if let Ok(mut rows) = stmt.query(rusqlite::params![cand_src.symbol_name, rel_cand_path]) {"""

new_context = """                        let rel_cand_path = relative_hint(db, &cand_src.file_path);
                        let db_path = if rel_cand_path.starts_with("./") { rel_cand_path.to_string() } else { format!("./{}", rel_cand_path) };
                        if subsystem_files.contains(&db_path) {
                            score += 1500; // High subsystem relevance
                        }

                        if let Some(qv) = query_vector {
                            if let Ok(mut stmt) = db.conn.prepare("SELECT embedding FROM symbol_embeddings WHERE symbol_id = (SELECT id FROM symbols WHERE name = ?1 AND file_id = (SELECT id FROM files WHERE path = ?2) LIMIT 1)") {
                                if let Ok(mut rows) = stmt.query(rusqlite::params![cand_src.symbol_name, db_path]) {"""

content = content.replace(old_context, new_context)

with open('mcp/src/main.rs', 'w') as f:
    f.write(content)

print("Path fix applied successfully")
