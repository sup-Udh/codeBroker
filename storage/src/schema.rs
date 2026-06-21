//  base DDL query to setup the sql database from scratch

pub const INIT_SQL: &str = "
    CREATE TABLE IF NOT EXISTS files (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        path TEXT NOT NULL UNIQUE,
        directive TEXT,
        route_path TEXT,
        route_segment TEXT
    );

    CREATE TABLE IF NOT EXISTS symbols (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        file_id INTEGER NOT NULL,
        name TEXT NOT NULL,
        kind TEXT NOT NULL,
        prop_type TEXT,
        start_line INTEGER NOT NULL,
        end_line INTEGER NOT NULL,
        start_byte INTEGER NOT NULL DEFAULT 0,
        end_byte INTEGER NOT NULL DEFAULT 0,
        signature TEXT,
        FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS edges (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        source_file_id INTEGER NOT NULL,
        target_symbol_id INTEGER NOT NULL,
        kind TEXT NOT NULL,
        FOREIGN KEY(source_file_id) REFERENCES files(id) ON DELETE CASCADE,
        FOREIGN KEY(target_symbol_id) REFERENCES symbols(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS raw_imports (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        file_id INTEGER NOT NULL,
        name TEXT NOT NULL,
        source TEXT,
        line_number INTEGER NOT NULL,
        kind TEXT,
        FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
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

    CREATE TABLE IF NOT EXISTS subsystem_overviews (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        subsystem_name TEXT NOT NULL,
        subsystem_hash TEXT NOT NULL,
        model_name TEXT NOT NULL,
        overview_text TEXT NOT NULL,
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP
    );

    -- Layer 4.5: Unified Analytics Events
    CREATE TABLE IF NOT EXISTS mcp_analytics_events (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        tool_name TEXT NOT NULL,
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
    
";