use rusqlite::{Connection, Result};
use rusqlite::params;
use graph::{SymbolNode, ImportNode};

use crate::schema::INIT_SQL;

/// Lexically normalizes a path, collapsing "." and ".." components without
/// touching the filesystem (the path may not exist yet). This ensures two
/// different textual spellings of the same path (e.g. an absolute path vs.
/// a "./relative" path joined onto the project root) compare equal.
pub fn normalize_path(path: &std::path::Path) -> String {
    use std::path::Component;
    let mut components: Vec<Component> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                match components.last() {
                    Some(Component::Normal(_)) => { components.pop(); }
                    _ => { components.push(component); }
                }
            }
            other => components.push(other),
        }
    }
    let mut normalized = std::path::PathBuf::new();
    for component in components {
        normalized.push(component.as_os_str());
    }
    normalized.to_string_lossy().to_string()
}

pub struct Database {
    pub conn: Connection,
    pub project_root: String,
}

/// Hashes raw file content as it was indexed, so retrieval can detect that a
/// file changed on disk since `start_byte`/`end_byte` were computed. Byte
/// offsets are only valid against the exact content they were derived from —
/// any edit (even appending a single character earlier in the file) shifts
/// every later offset, and slicing the new content with old offsets produces
/// a plausible-looking but semantically wrong substring instead of an error.
pub fn hash_content(content: &[u8]) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

pub struct CodeBrokerStats {
    pub files_indexed: i64,
    pub summaries_generated: i64,
    pub total_cache_hits: i64,
    pub extensions: std::collections::HashMap<String, i64>,
}

impl Database {
    /// Opens a connection to the SQLite database at the given path
    pub fn new(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        
        // Canonicalize the db_path to absolute, then get parent (.codebroker), then get its parent (project root)
        let abs_db_path = std::path::Path::new(db_path)
            .canonicalize()
            .unwrap_or_else(|_| std::path::PathBuf::from(db_path));
            
        let project_root = abs_db_path
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_string_lossy()
            .to_string();

        Ok(Database { conn, project_root })
    }

    /// Resolves a stored relative path into an absolute path based on the project root
    pub fn resolve_path(&self, relative_path: &str) -> String {
        let root = std::path::Path::new(&self.project_root);
        let rel = std::path::Path::new(relative_path);
        normalize_path(&root.join(rel))
    }

    /// Creates the tables if they don't already exist
    pub fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(INIT_SQL)?;
        
        // Safely migrate old schemas
        let _ = self.conn.execute("ALTER TABLE symbols RENAME COLUMN line_number TO start_line;", []);
        let _ = self.conn.execute("ALTER TABLE symbols ADD COLUMN end_line INTEGER NOT NULL DEFAULT 0;", []);
        let _ = self.conn.execute("ALTER TABLE symbols ADD COLUMN start_byte INTEGER NOT NULL DEFAULT 0;", []);
        let _ = self.conn.execute("ALTER TABLE symbols ADD COLUMN end_byte INTEGER NOT NULL DEFAULT 0;", []);
        let _ = self.conn.execute("ALTER TABLE symbols ADD COLUMN prop_type TEXT;", []);
        let _ = self.conn.execute("ALTER TABLE files ADD COLUMN directive TEXT;", []);
        let _ = self.conn.execute("ALTER TABLE files ADD COLUMN route_path TEXT;", []);
        let _ = self.conn.execute("ALTER TABLE files ADD COLUMN route_segment TEXT;", []);
        let _ = self.conn.execute("ALTER TABLE raw_imports ADD COLUMN source TEXT;", []);
        let _ = self.conn.execute("ALTER TABLE raw_imports ADD COLUMN kind TEXT;", []);
        let _ = self.conn.execute("ALTER TABLE symbols ADD COLUMN signature TEXT;", []);
        let _ = self.conn.execute("ALTER TABLE files ADD COLUMN content_hash TEXT;", []);

        Ok(())
    }

    /// Inserts a file and returns its new SQLite ID. `content_hash` (from
    /// `hash_content`) must be computed from the exact bytes the caller is
    /// about to extract `start_byte`/`end_byte` offsets from, so later reads
    /// can detect drift between the index and the file on disk.
    pub fn insert_file(&self, path: &str, content_hash: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO files (path, content_hash) VALUES (?1, ?2)",
            params![path, content_hash],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_file_metadata(&self, file_id: i64, directive: Option<&str>, route_path: Option<&str>, route_segment: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE files SET directive = ?1, route_path = ?2, route_segment = ?3 WHERE id = ?4",
            params![directive, route_path, route_segment, file_id],
        )?;
        Ok(())
    }

    /// Inserts a symbol attached to a specific file
    pub fn insert_symbol(&self, file_id: i64, symbol: &SymbolNode) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO symbols (file_id, name, kind, prop_type, start_line, end_line, start_byte, end_byte, signature) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![file_id, symbol.name, symbol.kind, symbol.prop_type, symbol.start_line as i64, symbol.end_line as i64, symbol.start_byte as i64, symbol.end_byte as i64, symbol.signature],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Inserts an import as a special kind of symbol
     pub fn insert_raw_import(&self, file_id: i64, import: &ImportNode) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO raw_imports (file_id, name, source, line_number, kind) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![file_id, import.name, import.source, import.line_number as i64, import.kind],
        )?;
        Ok(self.conn.last_insert_rowid())
    
    }

    // new methods: 

        /// Pass 2 Helper: Gets all staged imports that need to be resolved
    pub fn get_all_raw_imports(&self) -> Result<Vec<(i64, i64, String, Option<String>, Option<String>)>> {
        // Returns a tuple of (raw_import_id, file_id, import_name, source, kind)
        let mut stmt = self.conn.prepare("SELECT id, file_id, name, source, kind FROM raw_imports")?;
        
        let import_iter = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;

        let mut results = Vec::new();
        for import in import_iter {
            results.push(import?);
        }
        Ok(results)
    }

    /// Incremental reindex helper: finds the row id of an already-indexed file
    /// by its stored (relative) path, so a re-parse can clear its stale data
    /// first instead of accumulating duplicate symbols/edges.
    pub fn get_file_id_by_path(&self, path: &str) -> Result<Option<i64>> {
        let result = self.conn.query_row("SELECT id FROM files WHERE path = ?1", params![path], |row| row.get(0));
        match result {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Incremental reindex helper: clears all symbols, edges, and raw_imports
    /// tied to a file before re-parsing it. Must run before re-inserting fresh
    /// rows for the same file — `init_schema` never enables `PRAGMA
    /// foreign_keys`, so the `ON DELETE CASCADE` declared in the schema is
    /// inert and stale rows would otherwise dangle or double up on re-link.
    pub fn delete_file_data(&self, file_id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM edges WHERE target_symbol_id IN (SELECT id FROM symbols WHERE file_id = ?1)", params![file_id])?;
        self.conn.execute("DELETE FROM edges WHERE source_file_id = ?1", params![file_id])?;
        self.conn.execute("DELETE FROM raw_imports WHERE file_id = ?1", params![file_id])?;
        self.conn.execute("DELETE FROM symbols WHERE file_id = ?1", params![file_id])?;
        self.conn.execute("DELETE FROM files WHERE id = ?1", params![file_id])?;
        Ok(())
    }

    /// Pass 2 Helper: Tries to find a physical symbol matching the import name
    pub fn find_symbol_id_by_name(&self, name: &str) -> Result<Option<i64>> {
        let mut stmt = self.conn.prepare("SELECT id FROM symbols WHERE LOWER(name) = LOWER(?1) LIMIT 1")?;

        // We use query_row because we only expect 0 or 1 result
        let result = stmt.query_row(params![name], |row| row.get(0));

        match result {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Case-SENSITIVE exact-name symbol lookup, used specifically for call-edge
    /// resolution. The case-insensitive `find_symbol_id_by_name` is fine for
    /// import resolution (where casing is usually stable), but for call edges
    /// it manufactured phantom relationships: a member call like
    /// `someObject.get()` would resolve to an unrelated top-level `GET` route
    /// handler purely because the names matched ignoring case. Returns the
    /// symbol's id and its defining file_id (the caller needs the file_id to
    /// skip self-referential edges).
    pub fn find_symbol_exact_with_file(&self, name: &str) -> Result<Option<(i64, i64)>> {
        let mut stmt = self.conn.prepare("SELECT id, file_id FROM symbols WHERE name = ?1 LIMIT 1")?;
        let result = stmt.query_row(params![name], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)));
        match result {
            Ok(pair) => Ok(Some(pair)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Case-sensitive exact-name lookup restricted to a single file — used to
    /// resolve a free call to a same-file helper without risking a cross-file
    /// phantom match.
    pub fn find_symbol_id_in_file_exact(&self, file_id: i64, name: &str) -> Result<Option<i64>> {
        let mut stmt = self.conn.prepare("SELECT id FROM symbols WHERE file_id = ?1 AND name = ?2 LIMIT 1")?;
        let result = stmt.query_row(params![file_id, name], |row| row.get(0));
        match result {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
    


    /// Creates a directional relationship between two symbols
    pub fn insert_edge(&self, source_file_id: i64, target_symbol_id: i64, kind: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO edges (source_file_id, target_symbol_id, kind) VALUES (?1, ?2, ?3)",
            params![source_file_id, target_symbol_id, kind],
        )?;
        Ok(())
    }


        /// Layer 3: Save an AI-generated summary to the Knowledge Store
     /// Layer 3: Save an AI-generated summary to the Knowledge Store with metadata
    pub fn save_semantic_summary(
        &self, 
        symbol_id: i64, 
        summary: &str, 
        source_hash: &str, 
        context_hash: &str,
        model_name: &str,
        token_count: usize,
        generation_time_ms: u128
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO semantic_summaries (symbol_id, summary, source_hash, context_hash, model_name, token_count, generation_time_ms, hit_count) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)",
            params![symbol_id, summary, source_hash, context_hash, model_name, (token_count as i64), (generation_time_ms as i64)],        )?;
        Ok(())
    }

    /// Layer 3: Retrieve a cached summary and increment its hit_count
    pub fn get_cached_summary(
        &self, 
        symbol_id: i64, 
        source_hash: &str, 
        context_hash: &str,
        model_name: &str
    ) -> Result<Option<String>> {
        // 1. Fetch the summary and its primary ID
        let mut stmt = self.conn.prepare(
            "SELECT id, summary FROM semantic_summaries 
             WHERE symbol_id = ?1 AND source_hash = ?2 AND context_hash = ?3 AND model_name = ?4 
             ORDER BY created_at DESC LIMIT 1"
        )?;
        
        let result = stmt.query_row(params![symbol_id, source_hash, context_hash, model_name], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        });

        match result {
            Ok((id, summary)) => {
                // 2. Increment the hit_count!
                let _ = self.conn.execute("UPDATE semantic_summaries SET hit_count = hit_count + 1 WHERE id = ?1", params![id]);
                Ok(Some(summary))
            },
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }



        /// Phase 2: Aggregates metrics for the Knowledge Dashboard
    pub fn get_codebroker_stats(&self) -> Result<CodeBrokerStats> {
        let files_indexed: i64 = self.conn.query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0)).unwrap_or(0);
        let summaries_generated: i64 = self.conn.query_row("SELECT COUNT(*) FROM semantic_summaries", [], |row| row.get(0)).unwrap_or(0);
        
        // Sum up all the hits
        let total_cache_hits: i64 = self.conn.query_row("SELECT SUM(hit_count) FROM semantic_summaries", [], |row| row.get(0)).unwrap_or(0);
        
        // Grab all file paths so we can calculate languages
        let mut extensions = std::collections::HashMap::new();
        if let Ok(mut stmt) = self.conn.prepare("SELECT path FROM files") {
            if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) {
                for path in rows.flatten() {
                    if let Some(ext) = std::path::Path::new(&path).extension().and_then(|e| e.to_str()) {
                        *extensions.entry(ext.to_string()).or_insert(0) += 1;
                    }
                }
            }
        }

        Ok(CodeBrokerStats {
            files_indexed,
            summaries_generated,
            total_cache_hits,
            extensions
        })
    }

    /// Cache key for `project_overview_ai`'s repository overview. Previously
    /// this only hashed file/symbol/edge COUNTS plus the directory listing —
    /// so renaming a function, changing its signature, or any other edit
    /// that doesn't change those counts left the hash identical, and the
    /// cached (now stale) narrative kept being served indefinitely. Folding
    /// in every symbol's (path, name, kind, signature) makes the hash change
    /// whenever any symbol's shape actually changes, while still avoiding a
    /// full re-read of file contents on every cache check.
    pub fn get_repository_topology_hash(&self) -> Result<String> {
        let files: i64 = self.conn.query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0)).unwrap_or(0);
        let symbols: i64 = self.conn.query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0)).unwrap_or(0);
        let edges: i64 = self.conn.query_row("SELECT COUNT(*) FROM edges", [], |row| row.get(0)).unwrap_or(0);

        let mut stmt = self.conn.prepare("SELECT path FROM files")?;
        let mut rows = stmt.query([])?;
        let mut dirs = std::collections::HashSet::new();
        while let Some(row) = rows.next()? {
            let path: String = row.get(0)?;
            if let Some(parent) = std::path::Path::new(&path).parent() {
                dirs.insert(parent.to_string_lossy().to_string());
            }
        }
        let mut dirs_vec: Vec<_> = dirs.into_iter().collect();
        dirs_vec.sort();
        let top_modules = dirs_vec.join(",");

        let mut sym_stmt = self.conn.prepare(
            "SELECT files.path, symbols.name, symbols.kind, symbols.signature
             FROM symbols JOIN files ON symbols.file_id = files.id
             ORDER BY files.path, symbols.name"
        )?;
        let mut sym_rows = sym_stmt.query([])?;
        let mut symbol_shapes = String::new();
        while let Some(row) = sym_rows.next()? {
            let path: String = row.get(0)?;
            let name: String = row.get(1)?;
            let kind: String = row.get(2)?;
            let signature: Option<String> = row.get(3)?;
            symbol_shapes.push_str(&path);
            symbol_shapes.push('|');
            symbol_shapes.push_str(&name);
            symbol_shapes.push('|');
            symbol_shapes.push_str(&kind);
            symbol_shapes.push('|');
            symbol_shapes.push_str(signature.as_deref().unwrap_or(""));
            symbol_shapes.push(';');
        }

        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        format!("{}-{}-{}-{}-{}", files, symbols, edges, top_modules, symbol_shapes).hash(&mut hasher);
        Ok(format!("{:x}", hasher.finish()))
    }

    pub fn save_repository_overview(&self, repository_hash: &str, model_name: &str, overview_text: &str) -> Result<()> {
        let max_version: i64 = self.conn.query_row("SELECT MAX(topology_version) FROM repository_overviews", [], |row| row.get(0)).unwrap_or(0);
        self.conn.execute(
            "INSERT INTO repository_overviews (repository_hash, topology_version, model_name, overview_text) VALUES (?1, ?2, ?3, ?4)",
            params![repository_hash, max_version + 1, model_name, overview_text]
        )?;
        Ok(())
    }

    pub fn get_cached_repository_overview(&self, repository_hash: &str, model_name: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT overview_text FROM repository_overviews WHERE repository_hash = ?1 AND model_name = ?2 ORDER BY created_at DESC LIMIT 1"
        )?;
        let result = stmt.query_row(params![repository_hash, model_name], |row| row.get::<_, String>(0));
        match result {
            Ok(summary) => Ok(Some(summary)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}