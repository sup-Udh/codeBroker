use rusqlite::Connection;
use crate::node::{SemanticNode, SemanticNodeKind};

pub struct GraphQueryService<'a> {
    pub conn: &'a Connection,
    pub project_root: &'a str,
    // Add caching structures here later, e.g.
    // node_cache: HashMap<i64, SemanticNode>,
}

impl<'a> GraphQueryService<'a> {
    pub fn new(conn: &'a Connection, project_root: &'a str) -> Self {
        Self { conn, project_root }
    }

    /// Read-only API to get a single node by its resolved symbol id — the
    /// one place any traversal/formatter should fetch node metadata from, so
    /// a node looked up this way can never diverge from what
    /// `resolver::resolve_symbol` actually resolved (unlike re-deriving it
    /// from name+file_hint a second time).
    pub fn get_node(&self, id: i64) -> Option<SemanticNode> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT symbols.name, files.path, symbols.start_byte, symbols.end_byte,
                        COALESCE(sf.is_entrypoint, 0)
                 FROM symbols
                 JOIN files ON symbols.file_id = files.id
                 LEFT JOIN symbol_features sf ON sf.symbol_id = symbols.id
                 WHERE symbols.id = ?1",
            )
            .ok()?;
        stmt.query_row(rusqlite::params![id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, bool>(4)?,
            ))
        })
        .ok()
        .map(|(name, path, start_byte, end_byte, is_entrypoint)| {
            let kind = if is_entrypoint {
                SemanticNodeKind::Entrypoint
            } else {
                SemanticNodeKind::Symbol
            };
            SemanticNode::new(
                id,
                name.clone(),
                name,
                kind,
                None,
                Some((start_byte as usize, end_byte as usize)),
                path,
            )
        })
    }

    // Methods for lazily loading callers, callees, dependencies, etc.
}
