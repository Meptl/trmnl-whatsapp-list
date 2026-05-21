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

#[cfg(test)]
mod tests {
    use super::*;

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
}
