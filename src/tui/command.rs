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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BuiltinCommand {
    Help,
    Clear,
    Model,
    Status,
    Exit,
    Login,
    Logout,
    Compact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandMetadata {
    pub name: &'static str,
    pub description: &'static str,
    pub usage: &'static str,
    command: BuiltinCommand,
}

const COMMANDS: [CommandMetadata; 8] = [
    CommandMetadata {
        name: "/help",
        description: "查看内置命令",
        usage: "/help",
        command: BuiltinCommand::Help,
    },
    CommandMetadata {
        name: "/clear",
        description: "清空当前 transcript",
        usage: "/clear",
        command: BuiltinCommand::Clear,
    },
    CommandMetadata {
        name: "/model",
        description: "查看或切换后续请求 model",
        usage: "/model [name]",
        command: BuiltinCommand::Model,
    },
    CommandMetadata {
        name: "/status",
        description: "当前会话快照",
        usage: "/status",
        command: BuiltinCommand::Status,
    },
    CommandMetadata {
        name: "/exit",
        description: "退出 TUI",
        usage: "/exit",
        command: BuiltinCommand::Exit,
    },
    CommandMetadata {
        name: "/login",
        description: "凭据配置提示",
        usage: "/login",
        command: BuiltinCommand::Login,
    },
    CommandMetadata {
        name: "/logout",
        description: "登录态清理提示",
        usage: "/logout",
        command: BuiltinCommand::Logout,
    },
    CommandMetadata {
        name: "/compact",
        description: "压缩当前上下文",
        usage: "/compact",
        command: BuiltinCommand::Compact,
    },
];

pub fn command_metadata() -> &'static [CommandMetadata] {
    &COMMANDS
}

pub fn parse_command(input: &str) -> Option<Command> {
    let input = input.trim();
    let rest = input.strip_prefix('/')?.trim();
    let (name, args) = match rest.split_once(char::is_whitespace) {
        Some((name, args)) => (name, args.trim()),
        None => (rest, ""),
    };

    let Some(metadata) = command_metadata()
        .iter()
        .find(|command| command.name.strip_prefix('/') == Some(name))
    else {
        return Some(Command::Unknown(name.to_string()));
    };

    let command = match metadata.command {
        BuiltinCommand::Help => Command::Help,
        BuiltinCommand::Clear => Command::Clear,
        BuiltinCommand::Model if args.is_empty() => Command::Model(None),
        BuiltinCommand::Model => Command::Model(Some(args.to_string())),
        BuiltinCommand::Status => Command::Status,
        BuiltinCommand::Exit => Command::Exit,
        BuiltinCommand::Login => Command::Login,
        BuiltinCommand::Logout => Command::Logout,
        BuiltinCommand::Compact => Command::Compact,
    };

    Some(command)
}

#[cfg(test)]
mod tests {
    use super::{command_metadata, parse_command, Command};

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

    #[test]
    fn command_metadata_covers_all_builtin_commands_and_matches_parser() {
        let expected = [
            "/help", "/clear", "/model", "/status", "/exit", "/login", "/logout", "/compact",
        ];
        let metadata = command_metadata();
        let names = metadata
            .iter()
            .map(|command| command.name)
            .collect::<Vec<_>>();

        assert_eq!(names, expected);

        for command in metadata {
            assert!(
                !command.description.trim().is_empty(),
                "{} should expose a description for completion",
                command.name
            );
            assert!(
                !command.usage.trim().is_empty(),
                "{} should expose usage text for completion",
                command.name
            );
            let parsed = parse_command(command.name);
            assert!(
                parsed.is_some_and(|command| !matches!(command, Command::Unknown(_))),
                "{} should parse as a builtin command",
                command.name
            );
        }
    }
}
