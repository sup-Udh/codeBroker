//! Domain-concept tagging: assigns symbols a small set of free-text concept
//! labels (auth, realtime, notifications, database, ...) based on keyword
//! matches against the symbol's own name and its file's path. This runs
//! independently of (and in addition to) literal substring/path search —
//! `search_codebase`/`subsystem_stats`/`prepare_context` consult it
//! so a query like "authentication system" can surface `createClient`/
//! `createAdminClient`/`signInWithOAuth` even though none of those names or
//! their `utils/supabase/*.ts` path contain the literal word "auth" anywhere
//! a plain substring search would catch on the symbol/file name alone
//! (benchmark run_005's central finding: "auth" and "supabase" queries
//! returned almost nothing despite the real auth system being large and
//! central to the repo).

use rusqlite::Result;
use storage::Database;

/// (concept_name, keywords). A symbol/file matching ANY keyword in a
/// concept's list gets tagged with that concept. Keywords deliberately
/// overlap across concepts (e.g. "supabase" appears under both `auth` and
/// `database`) since the same piece of infrastructure often serves more
/// than one domain concept in a real codebase.
pub const CONCEPTS: &[(&str, &[&str])] = &[
    (
        "auth",
        &[
            "auth", "oauth", "login", "logout", "session", "signin", "signup", "supabase",
        ],
    ),
    (
        "realtime",
        &[
            "websocket",
            "awareness",
            "yjs",
            "collaboration",
            "collaborative",
            "room",
            "broadcast",
            "presence",
        ],
    ),
    (
        "notifications",
        &["notification", "notify", "toast", "alert"],
    ),
    (
        "database",
        &[
            "postgres", "supabase", "prisma", "storage", "sqlite", "database", "db",
        ],
    ),
];

/// Returns the concept names whose keyword list contains `term` (case
/// insensitive, substring match either direction) — used by
/// `search_codebase`/`prepare_context` to map a free-text query
/// term onto the concept(s) it should also search by, not just literal
/// name/path matching.
pub fn concepts_matching_term(term: &str) -> Vec<&'static str> {
    let term_lower = term.to_lowercase();
    if term_lower.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (concept, keywords) in CONCEPTS {
        if *concept == term_lower {
            out.push(*concept);
            continue;
        }
        if keywords
            .iter()
            .any(|kw| term_lower.contains(kw) || kw.contains(&term_lower))
        {
            out.push(*concept);
        }
    }
    out
}

/// Re-tags every indexed symbol with domain concepts based on its own name
/// and its file's path. Clears and rebuilds the table on every call so stale
/// tags from a renamed/deleted symbol never linger — cheap at the symbol
/// counts CodeBroker indexes (hundreds to low thousands), not worth doing
/// incrementally. Call once per (re)index, alongside `detect_logical_edges`.
pub fn tag_concepts(db: &Database) -> Result<usize> {
    db.conn.execute("DELETE FROM symbol_concepts", [])?;

    let mut stmt = db.conn.prepare(
        "SELECT symbols.id, symbols.name, files.path FROM symbols JOIN files ON symbols.file_id = files.id",
    )?;
    let mut rows = stmt.query([])?;

    let mut tagged = 0usize;
    while let Some(row) = rows.next()? {
        let symbol_id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        let path: String = row.get(2)?;
        let name_lower = name.to_lowercase();
        let path_lower = path.to_lowercase();

        for (concept, keywords) in CONCEPTS {
            let name_hit = keywords.iter().any(|kw| name_lower.contains(kw));
            let path_hit = !name_hit && keywords.iter().any(|kw| path_lower.contains(kw));
            if name_hit || path_hit {
                let matched_on = if name_hit { "name" } else { "path" };
                db.conn.execute(
                    "INSERT OR IGNORE INTO symbol_concepts (symbol_id, concept, matched_on) VALUES (?1, ?2, ?3)",
                    rusqlite::params![symbol_id, concept, matched_on],
                )?;
                tagged += 1;
            }
        }
    }

    Ok(tagged)
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ConceptMatch {
    pub symbol_name: String,
    pub symbol_kind: String,
    pub file_path: String,
    pub concept: String,
    pub matched_on: String,
}

/// All symbols tagged with `concept`, joined back to their name/kind/path.
/// Used by `search_codebase` (when the query term maps to a concept) and
/// `subsystem_stats`/`subsystem_overview` (to pull in symbols a literal
/// name/path substring match would miss entirely).
pub fn symbols_for_concept(db: &Database, concept: &str) -> Result<Vec<ConceptMatch>> {
    let mut stmt = db.conn.prepare(
        "SELECT symbols.name, symbols.kind, files.path, symbol_concepts.concept, symbol_concepts.matched_on
         FROM symbol_concepts
         JOIN symbols ON symbol_concepts.symbol_id = symbols.id
         JOIN files ON symbols.file_id = files.id
         WHERE symbol_concepts.concept = ?1",
    )?;
    let mut rows = stmt.query(rusqlite::params![concept])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(ConceptMatch {
            symbol_name: row.get(0)?,
            symbol_kind: row.get(1)?,
            file_path: db.resolve_path(&row.get::<_, String>(2)?),
            concept: row.get(3)?,
            matched_on: row.get(4)?,
        });
    }
    Ok(out)
}
