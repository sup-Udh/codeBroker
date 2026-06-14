use rusqlite::{Connection, Result};
use crate::schema::INIT_SQL;

pub struct Database {
    pub conn: Connection,
}

impl Database {
    /// Opens a connection to the SQLite database at the given path
    pub fn new(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        Ok(Database { conn })
    }
    /// Creates the tables if they don't already exist
    pub fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(INIT_SQL)?;
        Ok(())
    }
}