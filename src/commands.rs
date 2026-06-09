use std::error::Error;
use std::fmt;

use crate::store::{RemoveEntryResult, StoreError, StoreHandle};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    ToggleEntry(String),
    ListEntries,
    ClearEntries,
    Login(Option<String>),
    Logout,
    Ignore,
}

pub fn execute_command(
    store: &StoreHandle,
    command: Command,
) -> Result<String, CommandExecutionError> {
    match command {
        Command::ToggleEntry(text) => toggle_entry(store, text),
        Command::ListEntries => list_entries(store),
        Command::ClearEntries => clear_entries(store),
        Command::Login(_) | Command::Logout | Command::Ignore => {
            Ok("Nothing to do. Send an item name to update the list.".to_owned())
        }
    }
}

pub fn parse_command(message: &str) -> Command {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return Command::Ignore;
    }

    if trimmed.eq_ignore_ascii_case("/list") {
        return Command::ListEntries;
    }

    if trimmed.eq_ignore_ascii_case("/clear") {
        return Command::ClearEntries;
    }

    if trimmed.eq_ignore_ascii_case("/logout") {
        return Command::Logout;
    }

    let parts = trimmed.split_whitespace().collect::<Vec<_>>();
    if parts
        .first()
        .is_some_and(|command| command.eq_ignore_ascii_case("/login"))
    {
        return match parts.as_slice() {
            [_, key] => Command::Login(Some((*key).to_owned())),
            _ => Command::Login(None),
        };
    }

    Command::ToggleEntry(trimmed.to_owned())
}

fn toggle_entry(store: &StoreHandle, text: String) -> Result<String, CommandExecutionError> {
    let entry_text = text.trim();
    if entry_text.is_empty() {
        return Ok("Nothing to do. Send an item name to update the list.".to_owned());
    }

    match store.remove_entry_by_text(entry_text)? {
        RemoveEntryResult::Removed(entry) => Ok(format!("\"{}\" removed from list.", entry.text())),
        RemoveEntryResult::NotFound => {
            let entry = store.add_entry(entry_text)?;
            Ok(format!("\"{}\" added to list.", entry.text()))
        }
    }
}

fn list_entries(store: &StoreHandle) -> Result<String, CommandExecutionError> {
    let entries = store.list_entries()?;

    if entries.is_empty() {
        return Ok("The list is empty.".to_owned());
    }

    Ok(entries
        .iter()
        .enumerate()
        .map(|(index, entry)| format!("{}. {}", index + 1, entry.text()))
        .collect::<Vec<_>>()
        .join("\n"))
}

fn clear_entries(store: &StoreHandle) -> Result<String, CommandExecutionError> {
    let result = store.clear_entries()?;

    Ok(format!("Cleared {} entries.", result.deleted_count()))
}

#[derive(Debug)]
pub enum CommandExecutionError {
    Store(StoreError),
}

impl fmt::Display for CommandExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for CommandExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
        }
    }
}

impl From<StoreError> for CommandExecutionError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::store::StoreHandle;

    #[test]
    fn plain_text_toggles_trimmed_entry() {
        assert_eq!(
            parse_command("  Buy Milk Tomorrow  "),
            Command::ToggleEntry("Buy Milk Tomorrow".to_owned())
        );
    }

    #[test]
    fn empty_and_whitespace_only_messages_are_ignored() {
        assert_eq!(parse_command(""), Command::Ignore);
        assert_eq!(parse_command(" \n\t "), Command::Ignore);
    }

    #[test]
    fn former_command_words_are_item_text() {
        assert_eq!(
            parse_command("LIST"),
            Command::ToggleEntry("LIST".to_owned())
        );
        assert_eq!(
            parse_command("Clear"),
            Command::ToggleEntry("Clear".to_owned())
        );
        assert_eq!(
            parse_command("hElP"),
            Command::ToggleEntry("hElP".to_owned())
        );
        assert_eq!(
            parse_command("remove milk"),
            Command::ToggleEntry("remove milk".to_owned())
        );
    }

    #[test]
    fn slash_commands_are_case_insensitive() {
        assert_eq!(parse_command("/LIST"), Command::ListEntries);
        assert_eq!(parse_command(" /clear "), Command::ClearEntries);
        assert_eq!(parse_command(" /logout "), Command::Logout);
        assert_eq!(
            parse_command(" /LOGIN secret "),
            Command::Login(Some("secret".to_owned()))
        );
        assert_eq!(parse_command("/login"), Command::Login(None));
        assert_eq!(parse_command("/login one two"), Command::Login(None));
    }

    #[test]
    fn execute_new_text_adds_item() {
        let database = TestDatabase::new("execute_toggle_add");
        let store = initialized_store(&database);

        let reply =
            execute_command(&store, parse_command("milk")).expect("toggle command should execute");
        let entries = store.list_entries().expect("entries should list");

        assert_eq!(reply, "\"milk\" added to list.");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text(), "milk");
    }

    #[test]
    fn execute_existing_text_removes_item() {
        let database = TestDatabase::new("execute_toggle_remove");
        let store = initialized_store(&database);
        execute_command(&store, parse_command("milk")).expect("toggle add should execute");

        let reply =
            execute_command(&store, parse_command("MILK")).expect("toggle command should execute");
        let entries = store.list_entries().expect("entries should list");

        assert_eq!(reply, "\"milk\" removed from list.");
        assert!(entries.is_empty());
    }

    #[test]
    fn execute_list_returns_numbered_entries() {
        let database = TestDatabase::new("execute_list");
        let store = initialized_store(&database);
        execute_command(&store, parse_command("milk")).expect("first toggle should execute");
        execute_command(&store, parse_command("eggs")).expect("second toggle should execute");

        let reply =
            execute_command(&store, parse_command("/list")).expect("list command should execute");

        assert_eq!(reply, "1. milk\n2. eggs");
    }

    #[test]
    fn execute_empty_list_reply_is_clear() {
        let database = TestDatabase::new("execute_empty_list");
        let store = initialized_store(&database);

        let reply =
            execute_command(&store, parse_command("/list")).expect("list command should execute");

        assert_eq!(reply, "The list is empty.");
    }

    #[test]
    fn execute_clear_removes_entries() {
        let database = TestDatabase::new("execute_clear");
        let store = initialized_store(&database);
        execute_command(&store, parse_command("milk")).expect("first toggle should execute");
        execute_command(&store, parse_command("eggs")).expect("second toggle should execute");

        let reply =
            execute_command(&store, parse_command("/clear")).expect("clear command should execute");
        let entries = store.list_entries().expect("entries should list");

        assert_eq!(reply, "Cleared 2 entries.");
        assert!(entries.is_empty());
    }

    #[test]
    fn execute_ignore_returns_no_op_reply() {
        let database = TestDatabase::new("execute_ignore");
        let store = initialized_store(&database);

        let reply =
            execute_command(&store, Command::Ignore).expect("ignore command should execute");
        let entries = store.list_entries().expect("entries should list");

        assert_eq!(
            reply,
            "Nothing to do. Send an item name to update the list."
        );
        assert!(entries.is_empty());
    }

    fn initialized_store(database: &TestDatabase) -> StoreHandle {
        let store = StoreHandle::new(database.path());
        store.initialize().expect("store should initialize");
        store
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
                "trmnl-whatsapp-list-commands-{name}-{}-{timestamp}.db",
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
