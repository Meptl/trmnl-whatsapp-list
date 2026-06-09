use std::error::Error;
use std::fmt;
use std::path::PathBuf;

use rusqlite::{Connection, OptionalExtension, Row, params};

const CREATE_ENTRIES_TABLE: &str = "\
    CREATE TABLE IF NOT EXISTS entries (
        id INTEGER PRIMARY KEY,
        text TEXT NOT NULL,
        created_at TEXT NOT NULL
    )";

const CREATE_AUTHORIZED_CHAT_SENDERS_TABLE: &str = "\
    CREATE TABLE IF NOT EXISTS authorized_chat_senders (
        provider TEXT NOT NULL,
        sender_id TEXT NOT NULL,
        PRIMARY KEY (provider, sender_id)
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

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RemoveEntryResult {
    Removed(Entry),
    NotFound,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClearEntriesResult {
    deleted_count: usize,
}

#[allow(dead_code)]
impl ClearEntriesResult {
    pub fn new(deleted_count: usize) -> Self {
        Self { deleted_count }
    }

    pub fn deleted_count(&self) -> usize {
        self.deleted_count
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
        connection.execute(CREATE_AUTHORIZED_CHAT_SENDERS_TABLE, [])?;

        Ok(())
    }

    /// Stores text exactly as supplied; command execution owns normalization and validation.
    #[allow(dead_code)]
    pub fn add_entry(&self, text: impl Into<String>) -> Result<Entry, StoreError> {
        let connection = self.open_connection()?;
        connection.execute(
            "INSERT INTO entries (text, created_at) \
             VALUES (?1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![text.into()],
        )?;

        let entry_id = connection.last_insert_rowid();
        let entry = connection.query_row(
            "SELECT id, text, created_at FROM entries WHERE id = ?1",
            [entry_id],
            entry_from_row,
        )?;

        Ok(entry)
    }

    #[allow(dead_code)]
    pub fn list_entries(&self) -> Result<Vec<Entry>, StoreError> {
        let connection = self.open_connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT id, text, created_at FROM entries ORDER BY {ENTRIES_CREATION_ORDER_SQL}"
        ))?;

        let entries = statement
            .query_map([], entry_from_row)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(entries)
    }

    #[allow(dead_code)]
    pub fn remove_entry_by_text(&self, text: &str) -> Result<RemoveEntryResult, StoreError> {
        let connection = self.open_connection()?;
        let exact_entry = find_exact_entry_by_text(&connection, text)?;
        let entry = match exact_entry {
            Some(entry) => Some(entry),
            None => find_case_insensitive_entry_by_text(&connection, text)?,
        };

        match entry {
            Some(entry) => {
                let deleted_count =
                    connection.execute("DELETE FROM entries WHERE id = ?1", [entry.id()])?;

                if deleted_count == 0 {
                    Ok(RemoveEntryResult::NotFound)
                } else {
                    Ok(RemoveEntryResult::Removed(entry))
                }
            }
            None => Ok(RemoveEntryResult::NotFound),
        }
    }

    #[allow(dead_code)]
    pub fn remove_entry_by_position(
        &self,
        position: usize,
    ) -> Result<RemoveEntryResult, StoreError> {
        let Some(offset) = position.checked_sub(1) else {
            return Ok(RemoveEntryResult::NotFound);
        };
        let Ok(offset) = i64::try_from(offset) else {
            return Ok(RemoveEntryResult::NotFound);
        };

        let connection = self.open_connection()?;
        let entry = find_entry_by_position_offset(&connection, offset)?;

        match entry {
            Some(entry) => {
                let deleted_count =
                    connection.execute("DELETE FROM entries WHERE id = ?1", [entry.id()])?;

                if deleted_count == 0 {
                    Ok(RemoveEntryResult::NotFound)
                } else {
                    Ok(RemoveEntryResult::Removed(entry))
                }
            }
            None => Ok(RemoveEntryResult::NotFound),
        }
    }

    #[allow(dead_code)]
    pub fn clear_entries(&self) -> Result<ClearEntriesResult, StoreError> {
        let connection = self.open_connection()?;
        let deleted_count = connection.execute("DELETE FROM entries", [])?;

        Ok(ClearEntriesResult::new(deleted_count))
    }

    pub fn is_chat_sender_authorized(
        &self,
        provider: &str,
        sender_id: &str,
    ) -> Result<bool, StoreError> {
        let connection = self.open_connection()?;
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM authorized_chat_senders WHERE provider = ?1 AND sender_id = ?2",
            params![provider, sender_id],
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }

    pub fn authorize_chat_sender(&self, provider: &str, sender_id: &str) -> Result<(), StoreError> {
        let connection = self.open_connection()?;
        connection.execute(
            "INSERT OR IGNORE INTO authorized_chat_senders (provider, sender_id) VALUES (?1, ?2)",
            params![provider, sender_id],
        )?;

        Ok(())
    }

    pub fn deauthorize_chat_sender(
        &self,
        provider: &str,
        sender_id: &str,
    ) -> Result<(), StoreError> {
        let connection = self.open_connection()?;
        connection.execute(
            "DELETE FROM authorized_chat_senders WHERE provider = ?1 AND sender_id = ?2",
            params![provider, sender_id],
        )?;

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

#[allow(dead_code)]
fn entry_from_row(row: &Row<'_>) -> rusqlite::Result<Entry> {
    Ok(Entry::new(
        row.get::<_, i64>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, String>(2)?,
    ))
}

fn find_exact_entry_by_text(
    connection: &Connection,
    text: &str,
) -> Result<Option<Entry>, StoreError> {
    Ok(connection
        .query_row(
            &format!(
                "SELECT id, text, created_at FROM entries \
                 WHERE text = ?1 \
                 ORDER BY {ENTRIES_CREATION_ORDER_SQL} \
                 LIMIT 1"
            ),
            [text],
            entry_from_row,
        )
        .optional()?)
}

fn find_case_insensitive_entry_by_text(
    connection: &Connection,
    text: &str,
) -> Result<Option<Entry>, StoreError> {
    Ok(connection
        .query_row(
            &format!(
                "SELECT id, text, created_at FROM entries \
                 WHERE text = ?1 COLLATE NOCASE \
                 ORDER BY {ENTRIES_CREATION_ORDER_SQL} \
                 LIMIT 1"
            ),
            [text],
            entry_from_row,
        )
        .optional()?)
}

fn find_entry_by_position_offset(
    connection: &Connection,
    offset: i64,
) -> Result<Option<Entry>, StoreError> {
    Ok(connection
        .query_row(
            &format!(
                "SELECT id, text, created_at FROM entries \
                 ORDER BY {ENTRIES_CREATION_ORDER_SQL} \
                 LIMIT 1 OFFSET ?1"
            ),
            [offset],
            entry_from_row,
        )
        .optional()?)
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
    fn initialize_creates_entries_table_from_empty_file() {
        let database = TestDatabase::new("creates_entries_table_from_empty_file");
        fs::File::create(database.path()).expect("empty database file should create");
        assert_eq!(
            fs::metadata(database.path())
                .expect("empty database file metadata should read")
                .len(),
            0
        );
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

        assert_eq!(table_names, ["authorized_chat_senders", "entries"]);
    }

    #[test]
    fn initialize_creates_authorized_chat_senders_table_without_metadata() {
        let database = TestDatabase::new("creates_authorized_chat_senders_table");
        let store = StoreHandle::new(database.path());

        store.initialize().expect("store should initialize");

        let connection = Connection::open(database.path()).expect("database should open");
        let columns = table_columns(&connection, "authorized_chat_senders");

        assert_eq!(columns, ["provider", "sender_id"]);
    }

    #[test]
    fn chat_sender_authorization_is_idempotent_and_provider_scoped() {
        let database = TestDatabase::new("chat_auth_provider_scoped");
        let store = StoreHandle::new(database.path());
        store.initialize().expect("store should initialize");

        assert!(
            !store
                .is_chat_sender_authorized("whatsapp", "sender-1")
                .expect("auth lookup should succeed")
        );

        store
            .authorize_chat_sender("whatsapp", "sender-1")
            .expect("sender should authorize");
        store
            .authorize_chat_sender("whatsapp", "sender-1")
            .expect("repeated authorization should succeed");

        assert!(
            store
                .is_chat_sender_authorized("whatsapp", "sender-1")
                .expect("auth lookup should succeed")
        );
        assert!(
            !store
                .is_chat_sender_authorized("telegram", "sender-1")
                .expect("auth lookup should succeed")
        );

        store
            .deauthorize_chat_sender("whatsapp", "sender-1")
            .expect("sender should deauthorize");

        assert!(
            !store
                .is_chat_sender_authorized("whatsapp", "sender-1")
                .expect("auth lookup should succeed")
        );
    }

    #[test]
    fn entry_exposes_values_without_public_fields() {
        let entry = Entry::new(7, "milk", "2026-05-21T20:24:01Z");

        assert_eq!(entry.id(), 7);
        assert_eq!(entry.text(), "milk");
        assert_eq!(entry.created_at(), "2026-05-21T20:24:01Z");
    }

    #[test]
    fn add_entry_stores_exact_shared_list_text_including_empty_and_whitespace() {
        let database = TestDatabase::new("add_exact_text");
        let store = StoreHandle::new(database.path());
        store.initialize().expect("store should initialize");

        let empty_entry = store.add_entry("").expect("empty entry should insert");
        let whitespace_entry = store
            .add_entry("  milk\n")
            .expect("whitespace entry should insert");

        assert_eq!(empty_entry.text(), "");
        assert_eq!(whitespace_entry.text(), "  milk\n");
        assert_ne!(empty_entry.id(), whitespace_entry.id());
        assert!(empty_entry.created_at().ends_with('Z'));
        assert!(whitespace_entry.created_at().ends_with('Z'));
    }

    #[test]
    fn list_entries_returns_the_shared_list_in_creation_order() {
        let database = TestDatabase::new("list_shared_creation_order");
        let store = StoreHandle::new(database.path());
        store.initialize().expect("store should initialize");

        let first = store.add_entry("first").expect("first entry should insert");
        let second = store
            .add_entry("second")
            .expect("second entry should insert");
        let third = store.add_entry("third").expect("third entry should insert");

        let entries = store.list_entries().expect("entries should list");

        assert_eq!(entries, [first, second, third]);
    }

    #[test]
    fn list_entries_returns_empty_shared_list() {
        let database = TestDatabase::new("list_empty_shared");
        let store = StoreHandle::new(database.path());
        store.initialize().expect("store should initialize");

        let entries = store.list_entries().expect("entries should list");

        assert!(entries.is_empty());
    }

    #[test]
    fn remove_entry_by_text_prefers_exact_match_over_case_insensitive_match() {
        let database = TestDatabase::new("remove_prefers_exact");
        let store = StoreHandle::new(database.path());
        store.initialize().expect("store should initialize");
        let connection = Connection::open(database.path()).expect("database should open");

        insert_entry(&connection, "milk", "2026-05-21T20:24:00Z");
        insert_entry(&connection, "Milk", "2026-05-21T20:24:01Z");
        let exact_id = entry_id_by_text(&connection, "Milk");

        let result = store
            .remove_entry_by_text("Milk")
            .expect("entry should remove");
        let entries = store.list_entries().expect("entries should list");

        assert_eq!(
            result,
            RemoveEntryResult::Removed(Entry::new(exact_id, "Milk", "2026-05-21T20:24:01Z"))
        );
        assert_eq!(entry_texts(&entries), ["milk"]);
    }

    #[test]
    fn remove_entry_by_text_falls_back_to_case_insensitive_match() {
        let database = TestDatabase::new("remove_case_insensitive");
        let store = StoreHandle::new(database.path());
        store.initialize().expect("store should initialize");
        let connection = Connection::open(database.path()).expect("database should open");

        insert_entry(&connection, "Milk", "2026-05-21T20:24:00Z");
        let entry_id = entry_id_by_text(&connection, "Milk");

        let result = store
            .remove_entry_by_text("milk")
            .expect("entry should remove");
        let entries = store.list_entries().expect("entries should list");

        assert_eq!(
            result,
            RemoveEntryResult::Removed(Entry::new(entry_id, "Milk", "2026-05-21T20:24:00Z"))
        );
        assert!(entries.is_empty());
    }

    #[test]
    fn remove_entry_by_text_removes_earliest_displayed_duplicate() {
        let database = TestDatabase::new("remove_duplicate_order");
        let store = StoreHandle::new(database.path());
        store.initialize().expect("store should initialize");
        let connection = Connection::open(database.path()).expect("database should open");

        insert_entry(&connection, "milk", "2026-05-21T20:24:01Z");
        insert_entry(&connection, "first", "2026-05-21T20:24:00Z");
        insert_entry(&connection, "milk", "2026-05-21T20:24:02Z");

        let duplicate_ids = entry_ids_by_text(&connection, "milk");
        let result = store
            .remove_entry_by_text("milk")
            .expect("entry should remove");
        let entries = store.list_entries().expect("entries should list");

        assert_eq!(
            result,
            RemoveEntryResult::Removed(Entry::new(
                duplicate_ids[0],
                "milk",
                "2026-05-21T20:24:01Z"
            ))
        );
        assert_eq!(entry_texts(&entries), ["first", "milk"]);
        assert_eq!(entries[1].id(), duplicate_ids[1]);
    }

    #[test]
    fn remove_entry_by_text_returns_not_found_without_changing_list() {
        let database = TestDatabase::new("remove_not_found");
        let store = StoreHandle::new(database.path());
        store.initialize().expect("store should initialize");
        let connection = Connection::open(database.path()).expect("database should open");

        insert_entry(&connection, "milk", "2026-05-21T20:24:00Z");
        insert_entry(&connection, "eggs", "2026-05-21T20:24:01Z");
        let before = store.list_entries().expect("entries should list");

        let result = store
            .remove_entry_by_text("bread")
            .expect("not found should not error");
        let after = store.list_entries().expect("entries should list");

        assert_eq!(result, RemoveEntryResult::NotFound);
        assert_eq!(after, before);
    }

    #[test]
    fn remove_entry_by_position_one_removes_first_displayed_entry() {
        let database = TestDatabase::new("remove_position_first");
        let store = StoreHandle::new(database.path());
        store.initialize().expect("store should initialize");
        let connection = Connection::open(database.path()).expect("database should open");

        insert_entry(&connection, "second", "2026-05-21T20:24:01Z");
        insert_entry(&connection, "first", "2026-05-21T20:24:00Z");
        insert_entry(&connection, "third", "2026-05-21T20:24:02Z");
        let first_id = entry_id_by_text(&connection, "first");

        let result = store
            .remove_entry_by_position(1)
            .expect("entry should remove");
        let entries = store.list_entries().expect("entries should list");

        assert_eq!(
            result,
            RemoveEntryResult::Removed(Entry::new(first_id, "first", "2026-05-21T20:24:00Z"))
        );
        assert_eq!(entry_texts(&entries), ["second", "third"]);
    }

    #[test]
    fn remove_entry_by_position_two_removes_second_displayed_entry() {
        let database = TestDatabase::new("remove_position_second");
        let store = StoreHandle::new(database.path());
        store.initialize().expect("store should initialize");
        let connection = Connection::open(database.path()).expect("database should open");

        insert_entry(&connection, "first", "2026-05-21T20:24:00Z");
        insert_entry(&connection, "second", "2026-05-21T20:24:01Z");
        insert_entry(&connection, "third", "2026-05-21T20:24:02Z");
        let second_id = entry_id_by_text(&connection, "second");

        let result = store
            .remove_entry_by_position(2)
            .expect("entry should remove");
        let entries = store.list_entries().expect("entries should list");

        assert_eq!(
            result,
            RemoveEntryResult::Removed(Entry::new(second_id, "second", "2026-05-21T20:24:01Z"))
        );
        assert_eq!(entry_texts(&entries), ["first", "third"]);
    }

    #[test]
    fn remove_entry_by_position_out_of_range_returns_not_found_without_changing_list() {
        let database = TestDatabase::new("remove_position_out_of_range");
        let store = StoreHandle::new(database.path());
        store.initialize().expect("store should initialize");
        let connection = Connection::open(database.path()).expect("database should open");

        insert_entry(&connection, "first", "2026-05-21T20:24:00Z");
        insert_entry(&connection, "second", "2026-05-21T20:24:01Z");
        let before = store.list_entries().expect("entries should list");

        let result = store
            .remove_entry_by_position(3)
            .expect("not found should not error");
        let after = store.list_entries().expect("entries should list");

        assert_eq!(result, RemoveEntryResult::NotFound);
        assert_eq!(after, before);
    }

    #[test]
    fn remove_entry_by_position_empty_list_returns_not_found() {
        let database = TestDatabase::new("remove_position_empty");
        let store = StoreHandle::new(database.path());
        store.initialize().expect("store should initialize");

        let result = store
            .remove_entry_by_position(1)
            .expect("not found should not error");
        let entries = store.list_entries().expect("entries should list");

        assert_eq!(result, RemoveEntryResult::NotFound);
        assert!(entries.is_empty());
    }

    #[test]
    fn remove_entry_by_position_zero_returns_not_found_without_changing_list() {
        let database = TestDatabase::new("remove_position_zero");
        let store = StoreHandle::new(database.path());
        store.initialize().expect("store should initialize");
        let connection = Connection::open(database.path()).expect("database should open");

        insert_entry(&connection, "first", "2026-05-21T20:24:00Z");
        let before = store.list_entries().expect("entries should list");

        let result = store
            .remove_entry_by_position(0)
            .expect("not found should not error");
        let after = store.list_entries().expect("entries should list");

        assert_eq!(result, RemoveEntryResult::NotFound);
        assert_eq!(after, before);
    }

    #[test]
    fn clear_entries_removes_multiple_entries_and_returns_removed_count() {
        let database = TestDatabase::new("clear_multiple");
        let store = StoreHandle::new(database.path());
        store.initialize().expect("store should initialize");
        let connection = Connection::open(database.path()).expect("database should open");

        insert_entry(&connection, "first", "2026-05-21T20:24:00Z");
        insert_entry(&connection, "second", "2026-05-21T20:24:01Z");
        insert_entry(&connection, "third", "2026-05-21T20:24:02Z");

        let result = store.clear_entries().expect("entries should clear");
        let entries = store.list_entries().expect("entries should list");

        assert_eq!(result.deleted_count(), 3);
        assert!(entries.is_empty());
    }

    #[test]
    fn clear_entries_on_empty_database_succeeds_and_returns_zero() {
        let database = TestDatabase::new("clear_empty");
        let store = StoreHandle::new(database.path());
        store.initialize().expect("store should initialize");

        let result = store.clear_entries().expect("empty list should clear");
        let entries = store.list_entries().expect("entries should list");

        assert_eq!(result.deleted_count(), 0);
        assert!(entries.is_empty());
    }

    #[test]
    fn clear_entries_is_idempotent_when_called_twice() {
        let database = TestDatabase::new("clear_idempotent");
        let store = StoreHandle::new(database.path());
        store.initialize().expect("store should initialize");
        let connection = Connection::open(database.path()).expect("database should open");

        insert_entry(&connection, "first", "2026-05-21T20:24:00Z");
        insert_entry(&connection, "second", "2026-05-21T20:24:01Z");

        let first_result = store.clear_entries().expect("entries should clear");
        let second_result = store
            .clear_entries()
            .expect("empty list should clear again");
        let entries = store.list_entries().expect("entries should list");

        assert_eq!(first_result.deleted_count(), 2);
        assert_eq!(second_result.deleted_count(), 0);
        assert!(entries.is_empty());
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

    fn entry_ids_by_text(connection: &Connection, text: &str) -> Vec<i64> {
        let mut statement = connection
            .prepare(&format!(
                "SELECT id FROM entries WHERE text = ?1 ORDER BY {ENTRIES_CREATION_ORDER_SQL}"
            ))
            .expect("entry ids by text statement should prepare");

        statement
            .query_map([text], |row| row.get(0))
            .expect("entry ids should query by text")
            .collect::<Result<Vec<i64>, _>>()
            .expect("entry ids should collect")
    }

    fn entry_texts(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(Entry::text).collect()
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
