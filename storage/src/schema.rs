//  base DDL query to setup the sql database from scratch

pub const INIT_SQL: &str = "
    CREATE TABLE IF NOT EXISTS files (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        path TEXT NOT NULL UNIQUE,
        metadata TEXT
    );

    CREATE TABLE IF NOT EXISTS entrypoints (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        symbol_id INTEGER NOT NULL,
        reason TEXT,
        FOREIGN KEY(symbol_id) REFERENCES symbols(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS symbols (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        file_id INTEGER NOT NULL,
        name TEXT NOT NULL,
        kind TEXT NOT NULL,
        start_line INTEGER NOT NULL,
        end_line INTEGER NOT NULL,
        start_byte INTEGER NOT NULL DEFAULT 0,
        end_byte INTEGER NOT NULL DEFAULT 0,
        signature TEXT,
        attributes TEXT,
        metadata TEXT,
        FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS edges (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        source_file_id INTEGER NOT NULL,
        source_symbol_id INTEGER,
        target_symbol_id INTEGER NOT NULL,
        kind TEXT NOT NULL,
        edge_type TEXT NOT NULL DEFAULT 'static',
        metadata TEXT,
        confidence REAL DEFAULT 1.0,
        FOREIGN KEY(source_file_id) REFERENCES files(id) ON DELETE CASCADE,
        FOREIGN KEY(target_symbol_id) REFERENCES symbols(id) ON DELETE CASCADE
    );

    CREATE UNIQUE INDEX IF NOT EXISTS idx_edges_dedup ON edges (
        source_file_id, 
        IFNULL(source_symbol_id, -1), 
        target_symbol_id, 
        kind, 
        edge_type
    );

    CREATE TABLE IF NOT EXISTS relationships (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        file_id INTEGER NOT NULL,
        name TEXT NOT NULL,
        source TEXT,
        line_number INTEGER NOT NULL,
        kind TEXT,
        state TEXT,
        confidence REAL DEFAULT 1.0,
        evidence TEXT,
        FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
    );

    -- Embedding-based semantic search. Keyed by (symbol_id, model_id) so
    -- switching embedding models invalidates old vectors instead of silently
    -- mixing two models' vectors in one similarity comparison. `body_hash`
    -- is a hash of the exact card text that was embedded, so a reindex can
    -- skip re-embedding symbols whose content didn't change (symbol ids are
    -- NOT stable across a full rebuild — the hash is the stable identity).
    -- `vector` is a flat little-endian f32 BLOB (see
    -- storage::embedding_to_blob/blob_to_embedding) rather than a JSON array,
    -- to keep the table compact and avoid float-formatting precision loss.
    CREATE TABLE IF NOT EXISTS embeddings (
        symbol_id  INTEGER NOT NULL,
        model_id   TEXT    NOT NULL,
        dims       INTEGER NOT NULL,
        vector     BLOB    NOT NULL,
        body_hash  TEXT    NOT NULL,
        PRIMARY KEY (symbol_id, model_id),
        FOREIGN KEY(symbol_id) REFERENCES symbols(id) ON DELETE CASCADE
    );

    CREATE INDEX IF NOT EXISTS idx_embeddings_model ON embeddings(model_id);

    CREATE TABLE IF NOT EXISTS symbol_features (
        symbol_id INTEGER PRIMARY KEY,
        pagerank REAL NOT NULL DEFAULT 0.0,
        fan_in INTEGER NOT NULL DEFAULT 0,
        fan_out INTEGER NOT NULL DEFAULT 0,
        interaction_count INTEGER NOT NULL DEFAULT 0,
        community_id INTEGER,
        is_entrypoint BOOLEAN NOT NULL DEFAULT 0,
        is_exported BOOLEAN NOT NULL DEFAULT 0,
        is_public BOOLEAN NOT NULL DEFAULT 0,
        is_callable BOOLEAN NOT NULL DEFAULT 0,
        is_type BOOLEAN NOT NULL DEFAULT 0,
        is_constant BOOLEAN NOT NULL DEFAULT 0,
        is_local BOOLEAN NOT NULL DEFAULT 0,
        is_generated BOOLEAN NOT NULL DEFAULT 0,
        FOREIGN KEY(symbol_id) REFERENCES symbols(id) ON DELETE CASCADE
    );


        CREATE TABLE IF NOT EXISTS semantic_summaries (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        symbol_id INTEGER NOT NULL,
        summary TEXT NOT NULL,
        source_hash TEXT NOT NULL,
        context_hash TEXT NOT NULL,
        model_name TEXT NOT NULL,
        token_count INTEGER NOT NULL DEFAULT 0,
        generation_time_ms INTEGER NOT NULL DEFAULT 0,
        hit_count INTEGER NOT NULL DEFAULT 0,
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY(symbol_id) REFERENCES symbols(id) ON DELETE CASCADE
    );


        CREATE TABLE IF NOT EXISTS repository_overviews (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        repository_hash TEXT NOT NULL,
        topology_version INTEGER NOT NULL DEFAULT 1,
        model_name TEXT NOT NULL,
        overview_text TEXT NOT NULL,
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS pipeline_manifests (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        version TEXT NOT NULL,
        parser_version TEXT NOT NULL,
        semantic_version TEXT NOT NULL,
        resolver_version TEXT NOT NULL,
        graph_version TEXT NOT NULL,
        embedding_version TEXT NOT NULL,
        total_files INTEGER NOT NULL,
        total_symbols INTEGER NOT NULL,
        total_relationships INTEGER NOT NULL,
        edges_emitted INTEGER NOT NULL,
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP
    );

    -- Semantic bindings extracted by static analysis during indexing.
    -- Separate from the relationships table so they don't appear in resolution
    -- metrics. Deleted on incremental reindex via delete_file_data.
    CREATE TABLE IF NOT EXISTS semantic_bindings (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        file_id INTEGER NOT NULL,
        kind TEXT NOT NULL,
        name TEXT NOT NULL,
        type_name TEXT NOT NULL,
        context TEXT,
        FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
    );
    CREATE INDEX IF NOT EXISTS idx_semantic_bindings_file ON semantic_bindings(file_id);

    CREATE TABLE IF NOT EXISTS metadata (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    -- Layer 4.5: Unified Analytics Events
    CREATE TABLE IF NOT EXISTS mcp_analytics_events (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        tool_name TEXT NOT NULL,
        prompt TEXT,
        success BOOLEAN NOT NULL DEFAULT 1,
        execution_time_ms INTEGER NOT NULL,
        delivered_token_count INTEGER NOT NULL,
        estimated_raw_context_tokens INTEGER NOT NULL,
        token_reduction INTEGER NOT NULL,
        cache_hit BOOLEAN NOT NULL,
        model_used TEXT,
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS search_events (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        query TEXT NOT NULL,
        result_count INTEGER NOT NULL,
        latency_ms INTEGER NOT NULL,
        fallback_used BOOLEAN NOT NULL,
        llm_used BOOLEAN NOT NULL,
        top_result TEXT,
        search_mode TEXT,
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP
    );


    CREATE TABLE IF NOT EXISTS metadata (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    -- Domain-concept tags (auth, realtime, notifications, database, ...),
    -- independent of literal symbol-name/path substring matching. A symbol
    -- can carry more than one concept (e.g. a Supabase client factory is
    -- both 'auth' and 'database'). `matched_on` records whether the tag came
    -- from the symbol's own name or its file's path, for transparency in
    -- search results. Without this, a query like \"auth system\" only ever
    -- found symbols whose literal name/path contained \"auth\" — missing
    -- e.g. createClient/createAdminClient/signInWithOAuth entirely
    -- (benchmark run_005's central finding).
    CREATE TABLE IF NOT EXISTS symbol_concepts (
        symbol_id INTEGER NOT NULL,
        concept TEXT NOT NULL,
        matched_on TEXT NOT NULL,
        PRIMARY KEY (symbol_id, concept),
        FOREIGN KEY(symbol_id) REFERENCES symbols(id) ON DELETE CASCADE
    );

    CREATE INDEX IF NOT EXISTS idx_symbol_concepts_concept ON symbol_concepts(concept);

";
