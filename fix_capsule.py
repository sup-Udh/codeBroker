import sys

with open('mcp/src/main.rs', 'r') as f:
    content = f.read()

# Replace the subsystem boost logic for pivots
old_boost = """            // Boost existing results that are in subsystem
            for r in &mut results {
                if subsystem_files.contains(&r.path) {
                    r.score += 2000;
                    if !r.confidence.starts_with("High") {
                        r.confidence = "High (Subsystem Anchor)".to_string();
                    }
                }
            }"""

new_boost = """            // Boost existing results that are in subsystem
            let routes_set: std::collections::HashSet<_> = stats.routes.iter().cloned().collect();
            let symbols_set: std::collections::HashSet<_> = stats.symbols.iter().cloned().collect();
            
            for r in &mut results {
                let mut boosted = false;
                if routes_set.contains(&r.name) {
                    r.score += 5000;
                    r.confidence = "High (Subsystem Route)".to_string();
                    boosted = true;
                } else if symbols_set.contains(&r.name) {
                    r.score += 3000;
                    r.confidence = "High (Subsystem Core)".to_string();
                    boosted = true;
                } else if subsystem_files.contains(&r.path) {
                    r.score += 1000;
                    if !r.confidence.starts_with("High") {
                        r.confidence = "Medium (Subsystem Peripheral)".to_string();
                    }
                    boosted = true;
                }
                
                // Penalize massive generic UI components if we have a subsystem
                // so they don't drown out focused backend services.
                if boosted && r.path.contains("components/") || r.name == "Dashboard" {
                    r.score -= 2000;
                }
            }"""

content = content.replace(old_boost, new_boost)

# Fix relative paths for supporting context
old_context = """                        if subsystem_files.contains(&cand_src.file_path) {
                            score += 1000; // High subsystem relevance
                        }

                        if let Some(qv) = query_vector {
                            if let Ok(mut stmt) = db.conn.prepare("SELECT embedding FROM symbol_embeddings WHERE symbol_id = (SELECT id FROM symbols WHERE name = ?1 AND file_id = (SELECT id FROM files WHERE path = ?2) LIMIT 1)") {
                                if let Ok(mut rows) = stmt.query(rusqlite::params![cand_src.symbol_name, cand_src.file_path]) {"""

new_context = """                        let cand_rel_path = relative_hint(db, &cand_src.file_path);
                        if subsystem_files.contains(cand_rel_path) {
                            score += 1500; // High subsystem relevance
                        }

                        if let Some(qv) = query_vector {
                            if let Ok(mut stmt) = db.conn.prepare("SELECT embedding FROM symbol_embeddings WHERE symbol_id = (SELECT id FROM symbols WHERE name = ?1 AND file_id = (SELECT id FROM files WHERE path = ?2) LIMIT 1)") {
                                if let Ok(mut rows) = stmt.query(rusqlite::params![cand_src.symbol_name, cand_rel_path]) {"""

content = content.replace(old_context, new_context)

# Lower the threshold slightly just in case
content = content.replace("if score > 300 {", "if score > 200 {")

with open('mcp/src/main.rs', 'w') as f:
    f.write(content)

print("Fix applied successfully")
