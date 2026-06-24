use rusqlite::{Connection, Result};
use std::time::Instant;

fn main() -> Result<()> {
    let conn = Connection::open(".codebroker.db")?;
    let start = Instant::now();
    let mut stmt = conn.prepare("
        SELECT
            files.path,
            symbols.name,
            symbols.kind,
            (SELECT COUNT(*) FROM edges WHERE target_symbol_id = symbols.id) as incoming,
            (SELECT COUNT(*) FROM edges WHERE source_symbol_id = symbols.id) as outgoing
        FROM symbols
        JOIN files ON symbols.file_id = files.id
    ")?;
    let mut rows = stmt.query([])?;
    let mut count = 0;
    while let Some(_) = rows.next()? {
        count += 1;
    }
    println!("Scanned {} symbols in {:?}", count, start.elapsed());
    Ok(())
}
