use std::sync::{Arc, Mutex};
use storage::Database;

/// One semantic retrieval hit, before conversion to a search result.
#[derive(Debug, Clone)]
pub struct SemanticHit {
    pub symbol_id: i64,
    pub name: String,
    pub kind: String,
    /// Stored (relative) path, as in the `files` table.
    pub path: String,
    pub start_line: i64,
    pub cosine: f32,
}

/// Top-k similarity retrieval over the embedded symbols of one model.
/// Deliberately small so the brute-force implementation below can be swapped
/// for an ANN index later without touching any caller — but no ANN library
/// is warranted now: at 384 dims, even 100k symbols is ~150MB and a few ms
/// per brute-force query.
pub trait VectorSearch: Send + Sync {
    /// `path_scope` filters on the stored relative path (substring), the
    /// same contract lexical search's `path_scope` uses.
    fn top_k(&self, query: &[f32], k: usize, path_scope: Option<&str>) -> Vec<SemanticHit>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

struct Entry {
    symbol_id: i64,
    name: String,
    kind: String,
    path: String,
    start_line: i64,
}

/// All vectors for one model held in a flat in-memory f32 matrix (row-major,
/// one row per symbol) with a parallel metadata list. Similarity is computed
/// here in Rust — never inside SQL.
pub struct BruteForceVectorSearch {
    dims: usize,
    matrix: Vec<f32>,
    entries: Vec<Entry>,
}

impl BruteForceVectorSearch {
    pub fn load(db: &Database, model_id: &str) -> Result<Self, String> {
        let rows = db
            .get_all_symbol_embeddings(model_id)
            .map_err(|e| e.to_string())?;
        let mut dims = 0usize;
        let mut matrix: Vec<f32> = Vec::new();
        let mut entries: Vec<Entry> = Vec::new();
        for (symbol_id, name, kind, path, start_line, blob) in rows {
            let vector = storage::blob_to_embedding(&blob);
            if vector.is_empty() {
                continue; // corrupt/truncated row: treat as not embedded
            }
            if dims == 0 {
                dims = vector.len();
            }
            if vector.len() != dims {
                continue; // defensive: never mix dimensionalities in one matrix
            }
            matrix.extend_from_slice(&vector);
            entries.push(Entry {
                symbol_id,
                name,
                kind,
                path,
                start_line,
            });
        }
        Ok(BruteForceVectorSearch {
            dims,
            matrix,
            entries,
        })
    }
}

impl VectorSearch for BruteForceVectorSearch {
    fn top_k(&self, query: &[f32], k: usize, path_scope: Option<&str>) -> Vec<SemanticHit> {
        if self.dims == 0 || query.len() != self.dims || k == 0 {
            return Vec::new();
        }
        let mut scored: Vec<(usize, f32)> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| match path_scope {
                // Same matching contract as lexical search's path_scope
                // (slash-normalized substring), so the two legs never
                // disagree about what a scope means.
                Some(scope) => query::path_matches_scope(&e.path, scope),
                None => true,
            })
            .map(|(i, _)| {
                let row = &self.matrix[i * self.dims..(i + 1) * self.dims];
                (i, storage::cosine_similarity(row, query))
            })
            .collect();
        scored.sort_by(|(_, a), (_, b)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);
        scored
            .into_iter()
            .map(|(i, cosine)| {
                let e = &self.entries[i];
                SemanticHit {
                    symbol_id: e.symbol_id,
                    name: e.name.clone(),
                    kind: e.kind.clone(),
                    path: e.path.clone(),
                    start_line: e.start_line,
                    cosine,
                }
            })
            .collect()
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Cheap change-detector for the embeddings table. Row count catches
/// inserts/deletes; max rowid catches re-embeds, because
/// `INSERT OR REPLACE` gives the replacing row a fresh rowid. This matters
/// because reindexes can run in a DIFFERENT process (the CLI) than the
/// long-lived MCP server holding the cache — there is no in-process signal
/// to invalidate on, so the cache revalidates against this fingerprint on
/// every semantic query instead.
fn fingerprint(db: &Database, model_id: &str) -> (i64, i64) {
    db.conn
        .query_row(
            "SELECT COUNT(*), IFNULL(MAX(rowid), 0) FROM embeddings WHERE model_id = ?1",
            rusqlite::params![model_id],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        )
        .unwrap_or((0, 0))
}

struct CachedIndex {
    project_root: String,
    model_id: String,
    fingerprint: (i64, i64),
    index: Arc<BruteForceVectorSearch>,
}

static VECTOR_CACHE: Mutex<Option<CachedIndex>> = Mutex::new(None);

/// The process-wide vector index for (workspace, model), loaded on first
/// semantic query and refreshed whenever the underlying table changed
/// (including from another process — see `fingerprint`). Also refreshed on
/// workspace or model switch, since the MCP server serves one workspace at a
/// time.
pub fn cached_vector_search(
    db: &Database,
    model_id: &str,
) -> Result<Arc<BruteForceVectorSearch>, String> {
    let current = fingerprint(db, model_id);
    let mut guard = VECTOR_CACHE
        .lock()
        .map_err(|_| "vector cache poisoned by a previous panic".to_string())?;
    if let Some(cached) = guard.as_ref() {
        if cached.project_root == db.project_root
            && cached.model_id == model_id
            && cached.fingerprint == current
        {
            return Ok(Arc::clone(&cached.index));
        }
    }
    let index = Arc::new(BruteForceVectorSearch::load(db, model_id)?);
    *guard = Some(CachedIndex {
        project_root: db.project_root.clone(),
        model_id: model_id.to_string(),
        fingerprint: current,
        index: Arc::clone(&index),
    });
    Ok(index)
}
