use std::collections::HashMap;
use std::time::Instant;
use storage::Database;

pub fn extract_features(db: &Database) -> Result<(), String> {
    let t_total = Instant::now();
    // We compute:
    // pagerank, fan_in, fan_out, interaction_count, community_id
    // and composable booleans.

    // 1. Fetch all symbols (joined to their file path so entrypoint
    //    classification can see Next.js-style file conventions, not just the
    //    symbol kind/decorators).
    let t0 = Instant::now();
    let mut stmt = db
        .conn
        .prepare(
            "SELECT symbols.id, symbols.kind, symbols.name, symbols.attributes, files.path
         FROM symbols JOIN files ON symbols.file_id = files.id",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;

    struct SymInfo {
        fan_in: i64,
        fan_out: i64,
        interaction_count: i64,
        is_entrypoint: bool,
        is_exported: bool,
        is_public: bool,
        is_callable: bool,
        is_type: bool,
        is_constant: bool,
        is_local: bool,
        is_generated: bool,
    }

    let mut symbols: HashMap<i64, SymInfo> = HashMap::new();
    while let Ok(Some(row)) = rows.next() {
        let id: i64 = row.get(0).unwrap();
        let kind: String = row.get(1).unwrap();
        let name: String = row.get(2).unwrap();
        let attributes: Option<String> = row.get(3).unwrap_or(None);
        let path: String = row.get(4).unwrap_or_default();

        let kind_lower = kind.to_lowercase();
        let is_callable = ["function", "method", "route", "endpoint", "lambda", "macro"]
            .contains(&kind_lower.as_str());
        let is_type =
            ["class", "interface", "struct", "enum", "type"].contains(&kind_lower.as_str());
        let is_local = [
            "variable",
            "local",
            "parameter",
            "field",
            "property",
            "import",
        ]
        .contains(&kind_lower.as_str());
        let is_constant = ["constant"].contains(&kind_lower.as_str());
        // Entrypoint detection is delegated to the single shared classifier
        // (storage::entrypoints) so every tool agrees on what an entrypoint is.
        // It recognises decorator routes (FastAPI/Flask), explicit
        // route/page/layout kinds, and Next.js file conventions — replacing the
        // old `kind in [...]` + naive `contains("@")` heuristic that missed
        // both `@app.websocket` routes and `app/**/page.tsx` pages.
        let is_entrypoint = storage::entrypoints::classify_entrypoint_json(
            &name,
            &kind,
            &path,
            attributes.as_deref(),
        )
        .is_some();

        // Very basic approximations
        let is_exported = !name.starts_with('_');
        let is_public = !name.starts_with('_');
        let is_generated = name.contains("generated");

        symbols.insert(
            id,
            SymInfo {
                fan_in: 0,
                fan_out: 0,
                interaction_count: 0,
                is_entrypoint,
                is_exported,
                is_public,
                is_callable,
                is_type,
                is_constant,
                is_local,
                is_generated,
            },
        );
    }
    eprintln!(
        "[TIMING:features] Load symbols: {}ms ({} symbols)",
        t0.elapsed().as_millis(),
        symbols.len()
    );

    // 2. Compute fan_in, fan_out, interactions, and collect edges for PageRank/Community
    let t1 = Instant::now();
    let mut edge_stmt = db
        .conn
        .prepare("SELECT source_symbol_id, target_symbol_id, kind FROM edges")
        .map_err(|e| e.to_string())?;
    let mut edge_rows = edge_stmt.query([]).map_err(|e| e.to_string())?;

    let mut out_edges: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut in_edges: HashMap<i64, Vec<i64>> = HashMap::new();

    while let Ok(Some(row)) = edge_rows.next() {
        let source_id: Option<i64> = row.get(0).unwrap_or(None);
        let target_id: i64 = row.get(1).unwrap();
        let edge_kind: String = row.get(2).unwrap();

        if let Some(src) = source_id {
            if let Some(sym) = symbols.get_mut(&src) {
                sym.fan_out += 1;
                if edge_kind == "interaction" {
                    sym.interaction_count += 1;
                }
            }
            out_edges.entry(src).or_default().push(target_id);
            in_edges.entry(target_id).or_default().push(src);
        }

        if let Some(sym) = symbols.get_mut(&target_id) {
            sym.fan_in += 1;
            if edge_kind == "interaction" {
                sym.interaction_count += 1;
            }
        }
    }

    let total_edges = out_edges.values().map(|v| v.len()).sum::<usize>();
    eprintln!(
        "[TIMING:features] Load edges: {}ms ({} graph edges)",
        t1.elapsed().as_millis(),
        total_edges
    );

    // 3. Compute PageRank
    let t2 = Instant::now();
    let mut pagerank: HashMap<i64, f64> = symbols.keys().map(|&k| (k, 1.0)).collect();
    let damping = 0.85;
    for _ in 0..10 {
        let mut new_pr = HashMap::new();
        for &id in symbols.keys() {
            let mut sum = 0.0;
            if let Some(ins) = in_edges.get(&id) {
                for &in_id in ins {
                    let out_count = out_edges.get(&in_id).map(|e| e.len()).unwrap_or(1) as f64;
                    let pr = pagerank.get(&in_id).unwrap_or(&1.0);
                    sum += pr / out_count;
                }
            }
            new_pr.insert(id, (1.0 - damping) + damping * sum);
        }
        pagerank = new_pr;
    }

    eprintln!("[TIMING:features] PageRank (10 iter): {}ms", t2.elapsed().as_millis());

    // 4. Compute Communities (Label Propagation)
    let t3 = Instant::now();
    let mut community: HashMap<i64, i64> = symbols.keys().map(|&k| (k, k)).collect();
    for _ in 0..5 {
        // max iterations
        let mut changes = false;
        for &id in symbols.keys() {
            let mut label_counts = HashMap::new();
            if let Some(outs) = out_edges.get(&id) {
                for &neighbor in outs {
                    let lbl = *community.get(&neighbor).unwrap();
                    *label_counts.entry(lbl).or_insert(0) += 1;
                }
            }
            if let Some(ins) = in_edges.get(&id) {
                for &neighbor in ins {
                    let lbl = *community.get(&neighbor).unwrap();
                    *label_counts.entry(lbl).or_insert(0) += 1;
                }
            }
            if !label_counts.is_empty() {
                let best_label = label_counts
                    .into_iter()
                    .max_by_key(|&(_, count)| count)
                    .unwrap()
                    .0;
                if community[&id] != best_label {
                    community.insert(id, best_label);
                    changes = true;
                }
            }
        }
        if !changes {
            break;
        }
    }

    eprintln!("[TIMING:features] Community detection: {}ms", t3.elapsed().as_millis());

    // 5. Update SQLite Database. DELETE + the re-INSERT loop below share one
    // transaction so a concurrent WAL reader (e.g. an MCP query mid-flight)
    // never observes the table in its briefly-empty state between the two.
    let t4 = Instant::now();
    let _ = db.conn.execute_batch("BEGIN IMMEDIATE");
    let _ = db.conn.execute("DELETE FROM symbol_features", []);
    eprintln!("[TIMING:features] DELETE symbol_features: {}ms", t4.elapsed().as_millis());

    let t5 = Instant::now();
    {
        let mut stmt = db.conn.prepare_cached(
            "INSERT INTO symbol_features
             (symbol_id, pagerank, fan_in, fan_out, interaction_count, community_id,
              is_entrypoint, is_exported, is_public, is_callable, is_type, is_constant, is_local, is_generated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        ).map_err(|e| e.to_string())?;
        for (id, sym) in &symbols {
            let pr = pagerank.get(id).unwrap_or(&0.0);
            let cid = community.get(id).unwrap_or(id);
            let _ = stmt.execute(rusqlite::params![id, pr, sym.fan_in, sym.fan_out, sym.interaction_count, cid,
                sym.is_entrypoint, sym.is_exported, sym.is_public, sym.is_callable, sym.is_type, sym.is_constant, sym.is_local, sym.is_generated]);
        }
    }
    let _ = db.conn.execute_batch("COMMIT");
    let insert_count = symbols.len();
    eprintln!(
        "[TIMING:features] INSERT symbol_features ({} rows, batched in one transaction): {}ms ({:.2}ms/row)",
        insert_count,
        t5.elapsed().as_millis(),
        t5.elapsed().as_millis() as f64 / insert_count.max(1) as f64
    );
    eprintln!("[TIMING:features] Total extract_features: {}ms", t_total.elapsed().as_millis());

    Ok(())
}
