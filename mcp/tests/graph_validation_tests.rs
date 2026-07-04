use query::context::ContextResponseBuilder;
use query::graph::MIN_CONFIDENCE_FOR_RECEIVER_EDGES;
use query::response::ResponseProfile;
use storage::Database;

#[test]
fn test_graph_invariants() {
    let db = Database::new("../.codebroker/codebroker.db").expect("Database not found");

    // 1. No orphaned symbol references
    let orphaned_edges: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM edges 
         WHERE (source_symbol_id IS NOT NULL AND source_symbol_id NOT IN (SELECT id FROM symbols))
            OR target_symbol_id NOT IN (SELECT id FROM symbols)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        orphaned_edges, 0,
        "Found orphaned edges pointing to non-existent symbols"
    );

    // 2. No duplicate edges
    let duplicate_edges: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM (
            SELECT source_file_id, source_symbol_id, target_symbol_id, kind 
            FROM edges 
            GROUP BY source_file_id, source_symbol_id, target_symbol_id, kind 
            HAVING COUNT(*) > 1
         )",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(duplicate_edges, 0, "Found duplicate edges in the graph");

    // 3. Consistency between Context Assembly and Graph Traversal
    let mut stmt = db
        .conn
        .prepare("SELECT id, name FROM symbols LIMIT 50")
        .unwrap();
    let mut rows = stmt.query([]).unwrap();

    while let Some(row) = rows.next().unwrap() {
        let sym_id: i64 = row.get(0).unwrap();
        let sym_name: String = row.get(1).unwrap();

        let builder =
            ContextResponseBuilder::new(&db, &sym_name, None, ResponseProfile::Standard).unwrap();

        if let Some(ctx) = builder {
            // `sym_name` can be ambiguous (many methods share a name like "new");
            // `ContextResponseBuilder` and `fetch_callers`/`fetch_callees` operate
            // on the one specific symbol id it resolved to, so the comparison
            // query below must filter by that same id rather than by name.
            let Some(ctx_symbol_id) = ctx.symbol_id else {
                continue;
            };

            // Check Callers consistency. `fetch_callers` treats a `calls` edge
            // (any confidence) or a high-confidence `method_call` edge (see
            // MIN_CONFIDENCE_FOR_RECEIVER_EDGES) as a caller — mirrored here via
            // direct SQL since `explore_graph`'s edges don't carry confidence.
            let mut ctx_callers = ctx.fetch_callers().unwrap();
            ctx_callers.sort();

            let mut callers_stmt = db
                .conn
                .prepare(
                    "SELECT DISTINCT s1.name FROM edges
                     JOIN symbols s1 ON edges.source_symbol_id = s1.id
                     WHERE edges.target_symbol_id = ?1
                     AND (edges.kind = 'calls' OR (edges.kind = 'method_call' AND edges.confidence >= ?2))
                     AND edges.source_symbol_id != edges.target_symbol_id",
                )
                .unwrap();
            let mut expected_callers: Vec<String> = callers_stmt
                .query_map(
                    rusqlite::params![ctx_symbol_id, MIN_CONFIDENCE_FOR_RECEIVER_EDGES],
                    |r| r.get::<_, String>(0),
                )
                .unwrap()
                .map(|r| r.unwrap())
                .collect();
            expected_callers.sort();
            expected_callers.dedup();

            assert_eq!(
                ctx_callers, expected_callers,
                "Callers mismatch for symbol: {}",
                sym_name
            );

            // Check Callees consistency (same predicate, opposite direction).
            let mut ctx_callees = ctx.fetch_callees().unwrap();
            ctx_callees.sort();

            let mut callees_stmt = db
                .conn
                .prepare(
                    "SELECT DISTINCT s2.name FROM edges
                     JOIN symbols s2 ON edges.target_symbol_id = s2.id
                     WHERE edges.source_symbol_id = ?1
                     AND (edges.kind = 'calls' OR (edges.kind = 'method_call' AND edges.confidence >= ?2))
                     AND edges.source_symbol_id != edges.target_symbol_id",
                )
                .unwrap();
            let mut expected_callees: Vec<String> = callees_stmt
                .query_map(
                    rusqlite::params![ctx_symbol_id, MIN_CONFIDENCE_FOR_RECEIVER_EDGES],
                    |r| r.get::<_, String>(0),
                )
                .unwrap()
                .map(|r| r.unwrap())
                .collect();
            expected_callees.sort();
            expected_callees.dedup();

            assert_eq!(
                ctx_callees, expected_callees,
                "Callees mismatch for symbol: {}",
                sym_name
            );

            // Check Self-loops
            assert!(
                !ctx_callers.contains(&sym_name),
                "Self-loop detected in callers for {}",
                sym_name
            );
            assert!(
                !ctx_callees.contains(&sym_name),
                "Self-loop detected in callees for {}",
                sym_name
            );
        }
    }

    // 4. Caller Kind Invariant
    let invalid_callers: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM edges 
         JOIN symbols ON edges.source_symbol_id = symbols.id
         WHERE edges.kind = 'calls' AND symbols.kind IN ('variable', 'constant', 'parameter', 'local', 'property', 'field', 'import')",
        [],
        |r| r.get(0)
    ).unwrap();
    assert_eq!(
        invalid_callers, 0,
        "Found 'calls' edges originating from invalid assignment targets (variables, parameters, etc.)"
    );

    // 5. Shortest path BFS validation
    // Let's pick two connected nodes and ensure shortest_path finds a path, and every hop is valid
    let edge_exists: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM edges WHERE kind = 'calls'", [], |r| {
            r.get(0)
        })
        .unwrap();
    if edge_exists > 0 {
        let (source, target): (String, String) = db
            .conn
            .query_row(
                "SELECT s1.name, s2.name FROM edges 
             JOIN symbols s1 ON edges.source_symbol_id = s1.id 
             JOIN symbols s2 ON edges.target_symbol_id = s2.id 
             WHERE edges.kind = 'calls' LIMIT 1",
                [],
                |r| Ok((r.get(0).unwrap(), r.get(1).unwrap())),
            )
            .unwrap();

        let path = query::graph::shortest_path(&db, &source, &target, None, None).unwrap();
        assert!(path.found, "Shortest path should find an existing edge");
        assert_eq!(path.nodes.first().unwrap().symbol_name, source);
        assert_eq!(path.nodes.last().unwrap().symbol_name, target);

        // Verify each hop exists in DB
        for hop in &path.edges {
            let hop_exists: i64 = db
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM edges 
                 JOIN symbols s1 ON edges.source_symbol_id = s1.id 
                 JOIN symbols s2 ON edges.target_symbol_id = s2.id 
                 WHERE s1.name = ?1 AND s2.name = ?2",
                    rusqlite::params![hop.source, hop.target],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(hop_exists > 0, "Shortest path returned an invalid hop");
        }
    }
}
