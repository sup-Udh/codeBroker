use rusqlite::Connection;
use crate::node::SemanticNode;

pub struct GraphStore<'a> {
    pub conn: &'a Connection,
    pub project_root: &'a str,
}

impl<'a> GraphStore<'a> {
    pub fn new(conn: &'a Connection, project_root: &'a str) -> Self {
        Self { conn, project_root }
    }

    // Methods for inserting and updating graph relationships go here.
    // E.g. inserting edges, linking features, inserting SemanticNodes.
}
