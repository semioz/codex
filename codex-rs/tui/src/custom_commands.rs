use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// A custom slash command loaded from a markdown file
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CustomSlashCommand {
    /// The command name (derived from filename)
    pub name: String,
    /// The command description/content from the markdown file
    pub content: String,
    /// The source type (user or project)
    pub source: CommandSource,
    /// The subdirectory path (for organization)
    pub subdirectory: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CommandSource {
    User,
    Project,
}

impl CustomSlashCommand {
    /// Get the description with source indicator
    pub fn description(&self) -> String {
        let source_indicator = match self.source {
            CommandSource::User => "(user)",
            CommandSource::Project => "(project)",
        };

        // Take first line of content as description, fallback to source indicator
        let first_line = self.content.lines().next().unwrap_or("").trim();

        if first_line.is_empty() {
            format!("Custom command {}", source_indicator)
        } else {
            format!("{} {}", first_line, source_indicator)
        }
    }

    /// Get the prompt content with arguments substituted
    pub fn get_prompt(&self, arguments: &str) -> String {
        if arguments.is_empty() {
            self.content.clone()
        } else {
            self.content.replace("$ARGUMENTS", arguments)
        }
    }
}

/// Manager for loading and caching custom slash commands
pub struct CustomCommandManager {
    commands: HashMap<String, CustomSlashCommand>,
}

impl CustomCommandManager {
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }

    /// Load all custom commands from both user and project directories
    pub fn load_commands(
        &mut self,
        codex_home: &Path,
        project_root: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.commands.clear();

        // Load user commands from ~/.codex/commands
        let user_commands_dir = codex_home.join("commands");
        if user_commands_dir.exists() {
            self.load_commands_from_directory(&user_commands_dir, CommandSource::User, None)?;
        }

        // Load project commands from .codex/commands
        let project_commands_dir = project_root.join(".codex").join("commands");
        if project_commands_dir.exists() {
            self.load_commands_from_directory(&project_commands_dir, CommandSource::Project, None)?;
        }

        Ok(())
    }

    /// Load commands from a specific directory, handling subdirectories recursively
    fn load_commands_from_directory(
        &mut self,
        dir: &Path,
        source: CommandSource,
        subdirectory: Option<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                // Recursively load from subdirectory
                let subdir_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("")
                    .to_string();

                let new_subdirectory = match &subdirectory {
                    Some(parent) => Some(format!("{}:{}", parent, subdir_name)),
                    None => Some(subdir_name),
                };

                self.load_commands_from_directory(&path, source.clone(), new_subdirectory)?;
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
                // Load markdown file as command
                if let Some(command) =
                    self.load_command_from_file(&path, source.clone(), subdirectory.clone())?
                {
                    self.commands.insert(command.name.clone(), command);
                }
            }
        }

        Ok(())
    }

    /// Load a single command from a markdown file
    fn load_command_from_file(
        &self,
        path: &Path,
        source: CommandSource,
        subdirectory: Option<String>,
    ) -> Result<Option<CustomSlashCommand>, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;

        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or("Invalid filename")?
            .to_string();

        // Skip files that would conflict with built-in commands
        if is_builtin_command(&name) {
            return Ok(None);
        }

        Ok(Some(CustomSlashCommand {
            name,
            content: content.trim().to_string(),
            source,
            subdirectory,
        }))
    }

    /// Get all loaded commands
    pub fn get_commands(&self) -> Vec<&CustomSlashCommand> {
        self.commands.values().collect()
    }
}

/// Check if a command name conflicts with built-in commands
fn is_builtin_command(name: &str) -> bool {
    matches!(
        name,
        "new" | "init" | "compact" | "diff" | "mention" | "status" | "logout" | "quit" | "prompts"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_custom_command_creation() {
        let cmd = CustomSlashCommand {
            name: "test".to_string(),
            content: "This is a test command with $ARGUMENTS".to_string(),
            source: CommandSource::User,
            subdirectory: None,
        };

        assert_eq!(cmd.name, "test");
        assert_eq!(cmd.get_prompt("hello"), "This is a test command with hello");
        assert_eq!(cmd.get_prompt(""), "This is a test command with $ARGUMENTS");
    }

    #[test]
    fn test_command_with_subdirectory() {
        let cmd = CustomSlashCommand {
            name: "component".to_string(),
            content: "Create a component".to_string(),
            source: CommandSource::Project,
            subdirectory: Some("frontend".to_string()),
        };

        assert_eq!(cmd.name, "component");
        assert!(cmd.description().contains("(project)"));
    }

    #[test]
    fn test_builtin_command_detection() {
        assert!(is_builtin_command("new"));
        assert!(is_builtin_command("init"));
        assert!(!is_builtin_command("custom-command"));
    }
}
