use std::error::Error;
use std::fmt;

use crate::store::{RemoveEntryResult, StoreError, StoreHandle};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    AddEntry(String),
    ListEntries,
    RemoveEntry(RemoveTarget),
    ClearEntries,
    Help,
    Ignore,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RemoveTarget {
    Position(usize),
    Text(String),
}

pub fn execute_command(
    store: &StoreHandle,
    command: Command,
) -> Result<String, CommandExecutionError> {
    match command {
        Command::AddEntry(text) => add_entry(store, text),
        Command::ListEntries => list_entries(store),
        Command::RemoveEntry(target) => remove_entry(store, target),
        Command::ClearEntries => clear_entries(store),
        Command::Help => Ok(help_reply()),
        Command::Ignore => Ok("Nothing to do. Send help to see supported commands.".to_owned()),
    }
}

pub fn parse_command(message: &str) -> Command {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return Command::Ignore;
    }

    let Some((keyword, argument)) = split_first_token(trimmed) else {
        return parse_exact_command(trimmed)
            .unwrap_or_else(|| Command::AddEntry(trimmed.to_owned()));
    };

    if keyword.eq_ignore_ascii_case("remove") {
        return parse_remove(argument);
    }

    Command::AddEntry(trimmed.to_owned())
}

fn parse_exact_command(message: &str) -> Option<Command> {
    if message.eq_ignore_ascii_case("list") {
        Some(Command::ListEntries)
    } else if message.eq_ignore_ascii_case("clear") {
        Some(Command::ClearEntries)
    } else if message.eq_ignore_ascii_case("help") {
        Some(Command::Help)
    } else if message.eq_ignore_ascii_case("remove") {
        Some(Command::Ignore)
    } else {
        None
    }
}

fn parse_remove(argument: &str) -> Command {
    let target = argument.trim();
    if target.is_empty() {
        return Command::Ignore;
    }

    let remove_target = target
        .parse::<usize>()
        .ok()
        .filter(|position| *position > 0)
        .map_or_else(
            || RemoveTarget::Text(target.to_owned()),
            RemoveTarget::Position,
        );

    Command::RemoveEntry(remove_target)
}

fn split_first_token(message: &str) -> Option<(&str, &str)> {
    let index = message.find(char::is_whitespace)?;

    Some((&message[..index], &message[index..]))
}

fn add_entry(store: &StoreHandle, text: String) -> Result<String, CommandExecutionError> {
    let entry_text = text.trim();
    if entry_text.is_empty() {
        return Ok("Nothing to add. Send help to see supported commands.".to_owned());
    }

    let entry = store.add_entry(entry_text)?;

    Ok(format!("Added: {}", entry.text()))
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

fn remove_entry(
    store: &StoreHandle,
    target: RemoveTarget,
) -> Result<String, CommandExecutionError> {
    match target {
        RemoveTarget::Position(position) => {
            let result = store.remove_entry_by_position(position)?;
            Ok(format_remove_result(
                result,
                &format!("position {position}"),
            ))
        }
        RemoveTarget::Text(text) => {
            let result = store.remove_entry_by_text(text.trim())?;
            Ok(format_remove_result(result, text.trim()))
        }
    }
}

fn format_remove_result(result: RemoveEntryResult, target_description: &str) -> String {
    match result {
        RemoveEntryResult::Removed(entry) => format!("Removed: {}", entry.text()),
        RemoveEntryResult::NotFound => format!("Not found: {target_description}"),
    }
}

fn clear_entries(store: &StoreHandle) -> Result<String, CommandExecutionError> {
    let result = store.clear_entries()?;

    Ok(format!("Cleared {} entries.", result.deleted_count()))
}

fn help_reply() -> String {
    [
        "Supported commands:",
        "- plain text: add an entry",
        "- list: show all entries",
        "- remove <text>: remove a matching entry",
        "- remove <number>: remove by list position",
        "- clear: remove all entries",
        "- help: show this help",
    ]
    .join("\n")
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
    fn plain_non_command_text_adds_trimmed_entry() {
        assert_eq!(
            parse_command("  Buy Milk Tomorrow  "),
            Command::AddEntry("Buy Milk Tomorrow".to_owned())
        );
    }

    #[test]
    fn empty_and_whitespace_only_messages_are_ignored() {
        assert_eq!(parse_command(""), Command::Ignore);
        assert_eq!(parse_command(" \n\t "), Command::Ignore);
    }

    #[test]
    fn exact_commands_are_case_insensitive() {
        assert_eq!(parse_command("LIST"), Command::ListEntries);
        assert_eq!(parse_command("Clear"), Command::ClearEntries);
        assert_eq!(parse_command("hElP"), Command::Help);
    }

    #[test]
    fn commands_with_extra_words_are_added_as_text_except_remove() {
        assert_eq!(
            parse_command("list groceries"),
            Command::AddEntry("list groceries".to_owned())
        );
        assert_eq!(
            parse_command("help me"),
            Command::AddEntry("help me".to_owned())
        );
    }

    #[test]
    fn remove_text_argument_is_trimmed_and_casing_is_preserved() {
        assert_eq!(
            parse_command("  ReMoVe   Fresh Milk  "),
            Command::RemoveEntry(RemoveTarget::Text("Fresh Milk".to_owned()))
        );
    }

    #[test]
    fn remove_positive_integer_argument_targets_display_position() {
        assert_eq!(
            parse_command("remove 2"),
            Command::RemoveEntry(RemoveTarget::Position(2))
        );
        assert_eq!(
            parse_command("REMOVE 0012"),
            Command::RemoveEntry(RemoveTarget::Position(12))
        );
    }

    #[test]
    fn remove_non_positive_or_non_integer_argument_targets_text() {
        assert_eq!(
            parse_command("remove 0"),
            Command::RemoveEntry(RemoveTarget::Text("0".to_owned()))
        );
        assert_eq!(
            parse_command("remove -2"),
            Command::RemoveEntry(RemoveTarget::Text("-2".to_owned()))
        );
        assert_eq!(
            parse_command("remove 2.0"),
            Command::RemoveEntry(RemoveTarget::Text("2.0".to_owned()))
        );
    }

    #[test]
    fn remove_without_target_is_ignored() {
        assert_eq!(parse_command("remove"), Command::Ignore);
        assert_eq!(parse_command(" remove   "), Command::Ignore);
    }

    #[test]
    fn execute_add_then_list_shows_one_based_item() {
        let database = TestDatabase::new("execute_add_list");
        let store = initialized_store(&database);

        let add_reply =
            execute_command(&store, parse_command("milk")).expect("add command should execute");
        let list_reply =
            execute_command(&store, Command::ListEntries).expect("list command should execute");

        assert_eq!(add_reply, "Added: milk");
        assert_eq!(list_reply, "1. milk");
    }

    #[test]
    fn execute_empty_list_reply_is_clear() {
        let database = TestDatabase::new("execute_empty_list");
        let store = initialized_store(&database);

        let reply =
            execute_command(&store, Command::ListEntries).expect("list command should execute");

        assert_eq!(reply, "The list is empty.");
    }

    #[test]
    fn execute_remove_by_text_reports_success_and_not_found() {
        let database = TestDatabase::new("execute_remove_text");
        let store = initialized_store(&database);
        execute_command(&store, parse_command("milk")).expect("add command should execute");

        let removed = execute_command(&store, parse_command("remove milk"))
            .expect("remove command should execute");
        let not_found = execute_command(&store, parse_command("remove milk"))
            .expect("remove command should execute");

        assert_eq!(removed, "Removed: milk");
        assert_eq!(not_found, "Not found: milk");
    }

    #[test]
    fn execute_remove_by_number_uses_list_numbering() {
        let database = TestDatabase::new("execute_remove_number");
        let store = initialized_store(&database);
        execute_command(&store, parse_command("first")).expect("first add should execute");
        execute_command(&store, parse_command("second")).expect("second add should execute");

        let removed = execute_command(&store, parse_command("remove 2"))
            .expect("remove command should execute");
        let list_reply =
            execute_command(&store, Command::ListEntries).expect("list command should execute");

        assert_eq!(removed, "Removed: second");
        assert_eq!(list_reply, "1. first");
    }

    #[test]
    fn execute_clear_reply_reflects_deleted_count() {
        let database = TestDatabase::new("execute_clear");
        let store = initialized_store(&database);
        execute_command(&store, parse_command("first")).expect("first add should execute");
        execute_command(&store, parse_command("second")).expect("second add should execute");

        let reply =
            execute_command(&store, Command::ClearEntries).expect("clear command should execute");
        let list_reply =
            execute_command(&store, Command::ListEntries).expect("list command should execute");

        assert_eq!(reply, "Cleared 2 entries.");
        assert_eq!(list_reply, "The list is empty.");
    }

    #[test]
    fn execute_help_lists_supported_commands() {
        let database = TestDatabase::new("execute_help");
        let store = initialized_store(&database);

        let reply = execute_command(&store, Command::Help).expect("help command should execute");

        assert!(reply.contains("plain text"));
        assert!(reply.contains("list"));
        assert!(reply.contains("remove <text>"));
        assert!(reply.contains("remove <number>"));
        assert!(reply.contains("clear"));
        assert!(reply.contains("help"));
    }

    #[test]
    fn execute_ignore_returns_no_op_reply() {
        let database = TestDatabase::new("execute_ignore");
        let store = initialized_store(&database);

        let reply =
            execute_command(&store, Command::Ignore).expect("ignore command should execute");
        let list_reply =
            execute_command(&store, Command::ListEntries).expect("list command should execute");

        assert_eq!(reply, "Nothing to do. Send help to see supported commands.");
        assert_eq!(list_reply, "The list is empty.");
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
