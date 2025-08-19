use crate::custom_command::CustomCommand;
use crate::slash_command::SlashCommand;

/// Unified representation of all available commands (built-in and custom)
#[derive(Debug, Clone)]
pub enum AnyCommand {
    BuiltIn(SlashCommand),
    Custom {
        name: String,
        command: CustomCommand,
    },
}

impl AnyCommand {
    /// Get the command name (without the slash)
    pub fn name(&self) -> String {
        match self {
            AnyCommand::BuiltIn(cmd) => cmd.command().to_string(),
            AnyCommand::Custom { name, .. } => name.clone(),
        }
    }

    /// Get the description for this command
    pub fn description(&self) -> String {
        match self {
            AnyCommand::BuiltIn(cmd) => cmd.description().to_string(),
            AnyCommand::Custom { command, .. } => command.description(),
        }
    }

    /// Get the display name with source context
    pub fn display_name(&self) -> String {
        match self {
            AnyCommand::BuiltIn(cmd) => cmd.command().to_string(),
            AnyCommand::Custom { command, .. } => command.display_name(),
        }
    }

    /// Check if this command supports arguments
    pub fn supports_arguments(&self) -> bool {
        match self {
            AnyCommand::BuiltIn(_) => false,
            AnyCommand::Custom { command, .. } => command.supports_arguments,
        }
    }
}

/// Create a combined list of all available commands
pub fn all_available_commands(custom_commands: &std::collections::HashMap<String, CustomCommand>) -> Vec<AnyCommand> {
    let mut commands = Vec::new();
    
    // Add built-in commands
    for (_, built_in) in crate::slash_command::built_in_slash_commands() {
        commands.push(AnyCommand::BuiltIn(built_in));
    }
    
    // Add custom commands
    for (name, custom_cmd) in custom_commands {
        commands.push(AnyCommand::Custom {
            name: name.clone(),
            command: custom_cmd.clone(),
        });
    }
    
    commands
}
