use std::cell::RefCell;
use std::collections::HashMap;
use tree_sitter::{Language, Parser, Query};

thread_local! {
    static PARSERS: RefCell<HashMap<&'static str, Parser>> = RefCell::new(HashMap::new());
    static QUERIES: RefCell<HashMap<&'static str, Query>> = RefCell::new(HashMap::new());
}

/// Runs `f` against a thread-local `Parser` cached under `key`. Each worker
/// thread (e.g. a rayon pool thread) builds its own `Parser` once per key on
/// first use and reuses it for every subsequent file that thread processes,
/// instead of paying `Parser::new()` + `set_language()` on every single file.
/// `tree_sitter::Parser` is not `Sync`, so a thread-local cache (never shared
/// across threads) is the pooling strategy that needs no unsafe code.
pub fn with_parser<R>(
    key: &'static str,
    language: &Language,
    f: impl FnOnce(&mut Parser) -> R,
) -> R {
    PARSERS.with(|cache| {
        let mut cache = cache.borrow_mut();
        let parser = cache.entry(key).or_insert_with(|| {
            let mut p = Parser::new();
            p.set_language(language)
                .expect("failed to set tree-sitter language");
            p
        });
        f(parser)
    })
}

/// Same idea for `Query`: compiling a query from its S-expression source
/// string is nontrivial, so each (thread, key) pair compiles it at most once.
pub fn with_query<R>(
    key: &'static str,
    language: &Language,
    query_str: &str,
    f: impl FnOnce(&Query) -> R,
) -> R {
    QUERIES.with(|cache| {
        let mut cache = cache.borrow_mut();
        let query = cache
            .entry(key)
            .or_insert_with(|| Query::new(language, query_str).expect("Invalid Tree-sitter query"));
        f(query)
    })
}
