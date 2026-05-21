use std::error::Error;
use std::fmt;
use std::path::PathBuf;

use rusqlite::Connection;

const CREATE_ENTRIES_TABLE: &str = "\
    CREATE TABLE IF NOT EXISTS entries (
        id INTEGER PRIMARY KEY,
        text TEXT NOT NULL,
        created_at TEXT NOT NULL
    )";

#[allow(dead_code)]
pub(crate) const ENTRIES_CREATION_ORDER_SQL: &str = "created_at ASC, id ASC";

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    id: i64,
    text: String,
    created_at: String,
}

#[allow(dead_code)]
impl Entry {
    pub fn new(id: i64, text: impl Into<String>, created_at: impl Into<String>) -> Self {
        Self {
            id,
            text: text.into(),
            created_at: created_at.into(),
        }
    }

    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn created_at(&self) -> &str {
        &self.created_at
    }
}

#[derive(Clone)]
pub struct StoreHandle {
    database_path: PathBuf,
}

impl StoreHandle {
    pub fn new(database_path: impl Into<PathBuf>) -> Self {
        Self {
            database_path: database_path.into(),
        }
    }

    pub fn initialize(&self) -> Result<(), StoreError> {
        let connection = self.open_connection()?;
        connection.execute(CREATE_ENTRIES_TABLE, [])?;

        Ok(())
    }

    fn open_connection(&self) -> Result<Connection, StoreError> {
        Ok(Connection::open(&self.database_path)?)
    }

    #[cfg(test)]
    pub fn database_path(&self) -> &std::path::Path {
        &self.database_path
    }
}

#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "sqlite store error: {error}"),
        }
    }
}

impl Error for StoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::Connection;

    use super::*;

    #[test]
    fn initialize_creates_entries_table() {
        let database = TestDatabase::new("creates_entries_table");
        let store = StoreHandle::new(database.path());

        store.initialize().expect("store should initialize");

        let connection = Connection::open(database.path()).expect("database should open");
        let columns = table_columns(&connection, "entries");

        assert_eq!(columns, ["id", "text", "created_at"]);
    }

    #[test]
    fn initialize_is_idempotent() {
        let database = TestDatabase::new("idempotent");
        let store = StoreHandle::new(database.path());

        store
            .initialize()
            .expect("first initialization should succeed");
        store
            .initialize()
            .expect("second initialization should succeed");

        let connection = Connection::open(database.path()).expect("database should open");
        let table_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'entries'",
                [],
                |row| row.get(0),
            )
            .expect("table count should query");

        assert_eq!(table_count, 1);
    }

    #[test]
    fn initialize_does_not_create_schema_version_tables() {
        let database = TestDatabase::new("no_schema_version");
        let store = StoreHandle::new(database.path());

        store.initialize().expect("store should initialize");

        let connection = Connection::open(database.path()).expect("database should open");
        let table_names = table_names(&connection);

        assert_eq!(table_names, ["entries"]);
    }

    #[test]
    fn entry_exposes_values_without_public_fields() {
        let entry = Entry::new(7, "milk", "2026-05-21T20:24:01Z");

        assert_eq!(entry.id(), 7);
        assert_eq!(entry.text(), "milk");
        assert_eq!(entry.created_at(), "2026-05-21T20:24:01Z");
    }

    #[test]
    fn creation_order_sql_stabilizes_equal_timestamps_by_id() {
        let database = TestDatabase::new("creation_order");
        let store = StoreHandle::new(database.path());
        store.initialize().expect("store should initialize");
        let connection = Connection::open(database.path()).expect("database should open");

        insert_entry(&connection, "middle-a", "2026-05-21T20:24:01Z");
        insert_entry(&connection, "first", "2026-05-21T20:24:00Z");
        insert_entry(&connection, "middle-b", "2026-05-21T20:24:01Z");
        insert_entry(&connection, "last", "2026-05-21T20:24:02Z");

        let ordered_ids = ordered_entry_ids(&connection);
        let first_id = entry_id_by_text(&connection, "first");
        let middle_a_id = entry_id_by_text(&connection, "middle-a");
        let middle_b_id = entry_id_by_text(&connection, "middle-b");
        let last_id = entry_id_by_text(&connection, "last");

        assert_eq!(ordered_ids, [first_id, middle_a_id, middle_b_id, last_id]);
    }

    #[test]
    fn displayed_positions_are_one_based_creation_order_indexes() {
        let database = TestDatabase::new("position_mapping");
        let store = StoreHandle::new(database.path());
        store.initialize().expect("store should initialize");
        let connection = Connection::open(database.path()).expect("database should open");

        insert_entry(&connection, "first", "2026-05-21T20:24:00Z");
        insert_entry(&connection, "second", "2026-05-21T20:24:01Z");
        insert_entry(&connection, "third", "2026-05-21T20:24:01Z");

        let displayed_positions = ordered_entry_ids(&connection)
            .into_iter()
            .enumerate()
            .map(|(index, id)| (index + 1, id))
            .collect::<Vec<_>>();

        assert_eq!(
            displayed_positions,
            [
                (1, entry_id_by_text(&connection, "first")),
                (2, entry_id_by_text(&connection, "second")),
                (3, entry_id_by_text(&connection, "third")),
            ]
        );
    }

    fn insert_entry(connection: &Connection, text: &str, created_at: &str) {
        connection
            .execute(
                "INSERT INTO entries (text, created_at) VALUES (?1, ?2)",
                (text, created_at),
            )
            .expect("entry should insert");
    }

    fn ordered_entry_ids(connection: &Connection) -> Vec<i64> {
        let mut statement = connection
            .prepare(&format!(
                "SELECT id FROM entries ORDER BY {ENTRIES_CREATION_ORDER_SQL}"
            ))
            .expect("ordered ids statement should prepare");

        statement
            .query_map([], |row| row.get(0))
            .expect("ordered ids should query")
            .collect::<Result<Vec<i64>, _>>()
            .expect("ordered ids should collect")
    }

    fn entry_id_by_text(connection: &Connection, text: &str) -> i64 {
        connection
            .query_row("SELECT id FROM entries WHERE text = ?1", [text], |row| {
                row.get(0)
            })
            .expect("entry id should query by text")
    }

    fn table_columns(connection: &Connection, table_name: &str) -> Vec<String> {
        let mut statement = connection
            .prepare(&format!("PRAGMA table_info({table_name})"))
            .expect("table info statement should prepare");

        statement
            .query_map([], |row| row.get(1))
            .expect("table info should query")
            .collect::<Result<Vec<String>, _>>()
            .expect("table info rows should collect")
    }

    fn table_names(connection: &Connection) -> Vec<String> {
        let mut statement = connection
            .prepare(
                "SELECT name FROM sqlite_master \
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
                 ORDER BY name",
            )
            .expect("table name statement should prepare");

        statement
            .query_map([], |row| row.get(0))
            .expect("table names should query")
            .collect::<Result<Vec<String>, _>>()
            .expect("table names should collect")
    }

    struct TestDatabase {
        path: PathBuf,
    }

    impl TestDatabase {
        fn new(name: &str) -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "trmnl-whatsapp-list-{name}-{}-{timestamp}.db",
                std::process::id()
            ));

            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }
}
