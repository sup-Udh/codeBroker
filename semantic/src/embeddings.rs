use crate::provider::LlmProvider;
use storage::Database;

/// Keeps each embeddings API request to a reasonable payload size — OpenAI's
/// endpoint accepts far more than this per call, but batching too aggressively
/// just makes one slow request's failure (timeout, transient error) cost a
/// full repo's worth of retries instead of one chunk's.
const BATCH_SIZE: usize = 100;

pub struct EmbeddingBackfillStats {
    pub embedded: usize,
    pub batches: usize,
}

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
    let mut candidates = db.symbols_missing_embeddings().map_err(|e| e.to_string())?;
    if let Some(ids) = only_symbol_ids {
        candidates.retain(|(id, ..)| ids.contains(id));
    }

    let mut stats = EmbeddingBackfillStats {
        embedded: 0,
        batches: 0,
    };
    if candidates.is_empty() {
        return Ok(stats);
    }

    let model = provider.embedding_model_name().to_string();

    for chunk in candidates.chunks(BATCH_SIZE) {
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

        let vectors = provider.embed_texts(&texts)?;
        if vectors.len() != chunk.len() {
            return Err(format!(
                "embed_texts returned {} vectors for {} inputs",
                vectors.len(),
                chunk.len()
            ));
        }
        stats.batches += 1;

        for ((id, ..), vector) in chunk.iter().zip(vectors.iter()) {
            db.upsert_symbol_embedding(*id, vector, &model)
                .map_err(|e| e.to_string())?;
            stats.embedded += 1;
        }
    }

    Ok(stats)
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
                    prop_type: None,
                    start_line: 1,
                    end_line: 1,
                    start_byte: 0,
                    end_byte: 0,
                    signature: None, route_path: None, route_method: None,
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
                    prop_type: None,
                    start_line: 1,
                    end_line: 1,
                    start_byte: 0,
                    end_byte: 0,
                    signature: None, route_path: None, route_method: None,
                },
            )
            .unwrap();
        let touched = db
            .insert_symbol(
                file_id,
                &graph::SymbolNode {
                    name: "touched".to_string(),
                    kind: "function".to_string(),
                    prop_type: None,
                    start_line: 2,
                    end_line: 2,
                    start_byte: 0,
                    end_byte: 0,
                    signature: None, route_path: None, route_method: None,
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
