//! End-to-end semantic retrieval on a fixture repo with the REAL local
//! embedding model (fastembed, bge-small-en-v1.5): a conceptual query must
//! retrieve a `timeAgo`-style symbol that keyword symbol search misses.
//!
//! The model is ~100MB, downloaded once to ~/.codebroker/models. To keep
//! `cargo test` usable offline, the real-model test runs only when the model
//! is already cached locally or `CODEBROKER_EMBED_TESTS=1` opts in; it skips
//! (passing, with a note) otherwise. The degraded-path test below always runs.

use query::engine::SearchMode;
use semantic::config::EmbeddingsConfig;
use storage::Database;

fn fixture(tag: &str) -> (Database, std::path::PathBuf) {
    let unique = format!(
        "codebroker_test_semantic_{}_{}_{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let root = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(root.join(".codebroker")).unwrap();
    std::fs::create_dir_all(root.join("lib")).unwrap();

    // The target: a relative-time helper whose NAME shares no token with the
    // conceptual query below. Keyword symbol search cannot find it; the
    // embedding of its card (doc comment + body) can.
    std::fs::write(
        root.join("lib/relative.ts"),
        "/// Returns how long ago a timestamp occurred, like \"3 minutes ago\".\n\
         export function timeSince(timestamp) {\n\
           const seconds = Math.floor((Date.now() - timestamp) / 1000);\n\
           if (seconds < 60) return `${seconds} seconds ago`;\n\
           const minutes = Math.floor(seconds / 60);\n\
           return `${minutes} minutes ago`;\n\
         }\n",
    )
    .unwrap();
    // Distractors so ranking the target first actually means something.
    std::fs::write(
        root.join("lib/upload.ts"),
        "export function uploadFile(bytes) {\n  return fetch('/api/upload', { method: 'POST', body: bytes });\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("lib/config.ts"),
        "export function parseConfig(raw) {\n  return JSON.parse(raw);\n}\n",
    )
    .unwrap();

    let db_path = root.join(".codebroker").join("codebroker.db");
    let db = Database::new(db_path.to_str().unwrap()).unwrap();
    db.init_schema().unwrap();
    indexer::reindex::reindex_paths(
        &db,
        root.to_str().unwrap(),
        &[
            "lib/relative.ts".to_string(),
            "lib/upload.ts".to_string(),
            "lib/config.ts".to_string(),
        ],
    )
    .unwrap();
    (db, root)
}

fn local_model_cached() -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    let dir = std::path::Path::new(&home).join(".codebroker").join("models");
    dir.exists()
        && std::fs::read_dir(&dir)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false)
}

const CONCEPTUAL_QUERY: &str = "how long ago was this active";

#[test]
fn conceptual_query_finds_relative_time_symbol_keyword_search_misses() {
    if !local_model_cached() && std::env::var("CODEBROKER_EMBED_TESTS").is_err() {
        eprintln!(
            "SKIP: local embedding model not cached and CODEBROKER_EMBED_TESTS not set — \
             run with CODEBROKER_EMBED_TESTS=1 to download bge-small-en-v1.5 and run this test"
        );
        return;
    }

    let (db, root) = fixture("conceptual");

    // Premise: keyword symbol search does NOT find timeSince for this query.
    let (keyword_results, ..) = query::engine::search_symbols(
        &db,
        CONCEPTUAL_QUERY,
        None,
        SearchMode::Symbol,
        false,
        10,
    )
    .unwrap();
    assert!(
        !keyword_results.iter().any(|r| r.name == "timeSince"),
        "fixture invalid: keyword search already finds timeSince, so this test proves nothing"
    );

    // Embed the fixture with the real local model (default config).
    let config = EmbeddingsConfig::load(&db.project_root);
    let embedder = match semantic::embedder::embedder_from_config(&config) {
        Ok(e) => e,
        Err(e) => panic!("default local embedder must construct: {}", e),
    };
    let stats = match semantic::embeddings::backfill_embeddings(&db, embedder.as_ref(), None, None, None)
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("SKIP: model unavailable in this environment: {}", e);
            std::fs::remove_dir_all(&root).ok();
            return;
        }
    };
    if stats.embedded == 0 && stats.failed_batches > 0 {
        eprintln!("SKIP: embedding batches failed (likely no network for model download)");
        std::fs::remove_dir_all(&root).ok();
        return;
    }

    // Pure semantic retrieval must rank the relative-time helper first.
    let outcome = semantic::search::resolve_search_semantic(
        &db,
        CONCEPTUAL_QUERY,
        None,
        SearchMode::Semantic,
        false,
        5,
        false,
    );
    assert_eq!(outcome.semantic_search_available, Some(true));
    assert_eq!(outcome.semantic_degraded_reason, None);
    assert_eq!(
        outcome.results.first().map(|r| r.name.as_str()),
        Some("timeSince"),
        "conceptual query must retrieve the relative-time symbol; got {:?}",
        outcome
            .results
            .iter()
            .map(|r| (&r.name, &r.confidence))
            .collect::<Vec<_>>()
    );
    assert!(
        outcome.results[0].confidence.starts_with("Semantic (cosine "),
        "semantic hits carry a cosine confidence label, got '{}'",
        outcome.results[0].confidence
    );

    // Hybrid mode must ALSO surface it (fused with keyword results).
    let hybrid = semantic::search::resolve_search_semantic(
        &db,
        CONCEPTUAL_QUERY,
        None,
        SearchMode::Both,
        false,
        5,
        false,
    );
    assert!(
        hybrid.results.iter().any(|r| r.name == "timeSince"),
        "hybrid mode must include the semantic hit"
    );

    std::fs::remove_dir_all(&root).ok();
}

/// Degrade loudly but partially — no model needed for this test: with no
/// embeddings stored, semantic mode must fall back to keyword results with a
/// reason, never error out.
#[test]
fn semantic_mode_degrades_to_keyword_results_with_reason() {
    let (db, root) = fixture("degraded");

    let outcome = semantic::search::resolve_search_semantic(
        &db,
        "timeSince",
        None,
        SearchMode::Semantic,
        false,
        10,
        false,
    );
    assert_eq!(outcome.semantic_search_available, Some(false));
    let why = outcome
        .semantic_degraded_reason
        .expect("degraded search must say why");
    assert!(
        why.contains("no embeddings stored"),
        "reason should be actionable, got: {}",
        why
    );
    // The keyword fallback still finds the exact identifier.
    assert!(
        outcome.results.iter().any(|r| r.name == "timeSince"),
        "keyword fallback results must be returned on degradation"
    );

    // Lexical modes never report semantic fields at all.
    let lexical = semantic::search::resolve_search_semantic(
        &db,
        "timeSince",
        None,
        SearchMode::Symbol,
        false,
        10,
        false,
    );
    assert_eq!(lexical.semantic_search_available, None);
    assert_eq!(lexical.semantic_degraded_reason, None);

    std::fs::remove_dir_all(&root).ok();
}
