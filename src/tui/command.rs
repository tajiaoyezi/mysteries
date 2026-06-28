#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Clear,
    Model(Option<String>),
    Status,
    Exit,
    Compact,
    Login,
    Logout,
    Unknown(String),
}

pub fn parse_command(input: &str) -> Option<Command> {
    let input = input.trim();
    let rest = input.strip_prefix('/')?.trim();
    let (name, args) = match rest.split_once(char::is_whitespace) {
        Some((name, args)) => (name, args.trim()),
        None => (rest, ""),
    };

    let command = match name {
        "help" => Command::Help,
        "clear" => Command::Clear,
        "model" if args.is_empty() => Command::Model(None),
        "model" => Command::Model(Some(args.to_string())),
        "status" => Command::Status,
        "compact" => Command::Compact,
        "exit" => Command::Exit,
        "login" => Command::Login,
        "logout" => Command::Logout,
        unknown => Command::Unknown(unknown.to_string()),
    };

    Some(command)
}

#[cfg(test)]
mod tests {
    use super::{parse_command, Command};

    #[test]
    fn parse_command_recognizes_builtin_slash_commands() {
        let cases = [
            ("/help", Some(Command::Help)),
            ("/clear", Some(Command::Clear)),
            ("/model", Some(Command::Model(None))),
            (
                "/model claude-haiku",
                Some(Command::Model(Some("claude-haiku".to_string()))),
            ),
            ("/status", Some(Command::Status)),
            ("/compact", Some(Command::Compact)),
            ("/exit", Some(Command::Exit)),
            ("/login", Some(Command::Login)),
            ("/logout", Some(Command::Logout)),
            ("/xyz", Some(Command::Unknown("xyz".to_string()))),
            ("write code", None),
        ];

        for (input, expected) in cases {
            assert_eq!(parse_command(input), expected, "input: {input}");
        }
    }
}
