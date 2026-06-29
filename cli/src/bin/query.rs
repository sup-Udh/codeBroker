use rusqlite::{Connection, Result};

fn main() -> Result<()> {
    let conn = Connection::open("/home/labuser/Downloads/link-up/.codebroker/codebroker.db")?;

    let mut stmt = conn.prepare("SELECT file_id, name, source FROM imports LIMIT 20;")?;
    let rows = stmt.query_map([], |row| {
        let f: i64 = row.get(0)?;
        let n: String = row.get(1)?;
        let s: Option<String> = row.get(2)?;
        Ok((f, n, s))
    })?;

    println!("--- Imports ---");
    for row in rows {
        if let Ok((f, n, s)) = row {
            println!("file_id: {}, name: {}, source: {:?}", f, n, s);
        }
    }

    let mut stmt = conn.prepare("SELECT source, name, state, evidence FROM relationships WHERE kind = 'method_call' AND state != 'RepositorySymbol' AND state != 'Builtin' LIMIT 20;")?;
    let rows = stmt.query_map([], |row| {
        let s: Option<String> = row.get(0)?;
        let n: String = row.get(1)?;
        let state: String = row.get(2)?;
        let ev: Option<String> = row.get(3)?;
        Ok((s, n, state, ev))
    })?;

    println!("--- Unresolved Method Calls ---");
    for row in rows {
        if let Ok((s, n, state, ev)) = row {
            println!("receiver: {:?}, method: {}, state: {}, evidence: {:?}", s, n, state, ev);
        }
    }

    Ok(())
}
