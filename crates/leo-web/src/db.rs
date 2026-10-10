use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result};
use rusqlite::Connection;

pub const FILE: &str = "leo.db";
pub const BEFORE: &str = ".before-database";
const VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS chats (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    mode TEXT NOT NULL,
    refs TEXT NOT NULL,
    messages TEXT NOT NULL,
    named INTEGER NOT NULL,
    about TEXT NOT NULL,
    memory TEXT,
    count INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS chats_by_time ON chats(updated_at);
CREATE VIRTUAL TABLE IF NOT EXISTS chats_text USING fts5(id UNINDEXED, title, body, tokenize = 'unicode61 remove_diacritics 2');
CREATE TABLE IF NOT EXISTS chat_docs (
    id TEXT PRIMARY KEY,
    chat TEXT NOT NULL,
    name TEXT NOT NULL,
    added_at TEXT NOT NULL,
    text TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS chat_docs_by_chat ON chat_docs(chat, added_at);
CREATE TABLE IF NOT EXISTS reviewed (
    key TEXT PRIMARY KEY,
    n INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS captions (
    key TEXT PRIMARY KEY,
    caption TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS vectors (
    note TEXT NOT NULL,
    at INTEGER NOT NULL,
    hash TEXT NOT NULL,
    v BLOB NOT NULL,
    PRIMARY KEY (note, at)
);
CREATE TABLE IF NOT EXISTS graph_notes (
    id TEXT PRIMARY KEY,
    data TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS graph_links (
    n INTEGER PRIMARY KEY,
    data TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS graph_meta (
    key TEXT PRIMARY KEY,
    data TEXT NOT NULL
);
";

pub struct Db {
    path: PathBuf,
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not make {}", parent.display()))?;
        }
        let conn =
            Connection::open(path).with_context(|| format!("could not open {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", VERSION)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(Db {
            path: path.to_path_buf(),
            conn: Mutex::new(conn),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn with<R>(&self, work: impl FnOnce(&mut Connection) -> rusqlite::Result<R>) -> Result<R> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        Ok(work(&mut conn)?)
    }

    pub fn bytes(&self) -> u64 {
        ["", "-wal", "-shm"]
            .iter()
            .filter_map(|end| {
                let mut name = self.path.as_os_str().to_owned();
                name.push(end);
                std::fs::metadata(PathBuf::from(name)).ok()
            })
            .map(|m| m.len())
            .sum()
    }

    pub fn tidy(&self) -> Result<()> {
        self.with(|c| c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;"))
    }
}

static OPEN: OnceLock<Mutex<HashMap<PathBuf, Arc<Db>>>> = OnceLock::new();

pub fn at(path: &Path) -> Arc<Db> {
    let key = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let all = OPEN.get_or_init(Default::default);
    let mut held = all.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(db) = held.get(&key) {
        if db.path.exists() {
            return Arc::clone(db);
        }
    }
    let db =
        Arc::new(Db::open(&key).unwrap_or_else(|e| {
            panic!("leo could not open its database at {}: {e}", key.display())
        }));
    held.insert(key, Arc::clone(&db));
    db
}

pub fn beside(file: &Path) -> Arc<Db> {
    at(&file.parent().unwrap_or(Path::new(".")).join(FILE))
}

pub fn for_chats(chats_dir: &Path) -> Arc<Db> {
    let data = if chats_dir.file_name().is_some_and(|n| n == "chats") {
        chats_dir.parent().unwrap_or(chats_dir)
    } else {
        chats_dir
    };
    at(&data.join(FILE))
}

pub fn put_aside(path: &Path) {
    let mut name = path.as_os_str().to_owned();
    name.push(BEFORE);
    let _ = std::fs::rename(path, PathBuf::from(name));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_database_is_made_once_with_full_text_search_and_shared_by_everyone_who_asks() {
        let dir = tempfile::tempdir().unwrap();
        let one = at(&dir.path().join(FILE));
        let two = beside(&dir.path().join("graph.json"));
        assert!(Arc::ptr_eq(&one, &two));
        let three = for_chats(&dir.path().join("chats"));
        assert!(
            Arc::ptr_eq(&one, &three),
            "the chats folder's database is the one beside it"
        );
        one.with(|c| {
            c.execute("INSERT INTO chats_text (id, title, body) VALUES ('a', 'Heaps', 'a min-heap keeps the smallest on top')", [])?;
            let found: String = c.query_row("SELECT id FROM chats_text WHERE chats_text MATCH 'smallest'", [], |r| r.get(0))?;
            assert_eq!(found, "a");
            Ok(())
        })
        .unwrap();
        assert!(one.bytes() > 0);
        let version: i64 = one
            .with(|c| c.query_row("PRAGMA user_version", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(version, VERSION);
    }
}

#[cfg(test)]
mod moving_in {
    use crate::{captions, chat_files, chats, graph, review, vectors};

    #[test]
    fn files_from_before_the_database_are_brought_in_once_and_kept_aside() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();
        let chats_dir = data.join("chats");
        std::fs::create_dir_all(chats_dir.join("chat-move-0001.files")).unwrap();
        std::fs::write(
            chats_dir.join("chat-move-0001.json"),
            serde_json::json!({
                "id": "chat-move-0001",
                "title": "Heaps",
                "mode": "study",
                "messages": [{ "role": "user", "text": "what is a min-heap?" }],
                "memory": { "upto": 2, "hash": "h", "text": "Studying heaps." },
                "created_at": "2026-10-01T10:00:00Z",
                "updated_at": "2026-10-02T10:00:00Z"
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            chats_dir.join("chat-move-0001.files/0123456789abcdef0123456789abcdef.json"),
            serde_json::json!({ "id": "0123456789abcdef0123456789abcdef", "name": "w.pdf", "added_at": "2026-10-01T10:00:00Z", "text": "Week 3: heaps" }).to_string(),
        )
        .unwrap();
        std::fs::write(
            chats_dir.join("review.json"),
            serde_json::json!({ "reviewed": ["chat-move-0001:0000000000000001"] }).to_string(),
        )
        .unwrap();
        std::fs::write(data.join("captions.json"), r#"{"k": "a heap drawing"}"#).unwrap();
        std::fs::write(
            data.join("graph.json"),
            serde_json::json!({ "notes": { "n1": { "hash": "h", "summary": "heaps", "concepts": ["heap"] } }, "links": [], "built_at": "2026-10-01" }).to_string(),
        )
        .unwrap();
        std::fs::write(
            data.join("meaning.json"),
            r#"{"notes": {"n1": {"hash": "h", "pieces": [[0, "AACAPwAAAAA="]]}}}"#,
        )
        .unwrap();

        let listed = chats::list(&chats_dir);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].title, "Heaps");
        let chat = chats::load(&chats_dir, "chat-move-0001").unwrap();
        assert_eq!(chat.memory.unwrap().text, "Studying heaps.");
        assert_eq!(
            chat_files::texts(
                &chats_dir,
                "chat-move-0001",
                &["0123456789abcdef0123456789abcdef".into()]
            ),
            [("w.pdf".to_string(), "Week 3: heaps".to_string())]
        );
        assert_eq!(review::count(&chats_dir), 1);
        assert_eq!(
            chats::search(&chats_dir, "min heap")[0].id,
            "chat-move-0001"
        );
        assert!(
            !chats_dir.exists() && data.join("chats.before-database").is_dir(),
            "the old folder is kept aside"
        );

        let seen = captions::Captions::for_notes(&data.join("notes"));
        assert_eq!(seen.get("k").as_deref(), Some("a heap drawing"));
        assert!(data.join("captions.json.before-database").is_file());

        let graphs = graph::Graphs::for_notes(&data.join("notes"), None);
        let cache = graphs.load();
        assert_eq!(cache.notes["n1"].summary, "heaps");
        assert_eq!(cache.built_at.as_deref(), Some("2026-10-01"));
        assert!(data.join("graph.json.before-database").is_file());

        let index = vectors::Vectors::for_notes(&data.join("notes"));
        assert_eq!(index.len(), 1);
        assert!(data.join("meaning.json.before-database").is_file());

        assert!(
            data.join(super::FILE).is_file(),
            "everything lives in one database"
        );
        assert_eq!(
            chats::list(&chats_dir).len(),
            1,
            "a second look reads the database, not the files"
        );
    }
}
