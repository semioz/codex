use strum::IntoEnumIterator;
use strum_macros::AsRefStr;
use strum_macros::EnumIter;
use strum_macros::EnumString;
use strum_macros::IntoStaticStr;

use crate::custom_commands::CustomSlashCommand;

/// Commands that can be invoked by starting a message with a leading slash.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SlashCommand {
    // Built-in commands
    BuiltIn(BuiltInSlashCommand),
    // Custom commands loaded from files
    Custom(CustomSlashCommand),
}

/// Built-in slash commands that are always available
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, EnumIter, AsRefStr, IntoStaticStr,
)]
#[strum(serialize_all = "kebab-case")]
pub enum BuiltInSlashCommand {
    // DO NOT ALPHA-SORT! Enum order is presentation order in the popup, so
    // more frequently used commands should be listed first.
    New,
    Init,
    Compact,
    Diff,
    Mention,
    Status,
    Logout,
    Quit,
    #[cfg(debug_assertions)]
    TestApproval,
}

impl BuiltInSlashCommand {
    /// User-visible description shown in the popup.
    pub fn description(self) -> &'static str {
        match self {
            BuiltInSlashCommand::New => "start a new chat during a conversation",
            BuiltInSlashCommand::Init => "create an AGENTS.md file with instructions for Codex",
            BuiltInSlashCommand::Compact => {
                "summarize conversation to prevent hitting the context limit"
            }
            BuiltInSlashCommand::Quit => "exit Codex",
            BuiltInSlashCommand::Diff => "show git diff (including untracked files)",
            BuiltInSlashCommand::Mention => "mention a file",
            BuiltInSlashCommand::Status => "show current session configuration and token usage",
            BuiltInSlashCommand::Logout => "log out of Codex",
            #[cfg(debug_assertions)]
            BuiltInSlashCommand::TestApproval => "test approval request",
        }
    }

    /// Command string without the leading '/'. Provided for compatibility with
    /// existing code that expects a method named `command()`.
    pub fn command(self) -> &'static str {
        self.into()
    }
}

impl SlashCommand {
    /// User-visible description shown in the popup.
    pub fn description(&self) -> String {
        match self {
            SlashCommand::BuiltIn(builtin) => builtin.description().to_string(),
            SlashCommand::Custom(custom) => custom.description(),
        }
    }

    /// Command string without the leading '/'.
    pub fn command(&self) -> String {
        match self {
            SlashCommand::BuiltIn(builtin) => builtin.command().to_string(),
            SlashCommand::Custom(custom) => custom.name.clone(),
        }
    }
}

/// Return all built-in commands in a Vec paired with their command string.
pub fn built_in_slash_commands() -> Vec<(&'static str, SlashCommand)> {
    BuiltInSlashCommand::iter()
        .map(|c| (c.command(), SlashCommand::BuiltIn(c)))
        .collect()
}

/// Create all available slash commands (built-in + custom)
pub fn all_slash_commands(custom_commands: Vec<CustomSlashCommand>) -> Vec<(String, SlashCommand)> {
    let mut all_commands: Vec<(String, SlashCommand)> = BuiltInSlashCommand::iter()
        .map(|c| (c.command().to_string(), SlashCommand::BuiltIn(c)))
        .collect();

    // Add custom commands
    for custom_cmd in custom_commands {
        let name = custom_cmd.name.clone();
        all_commands.push((name, SlashCommand::Custom(custom_cmd)));
    }

    // Sort built-in commands first, then custom commands, both alphabetically
    all_commands.sort_by(|a, b| match (&a.1, &b.1) {
        (SlashCommand::BuiltIn(_), SlashCommand::Custom(_)) => std::cmp::Ordering::Less,
        (SlashCommand::Custom(_), SlashCommand::BuiltIn(_)) => std::cmp::Ordering::Greater,
        _ => a.0.cmp(&b.0),
    });

    all_commands
}
