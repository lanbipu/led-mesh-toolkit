use crate::dto::RecentProject;
use crate::error::LmtResult;
use rusqlite::Connection;

/// Insert or update a recent project entry.
/// On conflict (same abs_path), updates display_name and last_opened_at.
/// Returns the full row after the upsert.
pub fn upsert(
    conn: &Connection,
    abs_path: &str,
    display_name: &str,
) -> LmtResult<RecentProject> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        r#"
        INSERT INTO recent_projects (abs_path, display_name, last_opened_at)
        VALUES (?1, ?2, ?3)
        ON CONFLICT(abs_path) DO UPDATE SET
            display_name    = excluded.display_name,
            last_opened_at  = excluded.last_opened_at
        "#,
        rusqlite::params![abs_path, display_name, now],
    )?;

    let row = conn.query_row(
        "SELECT id, abs_path, display_name, last_opened_at FROM recent_projects WHERE abs_path = ?1",
        rusqlite::params![abs_path],
        |r| {
            Ok(RecentProject {
                id: r.get(0)?,
                abs_path: r.get(1)?,
                display_name: r.get(2)?,
                last_opened_at: r.get(3)?,
            })
        },
    )?;

    Ok(row)
}

/// Return all recent projects ordered by last_opened_at descending (most recent first).
pub fn list(conn: &Connection) -> LmtResult<Vec<RecentProject>> {
    let mut stmt = conn.prepare(
        "SELECT id, abs_path, display_name, last_opened_at FROM recent_projects ORDER BY last_opened_at DESC",
    )?;

    let rows = stmt.query_map([], |r| {
        Ok(RecentProject {
            id: r.get(0)?,
            abs_path: r.get(1)?,
            display_name: r.get(2)?,
            last_opened_at: r.get(3)?,
        })
    })?;

    let mut projects = Vec::new();
    for row in rows {
        projects.push(row?);
    }
    Ok(projects)
}

/// Delete a recent project by id. No-op if the id does not exist.
pub fn delete(conn: &Connection, id: i64) -> LmtResult<()> {
    conn.execute(
        "DELETE FROM recent_projects WHERE id = ?1",
        rusqlite::params![id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::schema;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        schema::migrate(&mut conn).unwrap();
        conn
    }

    #[test]
    fn upsert_and_list() {
        let conn = setup();

        // Insert /a and /b
        let a1 = upsert(&conn, "/a", "Project A").unwrap();
        let b = upsert(&conn, "/b", "Project B").unwrap();

        // Both paths should have different ids
        assert_ne!(a1.id, b.id);
        assert_eq!(a1.abs_path, "/a");
        assert_eq!(b.abs_path, "/b");

        // Small delay to ensure distinct last_opened_at timestamps
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Upsert /a again — same id, updated timestamp
        let a2 = upsert(&conn, "/a", "Project A v2").unwrap();
        assert_eq!(a1.id, a2.id, "upsert should preserve the same row id");
        assert_eq!(a2.display_name, "Project A v2");
        // Timestamp must have advanced
        assert!(
            a2.last_opened_at > a1.last_opened_at,
            "last_opened_at should be updated: {} > {}",
            a2.last_opened_at,
            a1.last_opened_at
        );

        // list: 2 entries, /a first (most recently touched)
        let rows = list(&conn).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].abs_path, "/a");
        assert_eq!(rows[1].abs_path, "/b");
    }

    #[test]
    fn delete_by_id() {
        let conn = setup();

        let p = upsert(&conn, "/del", "Delete Me").unwrap();
        let before = list(&conn).unwrap();
        assert_eq!(before.len(), 1);

        delete(&conn, p.id).unwrap();
        let after = list(&conn).unwrap();
        assert_eq!(after.len(), 0);
    }

    #[test]
    fn delete_nonexistent_is_noop() {
        let conn = setup();
        // Should not error
        delete(&conn, 9999).unwrap();
    }
}
