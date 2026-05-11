use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

pub type Db = std::sync::Arc<Mutex<Connection>>;

pub fn open(path: &Path) -> rusqlite::Result<Db> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(std::sync::Arc::new(Mutex::new(conn)))
}

pub fn open_in_memory() -> rusqlite::Result<Db> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(std::sync::Arc::new(Mutex::new(conn)))
}
