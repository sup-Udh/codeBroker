fn generate_context_capsule(
    db: &storage::Database,
    query: &str,
    file_hint: Option<&str>,
    semantic_tokens: &[String],
    query_vector: Option<&[f32]>,
) -> String {
    use std::fmt::Write;
    let mut md = String::new();
    let _ = writeln!(md, "# CodeBroker Context Capsule\n");
    let _ = writeln!(md, "**Query:** {}\n", query);

    // 1. Fetch initial candidates via search_symbols
    let (mut results, _reason) = query::engine::search_symbols(
        db,
        query,
        semantic_tokens,
        query_vector,
        false,
        file_hint,
        query::engine::SearchMode::Both,
        false,
        None,
    ).unwrap_or_else(|_| (vec![], None));

    results.retain(|r| r.kind != "file" && r.kind != "text_match");

    let is_conceptual = {
        let mut words = query.split_whitespace();
        words.any(|w| query::concepts::concepts_matching_term(w).next().is_some())
    };
    
    let highest_conf = results.first().map(|r| r.confidence.as_str()).unwrap_or("Low");
    let needs_subsystem = highest_conf.starts_with("Low") || is_conceptual;

    let mut subsystem_files = std::collections::HashSet::new();
    let mut subsystem_confidence = "Low".to_string();

    if needs_subsystem {
        if let Ok(stats) = query::subsystem::discover_subsystem(db, query, semantic_tokens, query_vector) {
            subsystem_confidence = stats.confidence.clone();
            for f in stats.files {
                subsystem_files.insert(f);
            }
            // Boost existing results that are in subsystem
            for r in &mut results {
                if subsystem_files.contains(&r.path) {
                    r.score += 2000;
                    if !r.confidence.starts_with("High") {
                        r.confidence = "High (Subsystem Anchor)".to_string();
                    }
                }
            }
            results.sort_by(|a, b| b.score.cmp(&a.score));
        }
    }

    let mut pivots = Vec::new();
    for r in results.into_iter().take(3) {
        let reason = if r.confidence.starts_with("High (Subsystem") {
            "Anchor point discovered via graph expansion of the subsystem."
        } else if r.confidence.starts_with("High (Semantic") {
            "Strong semantic vector match for the conceptual query."
        } else if r.confidence.starts_with("High") {
            "Exact or highly confident lexical match."
        } else if r.confidence.starts_with("Medium") {
            "Partial semantic or lexical match."
        } else {
            "Fuzzy fallback match."
        };
        pivots.push((r.name.clone(), r.path.clone(), reason.to_string(), r.confidence.clone()));
    }

    if pivots.is_empty() {
        let _ = writeln!(
            md,
            "_No matching symbols found for this query. Try search_codebase with mode: \"both\" for a broader sweep._"
        );
        return md;
    }

    // Capsule Confidence
    let capsule_conf = pivots.iter().map(|p| p.3.clone()).max().unwrap_or("Low".to_string());
    let capsule_conf = if subsystem_confidence.starts_with("High") { "High (Subsystem Validated)".to_string() } else { capsule_conf };
    let _ = writeln!(md, "**Capsule Confidence:** {}\n", capsule_conf);

    let _ = writeln!(md, "## Pivot Symbols (Full Implementation)\n");

    let mut seen_support: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let mut support_sections: Vec<(String, i32, String)> = Vec::new(); // (markdown, score, path)

    for (name, hint, reason, _) in &pivots {
        let rel_hint = relative_hint(db, hint);
        let sources = query::retrieval::read_symbol_source_scoped(db, name, false, Some(rel_hint))
            .unwrap_or_default();
        let Some(src) = sources.into_iter().next() else {
            continue;
        };

        let _ = writeln!(md, "### `{}::{}`", src.file_path, src.symbol_name);
        let _ = writeln!(md, "*Selection Reasoning:* {}\n", reason);

        let src_body = if src.file_path.ends_with(".json") {
            "/* JSON symbol – no source body */".to_string()
        } else {
            let lines: Vec<&str> = src.source.lines().collect();
            let total = lines.len();
            let max_lines = 100;
            if total > max_lines {
                let hidden = total - max_lines;
                let mut out = lines[..max_lines].join("\n");
                out.push_str(&format!("\n    ... // [{} lines hidden for token reduction]", hidden));
                out
            } else {
                src.source.clone()
            }
        };

        let _ = writeln!(md, "```\n{}\n```\n", src_body);
        seen_support.insert((src.symbol_name.clone(), src.file_path.clone()));

        // Gather adjacent symbols for relevance scoring
        if let Ok(Some(ctx)) = query::context::ContextObject::assemble_scoped(db, name, Some(rel_hint)) {
            let mut candidates = Vec::new();
            for d in ctx.forward_dependencies { candidates.push((d, "Forward Dependency")); }
            for d in ctx.same_file_callers { candidates.push((d, "Same-File Caller")); }
            for d in ctx.reverse_dependencies { candidates.push((d, "Reverse Dependency")); }
            for d in ctx.callees { candidates.push((d, "Callee")); }
            for d in ctx.callers { candidates.push((d, "Caller")); }

            for (cand, rel_type) in candidates {
                if let Ok(cand_sources) = query::retrieval::read_symbol_source_scoped(db, &cand, true, None) {
                    for cand_src in cand_sources {
                        if seen_support.contains(&(cand_src.symbol_name.clone(), cand_src.file_path.clone())) {
                            continue;
                        }
                        
                        // Compute Relevance Score
                        let mut score = 0;
                        if cand_src.file_path.contains(query) || cand_src.symbol_name.contains(query) {
                            score += 500;
                        }
                        for t in semantic_tokens {
                            if cand_src.symbol_name.contains(t) || cand_src.file_path.contains(t) {
                                score += 100;
                            }
                        }
                        if subsystem_files.contains(&cand_src.file_path) {
                            score += 1000; // High subsystem relevance
                        }
                        
                        if let Some(qv) = query_vector {
                            if let Ok(mut stmt) = db.conn.prepare("SELECT embedding FROM symbol_embeddings WHERE symbol_id = (SELECT id FROM symbols WHERE name = ?1 AND file_id = (SELECT id FROM files WHERE path = ?2) LIMIT 1)") {
                                if let Ok(mut rows) = stmt.query(rusqlite::params![cand_src.symbol_name, cand_src.file_path]) {
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
                        }

                        if score > 300 {
                            let md_block = format!(
                                "### `{}::{}` (Skeleton)\n*Relationship to {}*: {}\n```\n{}\n```\n",
                                cand_src.file_path, cand_src.symbol_name, src.symbol_name, rel_type, cand_src.source
                            );
                            support_sections.push((md_block, score, cand_src.file_path.clone()));
                            seen_support.insert((cand_src.symbol_name.clone(), cand_src.file_path.clone()));
                        }
                    }
                }
            }
        }
    }

    let _ = writeln!(md, "## Supporting Context (Relevant Adjacencies)\n");
    if support_sections.is_empty() {
        let _ = writeln!(md, "_No highly relevant supporting context found._");
    } else {
        // Apply diversity scoring: penalize items from the same file path
        support_sections.sort_by(|a, b| b.1.cmp(&a.1));
        let mut final_support = Vec::new();
        let mut path_counts = std::collections::HashMap::new();
        let mut token_budget = 2000;

        for (md_block, mut score, path) in support_sections {
            let count = path_counts.entry(path.clone()).or_insert(0);
            score -= *count * 200; // penalize 200 points per prior inclusion from same path
            if score > 0 {
                // insert and re-sort
                final_support.push((md_block.clone(), score));
                *count += 1;
            }
        }
        
        final_support.sort_by(|a, b| b.1.cmp(&a.1));

        for (md_block, _) in final_support {
            let approx_tokens = md_block.split_whitespace().count();
            if token_budget >= approx_tokens {
                let _ = write!(md, "{}", md_block);
                token_budget -= approx_tokens;
            }
            if token_budget <= 0 {
                break;
            }
        }
    }

    md
}
