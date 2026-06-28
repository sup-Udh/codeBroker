use storage::Database;
use indexer::reindex::reindex_paths;
use tempfile::tempdir;
use std::fs;
use std::path::PathBuf;
use rusqlite::Connection;

#[test]
fn test_regression_typescript() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("codebroker.db");
    
    let file1 = dir.path().join("file1.ts");
    let file2 = dir.path().join("file2.ts");
    
    fs::write(&file1, "export class Auth { login() {} }").unwrap();
    fs::write(&file2, "import { Auth } from './file1'; const a = new Auth(); a.login();").unwrap();
    
    let db = Database::new(db_path.to_str().unwrap()).unwrap();
    db.init_schema().unwrap();
    
    let root = dir.path().to_str().unwrap();
    reindex_paths(&db, root, &["file1.ts".to_string(), "file2.ts".to_string()]).unwrap();
    
    let files: Vec<(i64, String)> = db.conn.prepare("SELECT id, path FROM files").unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().map(|r| r.unwrap()).collect();
    println!("Files in DB: {:?}", files);
    
    let edges_list: Vec<(i64, i64, String)> = db.conn.prepare("SELECT source_file_id, target_symbol_id, kind FROM edges").unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap().map(|r| r.unwrap()).collect();
    println!("Edges in DB: {:?}", edges_list);
    
    let rels_list: Vec<(String, Option<String>, Option<String>)> = db.conn.prepare("SELECT name, source, kind FROM relationships").unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap().map(|r| r.unwrap()).collect();
    println!("Rels in DB: {:?}", rels_list);
    
    // In a real regression test we would dump to JSON and diff against a fixture
    // For now we just verify the basic edge exists
    let edges: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM edges WHERE target_symbol_id IN (SELECT id FROM symbols WHERE name = 'Auth')",
        [],
        |r| r.get(0)
    ).unwrap_or(0);
    
    assert!(edges > 0, "Should have created an edge for Auth");
}
