use rusqlite::{Connection, Result};
use rusqlite::params;
use graph::{SymbolNode, ImportNode};

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

    /// Inserts a file and returns its new SQLite ID
    pub fn insert_file(&self, path: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO files (path) VALUES (?1)",
            params![path],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Inserts a symbol attached to a specific file
    pub fn insert_symbol(&self, file_id: i64, symbol: &SymbolNode) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO symbols (file_id, name, kind, line_number) VALUES (?1, ?2, ?3, ?4)",
            params![file_id, symbol.name, symbol.kind, symbol.line_number as i64],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Inserts an import as a special kind of symbol
    pub fn insert_import(&self, file_id: i64, import: &ImportNode) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO symbols (file_id, name, kind, line_number) VALUES (?1, ?2, 'import', ?3)",
            params![file_id, import.name, import.line_number as i64],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Creates a directional relationship between two symbols
    pub fn insert_edge(&self, source_symbol_id: i64, target_symbol_id: i64, kind: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO edges (source_symbol_id, target_symbol_id, kind) VALUES (?1, ?2, ?3)",
            params![source_symbol_id, target_symbol_id, kind],
        )?;
        Ok(())
    }
}