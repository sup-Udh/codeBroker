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
     pub fn insert_raw_import(&self, file_id: i64, import: &ImportNode) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO raw_imports (file_id, name, line_number) VALUES (?1, ?2, ?3)",
            params![file_id, import.name, import.line_number as i64],
        )?;
        Ok(self.conn.last_insert_rowid())
    
    }

    // new methods: 

        /// Pass 2 Helper: Gets all staged imports that need to be resolved
    pub fn get_all_raw_imports(&self) -> Result<Vec<(i64, i64, String)>> {
        // Returns a tuple of (raw_import_id, file_id, import_name)
        let mut stmt = self.conn.prepare("SELECT id, file_id, name FROM raw_imports")?;
        
        let import_iter = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;

        let mut results = Vec::new();
        for import in import_iter {
            results.push(import?);
        }
        Ok(results)
    }

    /// Pass 2 Helper: Tries to find a physical symbol matching the import name
    pub fn find_symbol_id_by_name(&self, name: &str) -> Result<Option<i64>> {
        let mut stmt = self.conn.prepare("SELECT id FROM symbols WHERE name = ?1 LIMIT 1")?;
        
        // We use query_row because we only expect 0 or 1 result
        let result = stmt.query_row(params![name], |row| row.get(0));

        match result {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
    


    /// Creates a directional relationship between two symbols
    pub fn insert_edge(&self, source_file_id: i64, target_symbol_id: i64, kind: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO edges (source_file_id, target_symbol_id, kind) VALUES (?1, ?2, ?3)",
            params![source_file_id, target_symbol_id, kind],
        )?;
        Ok(())
    }
}