#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EscapeCommand {
    Help,
    LiteralTilde,
    Disconnect,
    Prompt,
    Put,
    Get,
}

pub(super) fn parse_escape_command(command: &str) -> Option<EscapeCommand> {
    match command.trim_end() {
        "~?" | "~help" => Some(EscapeCommand::Help),
        "~~" => Some(EscapeCommand::LiteralTilde),
        "~." => Some(EscapeCommand::Disconnect),
        "~C" | "~c" => Some(EscapeCommand::Prompt),
        s if s.starts_with("~put") => Some(EscapeCommand::Put),
        s if s.starts_with("~get") => Some(EscapeCommand::Get),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{EscapeCommand, parse_escape_command};

    #[test]
    fn question_mark_escape_parses_as_help() {
        assert_eq!(parse_escape_command("~?"), Some(EscapeCommand::Help));
    }

    #[test]
    fn help_escape_parses_as_help() {
        assert_eq!(parse_escape_command("~help"), Some(EscapeCommand::Help));
        assert_eq!(parse_escape_command("~help   "), Some(EscapeCommand::Help));
        assert_eq!(parse_escape_command("~?\t"), Some(EscapeCommand::Help));
    }

    #[test]
    fn ssh_core_escapes_parse() {
        assert_eq!(
            parse_escape_command("~~"),
            Some(EscapeCommand::LiteralTilde)
        );
        assert_eq!(parse_escape_command("~."), Some(EscapeCommand::Disconnect));
        assert_eq!(parse_escape_command("~C"), Some(EscapeCommand::Prompt));
        assert_eq!(parse_escape_command("~c"), Some(EscapeCommand::Prompt));
        assert_eq!(
            parse_escape_command("~~   "),
            Some(EscapeCommand::LiteralTilde)
        );
        assert_eq!(
            parse_escape_command("~.   "),
            Some(EscapeCommand::Disconnect)
        );
    }

    #[test]
    fn unknown_escape_remains_passthrough() {
        assert_eq!(parse_escape_command("~x"), None);
        assert_eq!(parse_escape_command("~help me"), None);
    }
}
