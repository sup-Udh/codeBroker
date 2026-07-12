use crate::cards;
use crate::embedder::Embedder;
use std::collections::HashMap;
use storage::Database;

/// Result of one backfill pass.
#[derive(Debug, Default)]
pub struct BackfillStats {
    /// Symbols embedded fresh (model/API actually called for them).
    pub embedded: usize,
    /// Symbols whose vector was carried over from `reuse` by body_hash —
    /// no model call needed (full-rebuild path).
    pub reused: usize,
    /// Symbols whose stored body_hash already matched — nothing written.
    pub skipped_unchanged: usize,
    /// Batches that failed after the embedder's own retries. Their symbols
    /// stay un-embedded this pass and are picked up by the next one.
    pub failed_batches: usize,
}

/// Same chunk size the embedding APIs are batched at; for the local model
/// this just bounds peak tokenization memory.
const EMBED_BATCH_SIZE: usize = 100;

/// Brings the `embeddings` table up to date with the symbol table for
/// `embedder`'s model:
///
/// - builds each symbol's card (see `cards::build_card`) from the file on
///   disk and hashes it;
/// - skips symbols whose stored `body_hash` for this model already matches;
/// - reuses vectors from `reuse` (old-database carry-over keyed by
///   body_hash) when provided;
/// - embeds the rest in batches, then writes every new row inside ONE
///   transaction with a cached prepared statement.
///
/// `only_symbol_ids` narrows the pass to an incremental reindex's touched
/// symbols instead of rescanning the whole repo. Vectors of REMOVED symbols
/// are not this function's job: `Database::delete_file_data` deletes them
/// when the file is re-parsed (and a full rebuild starts from an empty
/// table).
///
/// Symbols in ignored paths never appear here at all — candidates come from
/// the `symbols` table, which only ever contains files the indexer's
/// ignore-aware walker accepted.
///
/// This runs strictly AFTER symbol/graph indexing and must never fail it:
/// callers treat an `Err` as "semantic search degraded", log it, and move on.
pub fn backfill_embeddings(
    db: &Database,
    embedder: &dyn Embedder,
    only_symbol_ids: Option<&[i64]>,
    reuse: Option<&HashMap<String, Vec<u8>>>,
) -> Result<BackfillStats, String> {
    let model_id = embedder.model_id().to_string();
    let mut candidates = db.embedding_candidates(&model_id).map_err(|e| e.to_string())?;
    if let Some(ids) = only_symbol_ids {
        let ids: std::collections::HashSet<i64> = ids.iter().copied().collect();
        candidates.retain(|(id, ..)| ids.contains(id));
    }

    let mut stats = BackfillStats::default();
    if candidates.is_empty() {
        return Ok(stats);
    }

    // One file read per file, not per symbol.
    let mut file_cache: HashMap<String, Option<String>> = HashMap::new();

    // (symbol_id, body_hash, card) still needing a model call, and
    // (symbol_id, body_hash, vector) rows ready to write.
    let mut to_embed: Vec<(i64, String, String)> = Vec::new();
    let mut to_write: Vec<(i64, String, Vec<f32>)> = Vec::new();

    for (symbol_id, name, kind, path, signature, start_byte, end_byte, _line, stored_hash) in
        &candidates
    {
        let content = file_cache
            .entry(path.clone())
            .or_insert_with(|| std::fs::read_to_string(db.resolve_path(path)).ok());
        let Some(content) = content else {
            continue; // unreadable/deleted file; its rows go away on next reindex
        };
        let card = cards::build_card(
            path,
            kind,
            name,
            signature.as_deref(),
            content,
            *start_byte as usize,
            *end_byte as usize,
        );
        let hash = cards::card_hash(&card);
        if stored_hash.as_deref() == Some(hash.as_str()) {
            stats.skipped_unchanged += 1;
            continue;
        }
        if let Some(reuse) = reuse {
            if let Some(blob) = reuse.get(&hash) {
                to_write.push((*symbol_id, hash, storage::blob_to_embedding(blob)));
                stats.reused += 1;
                continue;
            }
        }
        to_embed.push((*symbol_id, hash, card));
    }

    // Embed everything BEFORE opening the write transaction: a transaction
    // held across model inference (or worse, API round-trips) would block
    // every other writer on this database for its whole duration.
    for chunk in to_embed.chunks(EMBED_BATCH_SIZE) {
        let texts: Vec<String> = chunk.iter().map(|(_, _, card)| card.clone()).collect();
        match embedder.embed(&texts) {
            Ok(vectors) if vectors.len() == chunk.len() => {
                for ((symbol_id, hash, _), vector) in chunk.iter().zip(vectors) {
                    to_write.push((*symbol_id, hash.clone(), vector));
                    stats.embedded += 1;
                }
            }
            Ok(vectors) => {
                eprintln!(
                    "[semantic] embedding batch returned {} vectors for {} texts; skipping batch",
                    vectors.len(),
                    chunk.len()
                );
                stats.failed_batches += 1;
            }
            Err(e) => {
                eprintln!("[semantic] embedding batch failed: {}", e);
                stats.failed_batches += 1;
                // Keep going: later batches may succeed, and failed symbols
                // are retried by the next backfill via their missing rows.
            }
        }
    }

    if to_write.is_empty() {
        return Ok(stats);
    }

    db.conn
        .execute_batch("BEGIN IMMEDIATE")
        .map_err(|e| e.to_string())?;
    let write_result: Result<(), String> = (|| {
        for (symbol_id, hash, vector) in &to_write {
            db.upsert_symbol_embedding(*symbol_id, &model_id, vector, hash)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    match write_result {
        Ok(()) => db.conn.execute_batch("COMMIT").map_err(|e| e.to_string())?,
        Err(e) => {
            let _ = db.conn.execute_batch("ROLLBACK");
            return Err(e);
        }
    }
    Ok(stats)
}

/// Loads the previous published database's vectors keyed by body_hash, for
/// carry-over across a full `init` rebuild (which reassigns every symbol id
/// but leaves most symbols' embeddable content — and therefore hash —
/// untouched). Returns an empty map when there is no previous database or it
/// has no vectors for this model; the caller then simply embeds everything.
pub fn load_reuse_cache(old_db_path: &str, model_id: &str) -> HashMap<String, Vec<u8>> {
    if !std::path::Path::new(old_db_path).exists() {
        return HashMap::new();
    }
    match Database::new(old_db_path) {
        Ok(old_db) => old_db
            .embeddings_by_body_hash(model_id)
            .map(|rows| rows.into_iter().collect())
            .unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}
