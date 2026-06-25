use storage::Database;
use query::context::ContextResponseBuilder;
use query::graph::{explore_graph, GraphDirection};
use query::response::ResponseProfile;

#[test]
fn test_graph_invariants() {
    let db = Database::new("../.codebroker/codebroker.db").expect("Database not found");

    // 1. No orphaned symbol references
    let orphaned_edges: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM edges 
         WHERE (source_symbol_id IS NOT NULL AND source_symbol_id NOT IN (SELECT id FROM symbols))
            OR target_symbol_id NOT IN (SELECT id FROM symbols)",
        [],
        |r| r.get(0)
    ).unwrap();
    assert_eq!(orphaned_edges, 0, "Found orphaned edges pointing to non-existent symbols");

    // 2. No duplicate edges
    let duplicate_edges: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM (
            SELECT source_file_id, source_symbol_id, target_symbol_id, kind 
            FROM edges 
            GROUP BY source_file_id, source_symbol_id, target_symbol_id, kind 
            HAVING COUNT(*) > 1
         )",
        [],
        |r| r.get(0)
    ).unwrap();
    assert_eq!(duplicate_edges, 0, "Found duplicate edges in the graph");

    // 3. Consistency between Context Assembly and Graph Traversal
    let mut stmt = db.conn.prepare("SELECT id, name FROM symbols LIMIT 50").unwrap();
    let mut rows = stmt.query([]).unwrap();
    
    while let Some(row) = rows.next().unwrap() {
        let sym_id: i64 = row.get(0).unwrap();
        let sym_name: String = row.get(1).unwrap();

        let builder = ContextResponseBuilder::new(&db, &sym_name, None, ResponseProfile::Standard)
            .unwrap();
            
        if let Some(ctx) = builder {
            // Check Callers consistency
            let mut ctx_callers = ctx.fetch_callers().unwrap();
            ctx_callers.sort();
            
            let graph_incoming = explore_graph(&db, &sym_name, 1, GraphDirection::Incoming, 100).unwrap();
            let mut expected_callers: Vec<String> = graph_incoming.edges.iter()
                .filter(|e| e.kind == "calls")
                .map(|e| graph_incoming.nodes.iter().find(|n| n.id == e.source).unwrap().name.clone())
                .collect();
            expected_callers.sort();
            expected_callers.dedup();
            
            assert_eq!(ctx_callers, expected_callers, "Callers mismatch for symbol: {}", sym_name);

            // Check Callees consistency
            let mut ctx_callees = ctx.fetch_callees().unwrap();
            ctx_callees.sort();
            
            let graph_outgoing = explore_graph(&db, &sym_name, 1, GraphDirection::Outgoing, 100).unwrap();
            let mut expected_callees: Vec<String> = graph_outgoing.edges.iter()
                .filter(|e| e.kind == "calls")
                .map(|e| graph_outgoing.nodes.iter().find(|n| n.id == e.target).unwrap().name.clone())
                .collect();
            expected_callees.sort();
            expected_callees.dedup();
            
            assert_eq!(ctx_callees, expected_callees, "Callees mismatch for symbol: {}", sym_name);
            
            // Check Self-loops
            assert!(!ctx_callers.contains(&sym_name), "Self-loop detected in callers for {}", sym_name);
            assert!(!ctx_callees.contains(&sym_name), "Self-loop detected in callees for {}", sym_name);
        }
    }
}
