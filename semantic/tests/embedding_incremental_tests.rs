//! Incremental-embedding behavior: body_hash skip logic, only-changed-file
//! re-embeds, and vector cleanup for removed symbols. All offline via
//! `MockEmbedder` — no model download, no network.

use semantic::embedder::{Embedder, MockEmbedder, MOCK_MODEL_ID};
use semantic::embeddings::backfill_embeddings;
use std::sync::atomic::Ordering;
use storage::Database;

struct Fixture {
    root: std::path::PathBuf,
    db: Database,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let unique = format!(
            "codebroker_test_embed_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(root.join(".codebroker")).unwrap();
        let db_path = root.join(".codebroker").join("codebroker.db");
        let db = Database::new(db_path.to_str().unwrap()).unwrap();
        db.init_schema().unwrap();
        Fixture { root, db }
    }

    fn write(&self, rel: &str, content: &str) {
        let path = self.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn reindex(&self, paths: &[&str]) -> indexer::reindex::IncrementalStats {
        let paths: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
        indexer::reindex::reindex_paths(&self.db, self.root.to_str().unwrap(), &paths).unwrap()
    }

    fn symbol_count(&self) -> i64 {
        self.db
            .conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))
            .unwrap()
    }

    fn embedding_count(&self) -> i64 {
        self.db.count_symbol_embeddings(MOCK_MODEL_ID).unwrap()
    }

    fn orphan_embedding_count(&self) -> i64 {
        self.db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM embeddings WHERE symbol_id NOT IN (SELECT id FROM symbols)",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

const TIME_FORMAT_TS: &str = "/// Formats how long ago a timestamp was.\n\
export function timeAgo(d) {\n  return d;\n}\n\n\
export function formatDate(d) {\n  return String(d);\n}\n";

const DASHBOARD_TS: &str = "import { timeAgo } from \"../lib/timeFormat\";\n\
export function render() {\n  return timeAgo(1);\n}\n";

#[test]
fn unchanged_symbols_are_skipped_by_body_hash() {
    let fx = Fixture::new("skip");
    fx.write("lib/timeFormat.ts", TIME_FORMAT_TS);
    fx.write("pages/dashboard.ts", DASHBOARD_TS);
    fx.reindex(&["lib/timeFormat.ts", "pages/dashboard.ts"]);

    let mock = MockEmbedder::new();
    let first = backfill_embeddings(&fx.db, &mock, None, None).unwrap();
    let total_symbols = fx.symbol_count() as usize;
    assert!(total_symbols >= 3, "fixture should index timeAgo/formatDate/render");
    assert_eq!(first.embedded, total_symbols, "first pass embeds everything");
    assert_eq!(fx.embedding_count() as usize, total_symbols);

    // Second pass over identical content: body hashes all match — the
    // embedder must not be called for a single text.
    let embedded_before = mock.texts_embedded.load(Ordering::SeqCst);
    let second = backfill_embeddings(&fx.db, &mock, None, None).unwrap();
    assert_eq!(second.embedded, 0);
    assert_eq!(second.skipped_unchanged, total_symbols);
    assert_eq!(
        mock.texts_embedded.load(Ordering::SeqCst),
        embedded_before,
        "no texts may be re-embedded when nothing changed"
    );
}

#[test]
fn editing_one_file_reembeds_only_its_symbols() {
    let fx = Fixture::new("onefile");
    fx.write("lib/timeFormat.ts", TIME_FORMAT_TS);
    fx.write("pages/dashboard.ts", DASHBOARD_TS);
    fx.reindex(&["lib/timeFormat.ts", "pages/dashboard.ts"]);

    let mock = MockEmbedder::new();
    backfill_embeddings(&fx.db, &mock, None, None).unwrap();

    // Edit ONLY timeFormat.ts (change timeAgo's body, keep formatDate as-is)
    // and incrementally reindex just that file, mirroring the CLI's
    // reindex-incremental flow: backfill scoped to touched_symbol_ids.
    fx.write(
        "lib/timeFormat.ts",
        "/// Formats how long ago a timestamp was.\n\
         export function timeAgo(d) {\n  return `${d} ago`;\n}\n\n\
         export function formatDate(d) {\n  return String(d);\n}\n",
    );
    let stats = fx.reindex(&["lib/timeFormat.ts"]);
    assert!(!stats.touched_symbol_ids.is_empty());

    let embedded_before = mock.texts_embedded.load(Ordering::SeqCst);
    let pass = backfill_embeddings(&fx.db, &mock, Some(&stats.touched_symbol_ids), None).unwrap();

    // timeAgo's card changed -> re-embedded. formatDate was re-inserted with
    // a new id by the reindex (so it lost its old vector and must be written
    // again), but dashboard.ts's render was untouched entirely.
    assert!(pass.embedded >= 1, "the edited symbol must be re-embedded");
    let touched: usize = stats.touched_symbol_ids.len();
    assert!(
        mock.texts_embedded.load(Ordering::SeqCst) - embedded_before <= touched,
        "embedding calls must be scoped to the touched file's symbols"
    );

    // render (other file) kept its original embedding row.
    let render_rows: i64 = fx
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM embeddings JOIN symbols ON embeddings.symbol_id = symbols.id
             WHERE symbols.name = 'render'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(render_rows, 1, "untouched file's vector must survive");

    // Every symbol embedded, no orphans.
    assert_eq!(fx.embedding_count(), fx.symbol_count());
    assert_eq!(fx.orphan_embedding_count(), 0);
}

#[test]
fn removed_symbols_vectors_are_deleted() {
    let fx = Fixture::new("removed");
    fx.write("lib/timeFormat.ts", TIME_FORMAT_TS);
    fx.reindex(&["lib/timeFormat.ts"]);

    let mock = MockEmbedder::new();
    backfill_embeddings(&fx.db, &mock, None, None).unwrap();
    let initial = fx.embedding_count();
    assert!(initial >= 2, "both timeAgo and formatDate embedded");

    // Rewrite the file WITHOUT formatDate and reindex: delete_file_data must
    // remove the old vectors, and the backfill must not resurrect them.
    fx.write(
        "lib/timeFormat.ts",
        "/// Formats how long ago a timestamp was.\n\
         export function timeAgo(d) {\n  return d;\n}\n",
    );
    let stats = fx.reindex(&["lib/timeFormat.ts"]);
    backfill_embeddings(&fx.db, &mock, Some(&stats.touched_symbol_ids), None).unwrap();

    assert_eq!(fx.orphan_embedding_count(), 0, "no vectors for deleted symbol ids");
    let format_date_rows: i64 = fx
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM embeddings JOIN symbols ON embeddings.symbol_id = symbols.id
             WHERE symbols.name = 'formatDate'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(format_date_rows, 0, "removed symbol must have no vector");
    assert_eq!(fx.embedding_count(), fx.symbol_count());
}

#[test]
fn body_hash_reuse_cache_avoids_model_calls_across_rebuilds() {
    let fx = Fixture::new("reuse");
    fx.write("lib/timeFormat.ts", TIME_FORMAT_TS);
    fx.reindex(&["lib/timeFormat.ts"]);

    let mock = MockEmbedder::new();
    backfill_embeddings(&fx.db, &mock, None, None).unwrap();
    let reuse: std::collections::HashMap<String, Vec<u8>> = fx
        .db
        .embeddings_by_body_hash(MOCK_MODEL_ID)
        .unwrap()
        .into_iter()
        .collect();
    assert!(!reuse.is_empty());

    // Simulate a full rebuild: wipe embeddings (symbol ids would be fresh in
    // a real rebuild; clearing rows models "no vectors for these new ids").
    fx.db.conn.execute("DELETE FROM embeddings", []).unwrap();

    let embedded_before = mock.texts_embedded.load(Ordering::SeqCst);
    let pass = backfill_embeddings(&fx.db, &mock, None, Some(&reuse)).unwrap();
    assert_eq!(pass.embedded, 0, "identical content must be served from the reuse cache");
    assert_eq!(pass.reused as i64, fx.symbol_count());
    assert_eq!(
        mock.texts_embedded.load(Ordering::SeqCst),
        embedded_before,
        "the model must not be called for reused vectors"
    );
    assert_eq!(fx.embedding_count(), fx.symbol_count());
}

#[test]
fn mock_embedder_is_deterministic() {
    let mock = MockEmbedder::new();
    let a = mock.embed(&["function timeAgo".to_string()]).unwrap();
    let b = mock.embed(&["function timeAgo".to_string()]).unwrap();
    assert_eq!(a, b);
    assert_eq!(a[0].len(), mock.dims());
}
