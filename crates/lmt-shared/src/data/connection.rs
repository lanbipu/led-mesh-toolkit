use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::{LmtError, LmtResult};

pub type Db = std::sync::Arc<Mutex<Connection>>;

/// Tauri GUI 的 bundle identifier。OS 标准 data dir 拼上它,正好等于
/// Tauri 2 `app.path().app_data_dir()` 的解析路径。**任何漂移会让 GUI
/// 与 CLI 看到不同的 DB**——所以这个常量与 `src-tauri/tauri.conf.json`
/// 的 `identifier` 必须保持同步。
pub const APP_IDENTIFIER: &str = "com.lanbipu.lmt";

/// SQLite 文件名,GUI 与 CLI 共用。
pub const DB_FILENAME: &str = "lmt.sqlite";

/// 用户态默认 DB 路径,与 Tauri GUI 的 `app_data_dir/lmt.sqlite` 一致。
///
/// 平台对应(`dirs::data_dir()` 行为):
/// - macOS:`~/Library/Application Support/com.lanbipu.lmt/lmt.sqlite`
/// - Linux:`$XDG_DATA_HOME/com.lanbipu.lmt/lmt.sqlite`
///   (默认 `~/.local/share/com.lanbipu.lmt/lmt.sqlite`)
/// - Windows:`%APPDATA%\com.lanbipu.lmt\lmt.sqlite`
///
/// CLI 解析顺序应当是:`--db <path>` > `LMT_DB_PATH` env > `default_db_path()`。
pub fn default_db_path() -> LmtResult<PathBuf> {
    let base = dirs::data_dir()
        .ok_or_else(|| LmtError::Other("OS data dir not resolvable on this platform".into()))?;
    Ok(base.join(APP_IDENTIFIER).join(DB_FILENAME))
}

/// 给跨进程并发(Tauri GUI + 同一台机上的 lmt-cli 共用同一份 lmt.sqlite)
/// 留出空间:
/// - `busy_timeout=5000`:遇到锁竞争最多等 5 秒,而不是立即 SQLITE_BUSY。
///   **必须在 `journal_mode=WAL` 之前设置**——否则 WAL 切换本身在另一个
///   进程持锁时也会立刻失败,白白浪费了 timeout。
/// - `journal_mode=WAL`:读写不互锁,GUI 在前台查 runs 时 CLI 也能写入。
/// - `foreign_keys=ON`:保留原有外键校验。
///
/// 这些 PRAGMA 写在 `open` 而非 `open_in_memory`,因为 in-memory 连接没有
/// 文件锁与单独的 WAL 日志,WAL 模式在那里没意义且会报错。
pub fn open(path: &Path) -> rusqlite::Result<Db> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA busy_timeout = 5000;\
         PRAGMA journal_mode = WAL;\
         PRAGMA foreign_keys = ON;",
    )?;
    Ok(std::sync::Arc::new(Mutex::new(conn)))
}

pub fn open_in_memory() -> rusqlite::Result<Db> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(std::sync::Arc::new(Mutex::new(conn)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn open_sets_pragmas_for_concurrent_access() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.sqlite");
        let db = open(&db_path).unwrap();
        let conn = db.lock().unwrap();

        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            journal_mode.to_lowercase(),
            "wal",
            "journal_mode must be WAL for cross-process concurrency"
        );

        let busy_timeout: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        assert_eq!(busy_timeout, 5000, "busy_timeout must be 5000 ms");

        let foreign_keys: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(foreign_keys, 1, "foreign_keys must stay ON");
    }

    #[test]
    fn default_db_path_ends_with_identifier_and_filename() {
        // 不断言整段路径(各 OS 不同),只校验末两段一致——这两段是 CLI
        // 与 GUI 共用同一文件的关键。
        let p = default_db_path().expect("data dir should resolve on test host");
        assert_eq!(
            p.file_name().and_then(|s| s.to_str()),
            Some(DB_FILENAME),
            "filename mismatch in {p:?}"
        );
        let parent_name = p
            .parent()
            .and_then(|d| d.file_name())
            .and_then(|s| s.to_str());
        assert_eq!(
            parent_name,
            Some(APP_IDENTIFIER),
            "parent dir mismatch in {p:?}"
        );
    }

    #[test]
    fn open_in_memory_keeps_foreign_keys_without_wal() {
        // In-memory connections have no file lock and cannot use WAL — make sure
        // we don't accidentally try to flip them, which would error.
        let db = open_in_memory().unwrap();
        let conn = db.lock().unwrap();

        let foreign_keys: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(foreign_keys, 1);

        // journal_mode on in-mem is "memory", not "wal"; assert it's NOT WAL
        // so a future careless edit can't silently regress.
        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_ne!(
            journal_mode.to_lowercase(),
            "wal",
            "in-memory connection cannot meaningfully be WAL"
        );
    }
}
