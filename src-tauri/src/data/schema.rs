use rusqlite::Connection;

const MIGRATIONS: &[(&str, &str)] = &[
    (
        "001_recent_projects",
        r#"
        CREATE TABLE IF NOT EXISTS recent_projects (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            abs_path TEXT NOT NULL UNIQUE,
            display_name TEXT NOT NULL,
            last_opened_at TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_recent_projects_last_opened
            ON recent_projects(last_opened_at DESC);
        "#,
    ),
    (
        "002_reconstruction_runs",
        r#"
        CREATE TABLE IF NOT EXISTS reconstruction_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_path TEXT NOT NULL,
            screen_id TEXT NOT NULL,
            measurements_path TEXT NOT NULL,
            method TEXT NOT NULL,
            measured_count INTEGER NOT NULL,
            expected_count INTEGER NOT NULL,
            estimated_rms_mm REAL NOT NULL,
            estimated_p95_mm REAL NOT NULL,
            vertex_count INTEGER NOT NULL,
            output_obj_path TEXT,
            report_json_path TEXT NOT NULL,
            target TEXT,
            warnings_json TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_runs_project_screen
            ON reconstruction_runs(project_path, screen_id, created_at DESC);
        "#,
    ),
];

pub fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            name TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;
    for (name, sql) in MIGRATIONS {
        let already: i64 = conn.query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE name = ?1",
            [name],
            |r| r.get(0),
        )?;
        if already > 0 {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute("INSERT INTO schema_migrations(name) VALUES (?1)", [name])?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn migrate_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        migrate(&mut conn).unwrap(); // 跑第二次应该无副作用
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='recent_projects'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
