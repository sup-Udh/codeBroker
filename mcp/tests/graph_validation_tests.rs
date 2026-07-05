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

        let path = query::graph::shortest_path(&db, &source, &target, None, None, None, None).unwrap();
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

fn fixture_symbol(name: &str, kind: &str, start_line: usize, end_line: usize) -> graph::SymbolNode {
    graph::SymbolNode {
        name: name.to_string(),
        kind: kind.to_string(),
        start_line,
        end_line,
        start_byte: 0,
        end_byte: 0,
        signature: None,
        attributes: Vec::new(),
        metadata: None,
    }
}

// Regression for Phase 21's core defect: `resolver::resolve_symbol` computes
// a definitive `id` (using a `line_hint` to disambiguate two same-named
// symbols in one file), but every id-aware tool used to discard it and
// re-resolve "the same" symbol a second time by name+file_hint alone — which
// has no way to tell the two apart and silently picks whichever row sorts
// first. This asserts the id, once resolved, is what every downstream tool
// actually operates on.
#[test]
fn symbol_id_threading_prevents_same_name_divergence() {
    let db = Database::new(":memory:").unwrap();
    let file_id = db.insert_file("dup.ts", "h").unwrap();

    // Two symbols named "config" in the same file — file_hint alone can't
    // tell them apart, only an id (or a line_hint) can.
    let first_id = db
        .insert_symbol(file_id, &fixture_symbol("config", "function", 2, 4))
        .unwrap();
    let second_id = db
        .insert_symbol(file_id, &fixture_symbol("config", "method", 20, 22))
        .unwrap();
    assert_ne!(first_id, second_id);

    // Without a line hint, this is genuinely ambiguous.
    let ambiguous = resolver::resolve_symbol(&db, "config", Some("dup.ts"), None, None);
    assert!(
        ambiguous.is_ambiguous(),
        "two same-named symbols in one file must be ambiguous without a line hint"
    );

    // A line hint at the second definition resolves it deterministically.
    let resolved = resolver::resolve_symbol(&db, "config", Some("dup.ts"), None, Some(21));
    let resolved = match resolved {
        resolver::ResolvedEntity::Symbol(s) => s,
        other => panic!(
            "expected a definitive Symbol resolution with a line hint, got {:?}",
            other
        ),
    };
    assert_eq!(
        resolved.id, second_id,
        "line hint must resolve to the second definition"
    );

    // ContextResponseBuilder's id fast path must land on exactly that row.
    let ctx = ContextResponseBuilder::new_by_id(&db, resolved.id, ResponseProfile::Standard)
        .unwrap()
        .unwrap();
    assert_eq!(ctx.symbol_id, Some(second_id));
    assert_eq!(ctx.target_kind, "method");

    // explore_graph_scoped's id fast path must land on the same row too —
    // asserted via `kind`, since two same-named/same-file nodes are
    // otherwise indistinguishable in the response.
    let explored = query::graph::explore_graph_scoped(
        &db,
        "config",
        1,
        query::graph::GraphDirection::Both,
        50,
        Some("dup.ts"),
        Some(resolved.id),
    )
    .unwrap();
    assert_eq!(explored.nodes[0].id, second_id.to_string());
    assert_eq!(explored.nodes[0].kind, "method");

    // Sanity check proving this fix actually matters: without an id, the
    // by-name/file_hint lookup can't disambiguate the two same-file rows and
    // deterministically picks the lowest id (the FIRST/wrong definition) —
    // exactly the divergence the id fast path exists to prevent.
    let explored_without_id = query::graph::explore_graph_scoped(
        &db,
        "config",
        1,
        query::graph::GraphDirection::Both,
        50,
        Some("dup.ts"),
        None,
    )
    .unwrap();
    assert_eq!(explored_without_id.nodes[0].id, first_id.to_string());

    // graph_subtree's id fast path, same check via `kind`.
    let subtree =
        query::graph::graph_subtree(&db, "config", 1, None, Some("dup.ts"), Some(resolved.id))
            .unwrap();
    assert_eq!(subtree.nodes[0].kind, "method");

    // shortest_path: only the second definition has an outgoing edge to
    // `sink`, so `to_id` selecting the wrong same-named row must report
    // `found: false` even though a real path exists to the intended one.
    let sink_id = db
        .insert_symbol(file_id, &fixture_symbol("sink", "function", 40, 42))
        .unwrap();
    db.insert_edge_attributed(file_id, Some(second_id), sink_id, "calls")
        .unwrap();

    let found_via_correct_id = query::graph::shortest_path(
        &db,
        "config",
        "sink",
        Some("dup.ts"),
        Some("dup.ts"),
        Some(second_id),
        None,
    )
    .unwrap();
    assert!(
        found_via_correct_id.found,
        "shortest_path must find the real config(2nd decl) -> sink edge when given the resolved id"
    );

    let not_found_via_wrong_id = query::graph::shortest_path(
        &db,
        "config",
        "sink",
        Some("dup.ts"),
        Some("dup.ts"),
        Some(first_id),
        None,
    )
    .unwrap();
    assert!(
        !not_found_via_wrong_id.found,
        "the first (unconnected) config definition must not report a path to sink"
    );
}

// Regression for Phase 21's path-canonicalization bug: `path_scope` on
// architectural_hotspots/dependency_cycles/etc. used to be matched with a
// raw `path.contains(scope)` with no normalization, so Windows-style
// backslashes or a project-root-prefixed absolute path silently matched
// nothing even though the equivalent forward-slash relative scope worked.
#[test]
fn path_scope_normalization_handles_backslashes_and_root_prefix() {
    let mut db = Database::new(":memory:").unwrap();
    db.project_root = "/repo".to_string();

    let target_file = db
        .insert_file("apps/web/modules/auth/handler.ts", "h")
        .unwrap();
    let target_id = db
        .insert_symbol(target_file, &fixture_symbol("handleAuth", "function", 1, 5))
        .unwrap();
    let caller_file = db.insert_file("apps/web/other.ts", "h").unwrap();
    let caller_id = db
        .insert_symbol(caller_file, &fixture_symbol("caller", "function", 1, 3))
        .unwrap();
    db.insert_edge_attributed(caller_file, Some(caller_id), target_id, "calls")
        .unwrap();

    let forward_slash_scope =
        resolver::CanonicalNameResolver::normalize_path(&db, "apps/web/modules/auth");
    let backslash_scope =
        resolver::CanonicalNameResolver::normalize_path(&db, "apps\\web\\modules\\auth");
    let root_prefixed_scope =
        resolver::CanonicalNameResolver::normalize_path(&db, "/repo/apps/web/modules/auth");

    assert_eq!(forward_slash_scope, "apps/web/modules/auth");
    assert_eq!(
        backslash_scope, forward_slash_scope,
        "backslashes must normalize to the same scope as forward slashes"
    );
    assert_eq!(
        root_prefixed_scope, forward_slash_scope,
        "a project-root-prefixed absolute path must normalize to the same scope"
    );

    let expected =
        query::graph::architectural_hotspots(&db, 10, Some(forward_slash_scope.as_str())).unwrap();
    assert!(
        !expected.top_hotspots.is_empty(),
        "fixture must actually produce a hotspot to be a meaningful test"
    );

    let via_backslash =
        query::graph::architectural_hotspots(&db, 10, Some(backslash_scope.as_str())).unwrap();
    let via_root_prefix =
        query::graph::architectural_hotspots(&db, 10, Some(root_prefixed_scope.as_str())).unwrap();

    assert_eq!(via_backslash.top_hotspots.len(), expected.top_hotspots.len());
    assert_eq!(via_root_prefix.top_hotspots.len(), expected.top_hotspots.len());
}

// Regression for Phase 21's subsystem-consistency bug: `resolve_subsystem`
// used to pass the raw input straight into `discover_subsystem` with no
// alias canonicalization, even though `CanonicalNameResolver::resolve_subsystem_name`
// (mapping "authentication"/"AUTH"/"login" -> "auth") already existed and was
// simply never called. `subsystem_stats`, `subsystem_communication`, and
// `prepare_context` all gate through `resolve_subsystem`, so this is the one
// place that needs to agree for all three to agree.
#[test]
fn subsystem_alias_canonicalization_converges_on_the_same_subsystem() {
    let db = Database::new(":memory:").unwrap();
    let file_id = db.insert_file("src/auth/index.ts", "h").unwrap();
    db.insert_symbol(file_id, &fixture_symbol("auth", "function", 1, 5))
        .unwrap();

    let via_canonical = resolver::resolve_subsystem(&db, "auth", &[], None);
    let via_alias = resolver::resolve_subsystem(&db, "authentication", &[], None);
    let via_case_variant = resolver::resolve_subsystem(&db, "AUTH", &[], None);

    let (resolver::ResolvedEntity::Subsystem(canonical), resolver::ResolvedEntity::Subsystem(alias), resolver::ResolvedEntity::Subsystem(case_variant)) =
        (via_canonical, via_alias, via_case_variant)
    else {
        panic!("all three spellings of the same subsystem must resolve confidently");
    };

    assert_eq!(
        canonical.files, alias.files,
        "'authentication' must converge on the same file set as 'auth' via alias canonicalization"
    );
    assert_eq!(
        canonical.files, case_variant.files,
        "'AUTH' must converge on the same file set as 'auth'"
    );

    // Sanity check proving the alias table is doing real work: the raw,
    // un-canonicalized "authentication" string has no lexical relationship
    // to a symbol/file named "auth", so calling discover_subsystem directly
    // (bypassing resolve_subsystem's alias step) must NOT find it.
    let raw_alias_lookup = query::subsystem::discover_subsystem(
        &db,
        "authentication",
        &[],
        None,
        query::subsystem::TraversalScope::Expanded,
    )
    .unwrap();
    assert!(
        raw_alias_lookup.files.is_empty() || raw_alias_lookup.confidence == "Low",
        "without alias canonicalization, 'authentication' should not confidently match the 'auth' fixture"
    );
}

// Regression for Phase 21's scoped-traversal bug: `discover_subsystem` used
// to always run the same fixed 3-hop cohesion expansion with no caller
// control, which is the literal mechanism behind "subsystem_stats(auth) ->
// 400 files -> token explosion". `TraversalScope` now lets a caller ask for
// just the seeds (`Strict`), today's default (`Expanded`), or a wider radius
// (`Full`) — this proves the three scopes actually produce different-sized
// results on a fixture shaped like a dependency chain.
#[test]
fn traversal_scope_controls_expansion_breadth() {
    let db = Database::new(":memory:").unwrap();

    // A linear chain: auth -> n1 -> n2 -> n3 -> n4, each link a real edge.
    // Only "auth" is a seed match (n1..n4 live under an unrelated "util"
    // path with no lexical relationship to "auth" — otherwise the path
    // substring match alone would seed-match them directly, defeating the
    // point of a test about hop expansion); n1..n4 are only reachable via
    // hop expansion, one additional hop per link in the chain.
    let auth_file = db.insert_file("src/auth/index.ts", "h").unwrap();
    let auth_id = db
        .insert_symbol(auth_file, &fixture_symbol("auth", "function", 1, 5))
        .unwrap();

    let mut prev_file = auth_file;
    let mut prev_id = auth_id;
    for i in 1..=4 {
        let file = db
            .insert_file(&format!("src/util/n{}.ts", i), "h")
            .unwrap();
        let id = db
            .insert_symbol(file, &fixture_symbol(&format!("n{}", i), "function", 1, 3))
            .unwrap();
        db.insert_edge_attributed(prev_file, Some(prev_id), id, "calls")
            .unwrap();
        prev_file = file;
        prev_id = id;
    }

    let strict = query::subsystem::discover_subsystem(
        &db,
        "auth",
        &[],
        None,
        query::subsystem::TraversalScope::Strict,
    )
    .unwrap();
    let expanded = query::subsystem::discover_subsystem(
        &db,
        "auth",
        &[],
        None,
        query::subsystem::TraversalScope::Expanded,
    )
    .unwrap();
    let full = query::subsystem::discover_subsystem(
        &db,
        "auth",
        &[],
        None,
        query::subsystem::TraversalScope::Full,
    )
    .unwrap();

    assert_eq!(
        strict.files.len(),
        1,
        "strict scope must return only the seed match, no hop expansion"
    );
    assert_eq!(strict.scope, "strict");
    assert!(!strict.truncated);

    // Expanded allows up to 3 hops: auth (seed) + n1, n2, n3.
    assert_eq!(
        expanded.files.len(),
        4,
        "expanded scope must walk up to 3 hops past the seed"
    );
    assert_eq!(expanded.scope, "expanded");

    // Full allows up to 8 hops, enough to reach the whole 5-node chain.
    assert_eq!(
        full.files.len(),
        5,
        "full scope must walk far enough to reach the entire chain"
    );
    assert_eq!(full.scope, "full");
    assert!(
        full.files.len() > expanded.files.len(),
        "full must see strictly more than expanded on a chain longer than 3 hops"
    );
}

// Regression for Phase 21's "never dump entire repositories" requirement:
// even `TraversalScope::Full` (or, as here, a subsystem whose seed matches
// alone already exceed the cap) must not return an unbounded file list —
// it must truncate deterministically and say so via `truncated`, instead of
// silently handing back everything it found.
#[test]
fn subsystem_discovery_truncates_past_the_hard_cap_instead_of_dumping_everything() {
    let db = Database::new(":memory:").unwrap();

    // A "star": one seed symbol directly connected to more neighbors than
    // MAX_SUBSYSTEM_FILES allows, all in one hop — reached purely via graph
    // expansion (unrelated names/paths), not `search_symbols`'s own top-50
    // seed cap, so it's the hop-expansion cap being exercised here.
    let auth_file = db.insert_file("src/auth/index.ts", "h").unwrap();
    let auth_id = db
        .insert_symbol(auth_file, &fixture_symbol("auth", "function", 1, 5))
        .unwrap();

    let fixture_count = query::subsystem::MAX_SUBSYSTEM_FILES + 10;
    for i in 0..fixture_count {
        let file = db
            .insert_file(&format!("src/util/n{}.ts", i), "h")
            .unwrap();
        let id = db
            .insert_symbol(file, &fixture_symbol(&format!("n{}", i), "function", 1, 3))
            .unwrap();
        db.insert_edge_attributed(auth_file, Some(auth_id), id, "calls")
            .unwrap();
    }

    let stats = query::subsystem::discover_subsystem(
        &db,
        "auth",
        &[],
        None,
        query::subsystem::TraversalScope::Expanded,
    )
    .unwrap();

    assert!(
        stats.truncated,
        "a subsystem exceeding MAX_SUBSYSTEM_FILES must report truncated: true"
    );
    assert_eq!(
        stats.files.len(),
        query::subsystem::MAX_SUBSYSTEM_FILES,
        "the file list itself must be capped, not just flagged"
    );
}

// Regression for Phase 21's find_duplicate_logic bugs:
//   1. The tool's documented default (`min_length: 80`, described as
//      "character length") was silently reinterpreted as an AST *node count*
//      threshold whenever it was <= 100 — so the real, shipped default
//      required 80 AST nodes, a bar most small-to-medium copy-pasted
//      functions never clear. Fixed default is 15 node.
//   2. Duplicate groups required `files.len() > 1`, so two copy-pasted
//      functions in the *same* file were never reported at all.
// Uses real files + the real indexer (not hand-built symbol rows), since
// find_duplicate_logic reads source bytes straight off disk.
#[test]
fn find_duplicate_logic_detects_renamed_and_same_file_duplicates_with_default_threshold() {
    let unique = format!(
        "codebroker_test_duplicates_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let project_root = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&project_root).unwrap();
    std::fs::create_dir_all(project_root.join(".codebroker")).unwrap();

    // Cross-file pair: same logic, renamed identifiers, different files.
    std::fs::write(
        project_root.join("calc_a.ts"),
        "export function calcA(x: number, y: number): number {\n  \
           const sum = x + y;\n  \
           const doubled = sum * 2;\n  \
           const tripled = sum * 3;\n  \
           return doubled + tripled;\n}\n",
    )
    .unwrap();
    std::fs::write(
        project_root.join("calc_b.ts"),
        "export function calcB(p: number, q: number): number {\n  \
           const sum = p + q;\n  \
           const doubled = sum * 2;\n  \
           const tripled = sum * 3;\n  \
           return doubled + tripled;\n}\n",
    )
    .unwrap();

    // Same-file pair: distinct structural shape from the cross-file pair
    // above, so it forms its own independent duplicate group.
    std::fs::write(
        project_root.join("same_file.ts"),
        "export function sameFileA(m: number): number {\n  \
           const doubledValue = m * 2;\n  \
           const result = doubledValue + 1;\n  \
           return result;\n}\n\n\
         export function sameFileB(n: number): number {\n  \
           const doubledValue = n * 2;\n  \
           const result = doubledValue + 1;\n  \
           return result;\n}\n",
    )
    .unwrap();

    // Trivial one-liners, duplicated, but small enough that they must not
    // clear the min-node floor and pollute the report.
    std::fs::write(
        project_root.join("trivial.ts"),
        "export function noop1() {}\nexport function noop2() {}\n",
    )
    .unwrap();

    let db_path = project_root.join(".codebroker").join("codebroker.db");
    let db = Database::new(db_path.to_str().unwrap()).unwrap();
    db.init_schema().unwrap();
    let project_root_str = project_root.to_str().unwrap();
    indexer::reindex::reindex_paths(
        &db,
        project_root_str,
        &[
            "calc_a.ts".to_string(),
            "calc_b.ts".to_string(),
            "same_file.ts".to_string(),
            "trivial.ts".to_string(),
        ],
    )
    .unwrap();

    // Using the tool's real default (no explicit min_length override) —
    // this is the exact call shape `find_duplicate_logic`'s MCP handler
    // makes with no arguments.
    let default_min_length = 15;
    let report =
        query::duplicates::find_duplicate_logic(&db, default_min_length, None).unwrap();

    let names_in_group = |group: &query::duplicates::DuplicateGroup| -> Vec<String> {
        let mut names: Vec<String> = group.members.iter().map(|m| m.symbol_name.clone()).collect();
        names.sort();
        names
    };

    let cross_file_group = report
        .groups
        .iter()
        .find(|g| names_in_group(g).contains(&"calcA".to_string()));
    let cross_file_group = cross_file_group.expect(
        "calcA/calcB (renamed-identifier duplicates in different files) must be detected with the default threshold",
    );
    assert_eq!(names_in_group(cross_file_group), vec!["calcA", "calcB"]);

    let same_file_group = report
        .groups
        .iter()
        .find(|g| names_in_group(g).contains(&"sameFileA".to_string()));
    let same_file_group = same_file_group.expect(
        "sameFileA/sameFileB (renamed-identifier duplicates in the SAME file) must be detected — same-file duplicates must not be excluded",
    );
    assert_eq!(
        names_in_group(same_file_group),
        vec!["sameFileA", "sameFileB"]
    );

    for group in &report.groups {
        let names = names_in_group(group);
        assert!(
            !names.contains(&"noop1".to_string()) && !names.contains(&"noop2".to_string()),
            "trivial one-line functions below the node-count floor must not be reported as duplicates"
        );
    }

    std::fs::remove_dir_all(&project_root).ok();
}

// Phase 21/13 regression: explore_graph used to return an empty
// nodes/edges list with zero explanation when the root symbol wasn't
// found, indistinguishable from "this symbol genuinely has no
// callers/callees at all".
#[test]
fn explore_graph_reports_why_when_root_is_not_found() {
    let db = Database::new(":memory:").unwrap();
    let res = query::graph::explore_graph_scoped(
        &db,
        "definitelyDoesNotExist",
        2,
        query::graph::GraphDirection::Both,
        50,
        None,
        None,
    )
    .unwrap();

    assert!(res.nodes.is_empty());
    assert!(res.edges.is_empty());
    assert!(
        res.not_found_reason.is_some(),
        "an empty graph because the root wasn't found must say so explicitly"
    );
}

// Phase 21/13 regression: shortest_path used to report `found: false` for
// two very different situations — "an endpoint doesn't exist at all" vs.
// "both endpoints exist but there's genuinely no path" — with no way for a
// caller to tell them apart.
#[test]
fn shortest_path_distinguishes_missing_endpoint_from_no_path_found() {
    let db = Database::new(":memory:").unwrap();
    let file_id = db.insert_file("src/a.ts", "h").unwrap();
    db.insert_symbol(file_id, &fixture_symbol("existingSymbol", "function", 1, 3))
        .unwrap();

    // "from" doesn't exist at all: this must be flagged as a resolution
    // failure via `reason`, not a bare `found: false`.
    let missing_endpoint = query::graph::shortest_path(
        &db,
        "doesNotExist",
        "existingSymbol",
        None,
        None,
        None,
        None,
    )
    .unwrap();
    assert!(!missing_endpoint.found);
    assert!(
        missing_endpoint.reason.is_some(),
        "a nonexistent endpoint must set `reason`, not just `found: false`"
    );

    // Both endpoints exist but are unrelated: a legitimate graph answer,
    // `reason` must stay None (the caller shouldn't be told anything is
    // wrong — nothing is).
    let other_id = db
        .insert_symbol(file_id, &fixture_symbol("unrelatedSymbol", "function", 5, 7))
        .unwrap();
    let _ = other_id;
    let no_path = query::graph::shortest_path(
        &db,
        "existingSymbol",
        "unrelatedSymbol",
        None,
        None,
        None,
        None,
    )
    .unwrap();
    assert!(!no_path.found);
    assert!(
        no_path.reason.is_none(),
        "two symbols that both resolve but have no path between them is a legitimate \
         answer, not a resolution failure — `reason` must stay unset"
    );
}

// Phase 21/14 regression fixture: `resolver::resolve_path` (backing
// `read_file_skeleton`/`read_file_snippet`) must resolve a Windows-style
// backslash path to the same indexed file a forward-slash query would,
// end-to-end through the full resolver entity (Milestone 1 only tested the
// underlying normalize helper directly).
#[test]
fn resolve_path_handles_windows_backslashes_end_to_end() {
    let db = Database::new(":memory:").unwrap();
    db.insert_file("apps/web/modules/auth/handler.ts", "h")
        .unwrap();

    let resolved = resolver::resolve_path(&db, "apps\\web\\modules\\auth\\handler.ts");
    match resolved {
        resolver::ResolvedEntity::File(f) => {
            assert!(f.file_path.ends_with("apps/web/modules/auth/handler.ts"));
        }
        other => panic!(
            "expected a backslash path to resolve to the indexed file, got {:?}",
            other
        ),
    }
}

// Phase 21/14 regression fixture: Next.js-style route groups `(marketing)`
// and dynamic segments `[slug]` are literal path characters, not glob/regex
// metacharacters — `path_scope` matching (architectural_hotspots and
// friends) must treat them as plain substrings like any other path segment.
#[test]
fn path_scope_matches_nextjs_route_groups_and_dynamic_segments() {
    let db = Database::new(":memory:").unwrap();
    let target_file = db
        .insert_file("app/(marketing)/blog/[slug]/page.tsx", "h")
        .unwrap();
    let target_id = db
        .insert_symbol(target_file, &fixture_symbol("BlogPostPage", "function", 1, 5))
        .unwrap();
    let caller_file = db.insert_file("app/(marketing)/nav.tsx", "h").unwrap();
    let caller_id = db
        .insert_symbol(caller_file, &fixture_symbol("Nav", "function", 1, 3))
        .unwrap();
    db.insert_edge_attributed(caller_file, Some(caller_id), target_id, "calls")
        .unwrap();

    let scope = resolver::CanonicalNameResolver::normalize_path(&db, "app/(marketing)");
    let hotspots = query::graph::architectural_hotspots(&db, 10, Some(scope.as_str())).unwrap();
    assert!(
        !hotspots.top_hotspots.is_empty(),
        "path_scope must match the literal '(marketing)'/'[slug]' segments as plain substrings"
    );
}
