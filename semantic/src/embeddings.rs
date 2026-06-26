use crate::provider::LlmProvider;
use std::collections::HashMap;
use std::time::Instant;
use storage::{blob_to_embedding, Database};

/// Keeps each embeddings API request to a reasonable payload size — OpenAI's
/// endpoint accepts far more than this per call, but batching too aggressively
/// just makes one slow request's failure (timeout, transient error) cost a
/// full repo's worth of retries instead of one chunk's.
const BATCH_SIZE: usize = 100;

pub struct EmbeddingBackfillStats {
    pub embedded: usize,
    pub batches: usize,
    /// Batches that failed after all retries — symbols in these batches are
    /// not embedded this run but will be picked up on the next run via
    /// `symbols_missing_embeddings`.
    pub failed_batches: usize,
}

const MAX_RETRIES: u32 = 3;

/// Embeds every symbol that doesn't have a stored embedding yet (per
/// `Database::symbols_missing_embeddings`), optionally narrowed to
/// `only_symbol_ids` — used after an incremental reindex to embed exactly the
/// symbols that were just (re)inserted, instead of rescanning the whole repo.
/// A no-op (returns `Ok` with zero counts) when there's nothing to embed, so
/// callers can call this unconditionally after every index/reindex without
/// special-casing "nothing changed."
///
/// The embedding text per symbol is intentionally short and structured
/// (`kind name in path: signature`) rather than the full source body — the
/// goal is to let a conceptual query land near the right symbol's NAME and
/// SIGNATURE, not to embed implementation details that would dilute the
/// vector with noise irrelevant to "what is this and where does it live."
pub fn backfill_missing_embeddings(
    db: &Database,
    provider: &dyn LlmProvider,
    only_symbol_ids: Option<&[i64]>,
) -> Result<EmbeddingBackfillStats, String> {
    let t_total = Instant::now();

    let t0 = Instant::now();
    let mut candidates = db.symbols_missing_embeddings().map_err(|e| e.to_string())?;
    if let Some(ids) = only_symbol_ids {
        candidates.retain(|(id, ..)| ids.contains(id));
    }
    let candidate_count = candidates.len();
    eprintln!(
        "[TIMING:embeddings] Load missing symbols query: {}ms ({} candidates)",
        t0.elapsed().as_millis(),
        candidate_count
    );

    let mut stats = EmbeddingBackfillStats {
        embedded: 0,
        batches: 0,
        failed_batches: 0,
    };
    if candidates.is_empty() {
        eprintln!("[TIMING:embeddings] No candidates — skipping. Total: {}ms", t_total.elapsed().as_millis());
        return Ok(stats);
    }

    let model = provider.embedding_model_name().to_string();
    let total_batches = (candidate_count + BATCH_SIZE - 1) / BATCH_SIZE;
    let mut total_text_prep_ms = 0u128;
    let mut total_http_ms = 0u128;
    let mut total_db_write_ms = 0u128;

    for (batch_idx, chunk) in candidates.chunks(BATCH_SIZE).enumerate() {
        let t_prep = Instant::now();
        let texts: Vec<String> = chunk
            .iter()
            .map(|(_, name, kind, path, signature, summary)| {
                let sig = signature.clone().unwrap_or_else(|| name.clone());
                if let Some(summ) = summary {
                    format!("{} {} in {}: {}\nSummary: {}", kind, name, path, sig, summ)
                } else {
                    format!("{} {} in {}: {}", kind, name, path, sig)
                }
            })
            .collect();
        let text_prep_ms = t_prep.elapsed().as_millis();
        total_text_prep_ms += text_prep_ms;

        let avg_bytes: usize = texts.iter().map(|t| t.len()).sum::<usize>() / texts.len().max(1);
        let payload_bytes: usize = texts.iter().map(|t| t.len()).sum();

        // Retry up to MAX_RETRIES times with exponential backoff. Transient
        // network errors (body truncation, connection reset) are common on
        // large responses; a single retry almost always succeeds.
        let t_http = Instant::now();
        let mut vectors_result: Result<Vec<Vec<f32>>, String> = Err(String::new());
        for attempt in 0..MAX_RETRIES {
            if attempt > 0 {
                let backoff_ms = 500u64 * (1u64 << (attempt - 1)); // 500ms, 1000ms
                eprintln!(
                    "[TIMING:embeddings] Batch {}/{} attempt {}/{} retrying in {}ms…",
                    batch_idx + 1, total_batches, attempt + 1, MAX_RETRIES, backoff_ms
                );
                std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
            }
            match provider.embed_texts(&texts) {
                Ok(v) => { vectors_result = Ok(v); break; }
                Err(e) => {
                    eprintln!(
                        "[TIMING:embeddings] Batch {}/{} attempt {}/{} failed: {}",
                        batch_idx + 1, total_batches, attempt + 1, MAX_RETRIES, e
                    );
                    vectors_result = Err(e);
                }
            }
        }
        let http_ms = t_http.elapsed().as_millis();
        total_http_ms += http_ms;

        let vectors = match vectors_result {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "[TIMING:embeddings] Batch {}/{} failed after {} retries ({} symbols skipped): {}",
                    batch_idx + 1, total_batches, MAX_RETRIES, chunk.len(), e
                );
                stats.failed_batches += 1;
                // Continue rather than abort — remaining batches may succeed,
                // and skipped symbols will be picked up on the next init/reindex.
                continue;
            }
        };

        if vectors.len() != chunk.len() {
            eprintln!(
                "[TIMING:embeddings] Batch {}/{}: vector count mismatch ({} vs {}), skipping",
                batch_idx + 1, total_batches, vectors.len(), chunk.len()
            );
            stats.failed_batches += 1;
            continue;
        }
        stats.batches += 1;

        eprintln!(
            "[TIMING:embeddings] Batch {}/{}: symbols={}, payload_bytes={}, avg_text_bytes={}, text_prep={}ms, http_request={}ms",
            batch_idx + 1,
            total_batches,
            chunk.len(),
            payload_bytes,
            avg_bytes,
            text_prep_ms,
            http_ms
        );

        let t_db = Instant::now();
        for ((id, ..), vector) in chunk.iter().zip(vectors.iter()) {
            db.upsert_symbol_embedding(*id, vector, &model)
                .map_err(|e| e.to_string())?;
            stats.embedded += 1;
        }
        let db_write_ms = t_db.elapsed().as_millis();
        total_db_write_ms += db_write_ms;
        eprintln!(
            "[TIMING:embeddings]   └─ DB writes ({} rows, individual): {}ms ({:.2}ms/row)",
            chunk.len(),
            db_write_ms,
            db_write_ms as f64 / chunk.len() as f64
        );
    }

    eprintln!(
        "[TIMING:embeddings] Summary: batches={}, failed={}, embedded={}, text_prep={}ms, http_total={}ms, db_writes={}ms, total={}ms",
        stats.batches,
        stats.failed_batches,
        stats.embedded,
        total_text_prep_ms,
        total_http_ms,
        total_db_write_ms,
        t_total.elapsed().as_millis()
    );

    Ok(stats)
}

fn embedding_cache_key(name: &str, kind: &str, path: &str, sig: &str) -> String {
    format!("{}:{}:{}:{}", name, kind, path, sig)
}

/// Loads all symbol embeddings from an existing database, keyed by a canonical
/// string of `name:kind:path:signature`. Safe to call when no database exists
/// at `db_path` — returns an empty map in that case.
///
/// Used by `codebroker init` to carry forward embeddings that are still valid
/// into the freshly-rebuilt database, so only new or modified symbols need API calls.
pub fn load_embedding_cache(db_path: &str) -> HashMap<String, (Vec<f32>, String)> {
    let mut cache = HashMap::new();
    let Ok(old_db) = Database::new(db_path) else {
        return cache;
    };
    let Ok(mut stmt) = old_db.conn.prepare(
        "SELECT s.name, s.kind, f.path, s.signature, e.embedding, e.model
         FROM symbol_embeddings e
         JOIN symbols s ON e.symbol_id = s.id
         JOIN files f ON s.file_id = f.id",
    ) else {
        return cache;
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Vec<u8>>(4)?,
            row.get::<_, String>(5)?,
        ))
    }) else {
        return cache;
    };
    for row in rows.flatten() {
        let (name, kind, path, signature, blob, model) = row;
        let sig = signature.as_deref().unwrap_or(&name).to_string();
        let key = embedding_cache_key(&name, &kind, &path, &sig);
        cache.insert(key, (blob_to_embedding(&blob), model));
    }
    cache
}

/// For each symbol in `db` whose `(name, kind, path, signature)` key exists in
/// `cache`, writes the cached embedding directly — bypassing any API call.
/// Returns the number of symbols served from cache.
///
/// Called after rebuilding symbols and before `backfill_missing_embeddings`, so
/// the backfill only sees genuinely new or changed symbols.
pub fn apply_embedding_cache(
    db: &Database,
    cache: &HashMap<String, (Vec<f32>, String)>,
) -> usize {
    if cache.is_empty() {
        return 0;
    }
    let Ok(mut stmt) = db.conn.prepare(
        "SELECT s.id, s.name, s.kind, f.path, s.signature
         FROM symbols s
         JOIN files f ON s.file_id = f.id",
    ) else {
        return 0;
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
        ))
    }) else {
        return 0;
    };
    let symbols: Vec<_> = rows.flatten().collect();
    let mut hits = 0usize;
    for (id, name, kind, path, signature) in symbols {
        let sig = signature.as_deref().unwrap_or(&name).to_string();
        let key = embedding_cache_key(&name, &kind, &path, &sig);
        if let Some((vector, model)) = cache.get(&key) {
            if db.upsert_symbol_embedding(id, vector, model).is_ok() {
                hits += 1;
            }
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::MockProvider;

    #[test]
    fn embeds_only_missing_symbols_and_is_idempotent() {
        let db = Database::new(":memory:").unwrap();
        db.init_schema().unwrap();
        let file_id = db.insert_file("a.ts", "h").unwrap();
        let id = db
            .insert_symbol(
                file_id,
                &graph::SymbolNode {
                    name: "foo".to_string(),
                    kind: "function".to_string(),
                    start_line: 1,
                    end_line: 1,
                    start_byte: 0,
                    end_byte: 0,
                    signature: None,
                    attributes: Vec::new(),
                    metadata: None,
                },
            )
            .unwrap();

        let provider = MockProvider;
        let stats = backfill_missing_embeddings(&db, &provider, None).unwrap();
        assert_eq!(stats.embedded, 1);
        assert_eq!(stats.batches, 1);

        // Re-running with nothing missing must be a true no-op (zero API calls).
        let stats2 = backfill_missing_embeddings(&db, &provider, None).unwrap();
        assert_eq!(stats2.embedded, 0);
        assert_eq!(stats2.batches, 0);

        let missing = db.symbols_missing_embeddings().unwrap();
        assert!(missing.is_empty());
        let _ = id;
    }

    #[test]
    fn only_symbol_ids_scopes_the_backfill() {
        let db = Database::new(":memory:").unwrap();
        db.init_schema().unwrap();
        let file_id = db.insert_file("a.ts", "h").unwrap();
        let keep_unembedded = db
            .insert_symbol(
                file_id,
                &graph::SymbolNode {
                    name: "untouched".to_string(),
                    kind: "function".to_string(),
                    start_line: 1,
                    end_line: 1,
                    start_byte: 0,
                    end_byte: 0,
                    signature: None,
                    attributes: Vec::new(),
                    metadata: None,
                },
            )
            .unwrap();
        let touched = db
            .insert_symbol(
                file_id,
                &graph::SymbolNode {
                    name: "touched".to_string(),
                    kind: "function".to_string(),
                    start_line: 2,
                    end_line: 2,
                    start_byte: 0,
                    end_byte: 0,
                    signature: None,
                    attributes: Vec::new(),
                    metadata: None,
                },
            )
            .unwrap();

        let provider = MockProvider;
        let stats = backfill_missing_embeddings(&db, &provider, Some(&[touched])).unwrap();
        assert_eq!(stats.embedded, 1);

        let missing = db.symbols_missing_embeddings().unwrap();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].0, keep_unembedded);
    }
}
