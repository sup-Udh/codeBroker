use graph::{RelationshipNode, SemanticBinding, SemanticBindingKind, SymbolNode};
use rusqlite::params;
use rusqlite::{Connection, Result};

/// Names so generic and widely reused (HTTP route-handler conventions,
/// common factory functions) that a bare-name match against them is more
/// likely a coincidence than a real call/reference. Matching these globally
/// fabricated phantom edges (e.g. every `query.delete()` linking to an
/// exported `DELETE` route, or every import of *some* `createClient` linking
/// to one unrelated local helper).
pub const GENERIC_SYMBOL_NAMES: &[&str] = &[
    "GET",
    "POST",
    "PUT",
    "DELETE",
    "PATCH",
    "HEAD",
    "OPTIONS",
    "createClient",
    "createContext",
];

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
            Component::ParentDir => match components.last() {
                Some(Component::Normal(_)) => {
                    components.pop();
                }
                _ => {
                    components.push(component);
                }
            },
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

/// Serializes an embedding vector to a flat little-endian f32 BLOB for
/// storage in `symbol_embeddings.embedding`. Chosen over a JSON array of
/// floats to keep the table compact and avoid repeated float-formatting
/// precision loss on every read of a value that's never edited by hand.
pub fn embedding_to_blob(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vector.len() * 4);
    for v in vector {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes
}

/// Inverse of `embedding_to_blob`. Silently drops a trailing partial float
/// (a corrupt/truncated row) rather than erroring — callers treat a missing
/// or malformed embedding as "not embedded yet," not a hard failure.
pub fn blob_to_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Cosine similarity between two equal-length embedding vectors. Returns 0.0
/// for a length mismatch or a zero-magnitude vector rather than panicking or
/// returning NaN — both are "this comparison is meaningless," not an error
/// worth propagating through every caller.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let mag_b = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if mag_a == 0.0 || mag_b == 0.0 {
        return 0.0;
    }
    dot / (mag_a * mag_b)
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
        // Negative cache_size is "KiB", not pages — 64MB page cache instead of
        // SQLite's default ~2MB, so a full-repo index's random-ish symbol/edge
        // lookups don't keep falling back to disk. mmap_size lets SQLite read
        // pages straight from the page cache via mmap instead of read(2) for
        // files up to 256MB, which covers every repo this indexer targets.
        conn.pragma_update(None, "cache_size", -64000)?;
        conn.pragma_update(None, "mmap_size", 256 * 1024 * 1024i64)?;

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

        let db = Database { conn, project_root };
        // Every caller of `new` used to be responsible for calling
        // `init_schema` itself, and most forgot to — a database created
        // before a later migration (e.g. `edges.source_symbol_id`) would
        // then surface a raw "no such column" SQLite error from whichever
        // query happened to need it first. `init_schema` is pure
        // `CREATE TABLE IF NOT EXISTS` + best-effort `ALTER TABLE ADD
        // COLUMN`, so running it unconditionally on every open is safe and
        // keeps any pre-existing database current.
        db.init_schema()?;
        Ok(db)
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
        let _ = self.conn.execute(
            "ALTER TABLE symbols RENAME COLUMN line_number TO start_line;",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE symbols ADD COLUMN end_line INTEGER NOT NULL DEFAULT 0;",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE symbols ADD COLUMN start_byte INTEGER NOT NULL DEFAULT 0;",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE symbols ADD COLUMN end_byte INTEGER NOT NULL DEFAULT 0;",
            [],
        );
        let _ = self
            .conn
            .execute("ALTER TABLE symbols ADD COLUMN attributes TEXT;", []);
        let _ = self
            .conn
            .execute("ALTER TABLE symbols ADD COLUMN metadata TEXT;", []);
        let _ = self
            .conn
            .execute("ALTER TABLE files ADD COLUMN metadata TEXT;", []);
        let _ = self
            .conn
            .execute("ALTER TABLE relationships RENAME TO relationships;", []);
        let _ = self
            .conn
            .execute("ALTER TABLE relationships ADD COLUMN source TEXT;", []);
        let _ = self
            .conn
            .execute("ALTER TABLE relationships ADD COLUMN kind TEXT;", []);
        let _ = self
            .conn
            .execute("ALTER TABLE relationships ADD COLUMN state TEXT;", []);
        let _ = self
            .conn
            .execute("ALTER TABLE relationships ADD COLUMN confidence REAL DEFAULT 1.0;", []);
        let _ = self
            .conn
            .execute("ALTER TABLE relationships ADD COLUMN evidence TEXT;", []);
        let _ = self
            .conn
            .execute("ALTER TABLE symbols ADD COLUMN signature TEXT;", []);
        let _ = self
            .conn
            .execute("ALTER TABLE files ADD COLUMN content_hash TEXT;", []);
        // Which symbol an edge originates FROM. Edges are otherwise recorded at
        // file granularity (source_file_id only); for cycle detection that's
        // ambiguous — every symbol in a file looked like the source of every
        // edge leaving it, manufacturing same-file "cycles". Nullable: top-level
        // imports and non-call edges have no enclosing symbol, and pre-existing
        // databases carry NULL here until reindexed.
        let _ = self
            .conn
            .execute("ALTER TABLE edges ADD COLUMN source_symbol_id INTEGER;", []);
        // Distinguishes AST-derived edges ("static") from edges inferred by
        // matching a runtime boundary pattern, e.g. a `fetch("/api/...")`
        // literal resolved to the route handler that answers it ("logical").
        // Defaulted to 'static' so every pre-existing row (all of which came
        // from syntax-walking) is correctly classified with no backfill pass.
        let _ = self.conn.execute(
            "ALTER TABLE edges ADD COLUMN edge_type TEXT NOT NULL DEFAULT 'static';",
            [],
        );
        let _ = self
            .conn
            .execute("ALTER TABLE edges ADD COLUMN metadata TEXT;", []);
        let _ = self.conn.execute(
            "ALTER TABLE edges ADD COLUMN confidence REAL DEFAULT 1.0;",
            [],
        );

        Ok(())
    }

    /// Counts indexed files whose on-disk content no longer matches the hash
    /// recorded at index time (i.e. edited outside CodeBroker since the last
    /// `reindex_workspace`). Capped at `limit` files so this stays cheap on a
    /// large repo — `total_checked < limit` tells the caller the scan covered
    /// every file; `stale_count` may then undercount on a workspace with more
    /// files than `limit` that aren't otherwise touched by name.
    pub fn count_stale_files(&self, limit: usize) -> Result<(usize, usize)> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, content_hash FROM files LIMIT ?1")?;
        let mut rows = stmt.query(params![limit as i64])?;
        let mut total_checked = 0usize;
        let mut stale_count = 0usize;
        while let Some(row) = rows.next()? {
            let path: String = row.get(0)?;
            let indexed_hash: Option<String> = row.get(1)?;
            total_checked += 1;
            let Some(indexed_hash) = indexed_hash else {
                continue;
            };
            let abs_path = self.resolve_path(&path);
            match std::fs::read(&abs_path) {
                Ok(content) => {
                    if hash_content(&content) != indexed_hash {
                        stale_count += 1;
                    }
                }
                // A file that's gone missing since indexing is just as stale
                // as one with edited content.
                Err(_) => stale_count += 1,
            }
        }
        Ok((stale_count, total_checked))
    }

    /// Inserts a file and returns its new SQLite ID. `content_hash` (from
    /// `hash_content`) must be computed from the exact bytes the caller is
    /// about to extract `start_byte`/`end_byte` offsets from, so later reads
    /// can detect drift between the index and the file on disk.
    pub fn insert_file(&self, path: &str, content_hash: &str) -> Result<i64> {
        self.conn
            .prepare_cached("INSERT INTO files (path, content_hash) VALUES (?1, ?2)")?
            .execute(params![path, content_hash])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_file_metadata(&self, file_id: i64, metadata: Option<&str>) -> Result<()> {
        self.conn
            .prepare_cached("UPDATE files SET metadata = ?1 WHERE id = ?2")?
            .execute(params![metadata, file_id])?;
        Ok(())
    }

    /// Inserts a symbol attached to a specific file
    pub fn insert_symbol(&self, file_id: i64, symbol: &SymbolNode) -> Result<i64> {
        let attributes_json = if symbol.attributes.is_empty() {
            None
        } else {
            serde_json::to_string(&symbol.attributes).ok()
        };
        self.conn
            .prepare_cached(
                "INSERT INTO symbols (file_id, name, kind, start_line, end_line, start_byte, end_byte, signature, attributes, metadata) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?
            .execute(params![file_id, symbol.name, symbol.kind, symbol.start_line as i64, symbol.end_line as i64, symbol.start_byte as i64, symbol.end_byte as i64, symbol.signature, attributes_json, symbol.metadata])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn insert_entrypoint(&self, symbol_id: i64, reason: &str) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO entrypoints (symbol_id, reason) VALUES (?1, ?2)",
            rusqlite::params![symbol_id, reason],
        )?;
        Ok(())
    }

    /// Stores (or replaces) a symbol's embedding vector. `REPLACE` rather
    /// than `INSERT` since a symbol whose body changed gets re-embedded with
    /// the same symbol_id when an incremental reindex only touched its
    /// signature/content, not its declaration identity.
    pub fn upsert_symbol_embedding(
        &self,
        symbol_id: i64,
        embedding: &[f32],
        model: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO symbol_embeddings (symbol_id, embedding, model) VALUES (?1, ?2, ?3)",
            params![symbol_id, embedding_to_blob(embedding), model],
        )?;
        Ok(())
    }

    /// All stored embeddings, joined back to their symbol's name/kind/file
    /// for direct use as search results. A linear scan over this is fine at
    /// the symbol counts CodeBroker indexes (hundreds to low thousands) —
    /// not worth standing up a real vector index for.
    pub fn get_all_symbol_embeddings(&self) -> Result<Vec<(i64, String, String, String, Vec<u8>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT symbols.id, symbols.name, symbols.kind, files.path, symbol_embeddings.embedding
             FROM symbol_embeddings
             JOIN symbols ON symbol_embeddings.symbol_id = symbols.id
             JOIN files ON symbols.file_id = files.id",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Vec<u8>>(4)?,
            ));
        }
        Ok(out)
    }

    /// Symbols that have no row in `symbol_embeddings` yet — either never
    /// embedded (no API key was set at index time) or re-inserted with a new
    /// id by an incremental reindex (whose old embedding row was deleted by
    /// `delete_file_data`). Lets a caller backfill embeddings incrementally
    /// instead of re-embedding the whole repo on every reindex.
    pub fn symbols_missing_embeddings(
        &self,
    ) -> Result<Vec<(i64, String, String, String, Option<String>, Option<String>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT symbols.id, symbols.name, symbols.kind, files.path, symbols.signature, semantic_summaries.summary
             FROM symbols
             JOIN files ON symbols.file_id = files.id
             LEFT JOIN semantic_summaries ON semantic_summaries.symbol_id = symbols.id
             LEFT JOIN symbol_embeddings ON symbol_embeddings.symbol_id = symbols.id
             WHERE symbol_embeddings.symbol_id IS NULL",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
            ));
        }
        Ok(out)
    }

    /// Inserts a generic relationship (import, call, reference)
    pub fn insert_relationship(&self, file_id: i64, rel: &RelationshipNode) -> Result<i64> {
        self.conn
            .prepare_cached(
                "INSERT INTO relationships (file_id, name, source, line_number, kind) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?
            .execute(params![file_id, rel.name, rel.source, rel.line_number as i64, rel.kind])?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Insert a semantic binding (type annotation, field type, return type, alias).
    pub fn insert_semantic_binding(&self, file_id: i64, binding: &SemanticBinding) -> Result<i64> {
        self.conn
            .prepare_cached(
                "INSERT INTO semantic_bindings (file_id, kind, name, type_name, context) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?
            .execute(params![file_id, binding.kind.as_str(), binding.name, binding.type_name, binding.context])?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Returns all semantic bindings grouped by file_id.
    /// Tuple: (file_id, SemanticBinding)
    pub fn get_all_semantic_bindings(&self) -> Result<Vec<(i64, SemanticBinding)>> {
        let mut stmt = self.conn.prepare(
            "SELECT file_id, kind, name, type_name, context FROM semantic_bindings ORDER BY file_id",
        )?;
        let iter = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;
        let mut results = Vec::new();
        for item in iter {
            let (file_id, kind_str, name, type_name, context) = item?;
            if let Some(kind) = SemanticBindingKind::from_str(&kind_str) {
                results.push((file_id, SemanticBinding { kind, name, type_name, context }));
            }
        }
        Ok(results)
    }

    /// Pass 2 Helper: Gets all staged relationships that need to be resolved
    pub fn get_all_relationships(
        &self,
    ) -> Result<Vec<(i64, i64, String, Option<String>, Option<String>)>> {
        // Returns a tuple of (relationship_id, file_id, name, source, kind)
        let mut stmt = self
            .conn
            .prepare("SELECT id, file_id, name, source, kind FROM relationships")?;

        let rel_iter = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;

        let mut results = Vec::new();
        for rel in rel_iter {
            results.push(rel?);
        }
        Ok(results)
    }

    /// Like `get_all_relationships`, but also returns each relationship's
    /// `line_number`. The linker uses the line to resolve which symbol a call
    /// originates from (`enclosing_symbol_id`) and record it as the edge's
    /// `source_symbol_id`, so cycle detection has a real symbol-level graph.
    /// Tuple: (relationship_id, file_id, name, source, kind, line_number).
    pub fn get_all_relationships_with_lines(
        &self,
    ) -> Result<Vec<(i64, i64, String, Option<String>, Option<String>, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, file_id, name, source, kind, line_number FROM relationships ORDER BY id",
        )?;
        let rel_iter = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;
        let mut results = Vec::new();
        for rel in rel_iter {
            results.push(rel?);
        }
        Ok(results)
    }

    /// Updates the resolved state of a relationship
    pub fn update_relationship_state(
        &self,
        rel_id: i64,
        state: &str,
        confidence: f64,
        evidence: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE relationships SET state = ?1, confidence = ?2, evidence = ?3 WHERE id = ?4",
            params![state, confidence, evidence, rel_id],
        )?;
        Ok(())
    }

    /// Returns the id of the innermost symbol in `file_id` whose line range
    /// contains `line` — i.e. the symbol a call/reference on that line belongs
    /// to. Returns None for lines outside any symbol (e.g. top-level imports).
    /// "Innermost" (smallest range) handles nested definitions correctly.
    pub fn enclosing_symbol_id(&self, file_id: i64, line: i64) -> Result<Option<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM symbols
             WHERE file_id = ?1 AND start_line <= ?2 AND end_line >= ?2
             AND kind NOT IN ('variable', 'constant', 'parameter', 'local', 'property', 'field', 'import')
             ORDER BY (end_line - start_line) ASC
             LIMIT 1",
        )?;
        let result = stmt.query_row(params![file_id, line], |row| row.get(0));
        match result {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Incremental reindex helper: finds the row id of an already-indexed file
    /// by its stored (relative) path, so a re-parse can clear its stale data
    /// first instead of accumulating duplicate symbols/edges.
    pub fn get_file_id_by_path(&self, path: &str) -> Result<Option<i64>> {
        let result = self.conn.query_row(
            "SELECT id FROM files WHERE path = ?1",
            params![path],
            |row| row.get(0),
        );
        match result {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Incremental reindex helper: clears all symbols, edges, relationships, and
    /// embeddings tied to a file before re-parsing it. Must run before
    /// re-inserting fresh rows for the same file — `init_schema` never enables
    /// `PRAGMA foreign_keys`, so the `ON DELETE CASCADE` declared in the
    /// schema is inert and stale rows would otherwise dangle or double up on
    /// re-link. The `symbol_embeddings` delete matters for the same reason:
    /// symbols are about to be deleted and reinserted with new ids, so their
    /// old embedding rows (keyed by the old id) would otherwise become
    /// permanently orphaned, silently bloating the table without ever being
    /// queryable again.
    pub fn delete_file_data(&self, file_id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM symbol_embeddings WHERE symbol_id IN (SELECT id FROM symbols WHERE file_id = ?1)", params![file_id])?;
        self.conn.execute("DELETE FROM edges WHERE target_symbol_id IN (SELECT id FROM symbols WHERE file_id = ?1)", params![file_id])?;
        self.conn.execute(
            "DELETE FROM edges WHERE source_file_id = ?1",
            params![file_id],
        )?;
        self.conn.execute(
            "DELETE FROM relationships WHERE file_id = ?1",
            params![file_id],
        )?;
        self.conn
            .execute("DELETE FROM symbols WHERE file_id = ?1", params![file_id])?;
        self.conn
            .execute("DELETE FROM files WHERE id = ?1", params![file_id])?;
        Ok(())
    }

    /// Pass 2 Helper: Tries to find a physical symbol matching the import name
    pub fn find_symbol_id_by_name(&self, name: &str) -> Result<Option<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM symbols WHERE LOWER(name) = LOWER(?1) ORDER BY id LIMIT 1")?;

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
        let mut stmt = self
            .conn
            .prepare("SELECT id, file_id FROM symbols WHERE name = ?1 ORDER BY id LIMIT 1")?;
        let result = stmt.query_row(params![name], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        });
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
        let mut stmt = self.conn.prepare(
            "SELECT id FROM symbols WHERE file_id = ?1 AND name = ?2 ORDER BY id LIMIT 1",
        )?;
        let result = stmt.query_row(params![file_id, name], |row| row.get(0));
        match result {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Creates a directional relationship between two symbols
    pub fn insert_edge(
        &self,
        source_file_id: i64,
        target_symbol_id: i64,
        kind: &str,
    ) -> Result<()> {
        self.insert_edge_attributed(source_file_id, None, target_symbol_id, kind)
    }

    /// Inserts an edge, optionally attributed to the specific source symbol it
    /// originates from. `source_symbol_id` should be the enclosing symbol of
    /// the call/reference (see `enclosing_symbol_id`); pass None for edges with
    /// no enclosing symbol (top-level imports) or that aren't symbol-scoped.
    /// dependency_cycles relies on this attribution to build an accurate
    /// symbol-level graph instead of a file-level cartesian product.
    pub fn insert_edge_attributed(
        &self,
        source_file_id: i64,
        source_symbol_id: Option<i64>,
        target_symbol_id: i64,
        kind: &str,
    ) -> Result<()> {
        self.conn
            .prepare_cached(
                "INSERT OR IGNORE INTO edges (source_file_id, source_symbol_id, target_symbol_id, kind) VALUES (?1, ?2, ?3, ?4)",
            )?
            .execute(params![source_file_id, source_symbol_id, target_symbol_id, kind])?;
        Ok(())
    }

    /// Inserts a logical edge (`graph::EdgeType::Logical`) — one inferred by
    /// matching a runtime-boundary pattern (HTTP fetch, websocket emit) to
    /// the symbol that handles it, rather than found by walking syntax. Same
    /// dedup contract as `insert_edge_attributed`: a (source_symbol_id,
    /// target_symbol_id, kind) triple is only ever stored once.
    pub fn insert_interaction_edge(
        &self,
        source_file_id: i64,
        source_symbol_id: Option<i64>,
        target_symbol_id: i64,
        metadata: &str,
        confidence: f64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO edges (source_file_id, source_symbol_id, target_symbol_id, kind, edge_type, metadata, confidence) VALUES (?1, ?2, ?3, 'interaction', 'logical', ?4, ?5)",
            params![source_file_id, source_symbol_id, target_symbol_id, metadata, confidence],
        )?;
        Ok(())
    }

    /// Layer 3: Save an AI-generated summary to the Knowledge Store
    pub fn save_pipeline_manifest(
        &self,
        version: &str,
        parser_version: &str,
        semantic_version: &str,
        resolver_version: &str,
        graph_version: &str,
        embedding_version: &str,
        total_files: i64,
        total_symbols: i64,
        total_relationships: i64,
        edges_emitted: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO pipeline_manifests (version, parser_version, semantic_version, resolver_version, graph_version, embedding_version, total_files, total_symbols, total_relationships, edges_emitted) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![version, parser_version, semantic_version, resolver_version, graph_version, embedding_version, total_files, total_symbols, total_relationships, edges_emitted],
        )?;
        Ok(())
    }

    /// Layer 3: Save an AI-generated summary to the Knowledge Store with metadata
    pub fn save_semantic_summary(
        &self,
        symbol_id: i64,
        summary: &str,
        source_hash: &str,
        context_hash: &str,
        model_name: &str,
        token_count: usize,
        generation_time_ms: u128,
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
        model_name: &str,
    ) -> Result<Option<String>> {
        // 1. Fetch the summary and its primary ID
        let mut stmt = self.conn.prepare(
            "SELECT id, summary FROM semantic_summaries 
             WHERE symbol_id = ?1 AND source_hash = ?2 AND context_hash = ?3 AND model_name = ?4 
             ORDER BY created_at DESC LIMIT 1",
        )?;

        let result = stmt.query_row(
            params![symbol_id, source_hash, context_hash, model_name],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        );

        match result {
            Ok((id, summary)) => {
                // 2. Increment the hit_count!
                let _ = self.conn.execute(
                    "UPDATE semantic_summaries SET hit_count = hit_count + 1 WHERE id = ?1",
                    params![id],
                );
                Ok(Some(summary))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Phase 2: Aggregates metrics for the Knowledge Dashboard
    pub fn get_codebroker_stats(&self) -> Result<CodeBrokerStats> {
        let files_indexed: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
            .unwrap_or(0);
        let summaries_generated: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM semantic_summaries", [], |row| {
                row.get(0)
            })
            .unwrap_or(0);

        // Sum up all the hits
        let total_cache_hits: i64 = self
            .conn
            .query_row("SELECT SUM(hit_count) FROM semantic_summaries", [], |row| {
                row.get(0)
            })
            .unwrap_or(0);

        // Grab all file paths so we can calculate languages
        let mut extensions = std::collections::HashMap::new();
        if let Ok(mut stmt) = self.conn.prepare("SELECT path FROM files") {
            if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) {
                for path in rows.flatten() {
                    if let Some(ext) = std::path::Path::new(&path)
                        .extension()
                        .and_then(|e| e.to_str())
                    {
                        *extensions.entry(ext.to_string()).or_insert(0) += 1;
                    }
                }
            }
        }

        Ok(CodeBrokerStats {
            files_indexed,
            summaries_generated,
            total_cache_hits,
            extensions,
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
        let files: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
            .unwrap_or(0);
        let symbols: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))
            .unwrap_or(0);
        let edges: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM edges", [], |row| row.get(0))
            .unwrap_or(0);

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
             ORDER BY files.path, symbols.name",
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
        format!(
            "{}-{}-{}-{}-{}",
            files, symbols, edges, top_modules, symbol_shapes
        )
        .hash(&mut hasher);
        Ok(format!("{:x}", hasher.finish()))
    }

    pub fn save_repository_overview(
        &self,
        repository_hash: &str,
        model_name: &str,
        overview_text: &str,
    ) -> Result<()> {
        let max_version: i64 = self
            .conn
            .query_row(
                "SELECT MAX(topology_version) FROM repository_overviews",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        self.conn.execute(
            "INSERT INTO repository_overviews (repository_hash, topology_version, model_name, overview_text) VALUES (?1, ?2, ?3, ?4)",
            params![repository_hash, max_version + 1, model_name, overview_text]
        )?;
        Ok(())
    }

    pub fn get_cached_repository_overview(
        &self,
        repository_hash: &str,
        model_name: &str,
    ) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT overview_text FROM repository_overviews WHERE repository_hash = ?1 AND model_name = ?2 ORDER BY created_at DESC LIMIT 1"
        )?;
        let result = stmt.query_row(params![repository_hash, model_name], |row| {
            row.get::<_, String>(0)
        });
        match result {
            Ok(summary) => Ok(Some(summary)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
