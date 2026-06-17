//  base DDL query to setup the sql database from scratch

pub const INIT_SQL: &str = "
    CREATE TABLE IF NOT EXISTS files (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        path TEXT NOT NULL UNIQUE
    );

    CREATE TABLE IF NOT EXISTS symbols (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        file_id INTEGER NOT NULL,
        name TEXT NOT NULL,
        kind TEXT NOT NULL,
        line_number INTEGER NOT NULL,
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
        line_number INTEGER NOT NULL,
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

    -- Layer 4.5: Analytics Events
    CREATE TABLE IF NOT EXISTS analytics_events (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        event_type TEXT NOT NULL,
        agent_name TEXT,
        session_id TEXT,
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP
    );

    -- Layer 4.5: Token and Cost Accounting
    CREATE TABLE IF NOT EXISTS token_metrics (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        symbol_name TEXT NOT NULL,
        raw_tokens_avoided INTEGER NOT NULL,
        context_tokens_used INTEGER NOT NULL,
        cost_saved_cents REAL NOT NULL,
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP
    );

    -- Layer 4.5: Cache Observability
    CREATE TABLE IF NOT EXISTS cache_metrics (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        symbol_name TEXT NOT NULL,
        status TEXT NOT NULL, -- 'hit' or 'miss'
        latency_ms INTEGER NOT NULL,
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP
    );




    
";