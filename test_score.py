import sys

with open('mcp/src/main.rs', 'r') as f:
    content = f.read()

log_code = """
                        let rel_cand_path = relative_hint(db, &cand_src.file_path);
                        let db_path = if rel_cand_path.starts_with("./") { rel_cand_path.to_string() } else { format!("./{}", rel_cand_path) };
                        if subsystem_files.contains(&db_path) {
                            score += 1500; // High subsystem relevance
                        }

                        if let Some(qv) = query_vector {
                            if let Ok(mut stmt) = db.conn.prepare("SELECT embedding FROM symbol_embeddings WHERE symbol_id = (SELECT id FROM symbols WHERE name = ?1 AND file_id = (SELECT id FROM files WHERE path = ?2) LIMIT 1)") {
                                if let Ok(mut rows) = stmt.query(rusqlite::params![cand_src.symbol_name, db_path]) {
                                    if let Ok(Some(row)) = rows.next() {
                                        let blob: Vec<u8> = row.get(0).unwrap_or_default();
                                        if !blob.is_empty() {
                                            let s_vec = storage::blob_to_embedding(&blob);
                                            let sim = storage::cosine_similarity(qv, &s_vec);
                                            score += ((sim + 1.0) * 500.0) as i32;
                                            eprintln!("SIMILARITY for {}: {}, score now {}", cand_src.symbol_name, sim, score);
                                        }
                                    } else {
                                        eprintln!("No embedding row for {} in {}", cand_src.symbol_name, db_path);
                                    }
                                } else {
                                    eprintln!("Query failed for {}", cand_src.symbol_name);
                                }
                            } else {
                                eprintln!("Prepare failed");
                            }
                        } else {
                            eprintln!("Query vector is NONE!");
                        }
                        eprintln!("Final Score for {}: {}", cand_src.symbol_name, score);
"""

content = content.replace("""                        let rel_cand_path = relative_hint(db, &cand_src.file_path);
                        let db_path = if rel_cand_path.starts_with("./") { rel_cand_path.to_string() } else { format!("./{}", rel_cand_path) };
                        if subsystem_files.contains(&db_path) {
                            score += 1500; // High subsystem relevance
                        }

                        if let Some(qv) = query_vector {
                            if let Ok(mut stmt) = db.conn.prepare("SELECT embedding FROM symbol_embeddings WHERE symbol_id = (SELECT id FROM symbols WHERE name = ?1 AND file_id = (SELECT id FROM files WHERE path = ?2) LIMIT 1)") {
                                if let Ok(mut rows) = stmt.query(rusqlite::params![cand_src.symbol_name, db_path]) {
                                    if let Ok(Some(row)) = rows.next() {
                                        let blob: Vec<u8> = row.get(0).unwrap_or_default();
                                        if !blob.is_empty() {
                                            let s_vec = storage::blob_to_embedding(&blob);
                                            let sim = storage::cosine_similarity(qv, &s_vec);
                                            score += ((sim + 1.0) * 500.0) as i32;
                                        }
                                    }
                                }
                            }
                        }""", log_code)

with open('mcp/src/main.rs', 'w') as f:
    f.write(content)
