use anyhow::Context;
use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

/// Represents a custom slash command loaded from a markdown file
#[derive(Debug, Clone)]
pub struct CustomCommand {
    /// The command name (derived from filename)
    pub name: String,
    /// The full prompt content from the markdown file
    pub content: String,
    /// Whether this command supports $ARGUMENTS placeholder
    pub supports_arguments: bool,
    /// The source of this command (user/project/subdirectory)
    pub source: CommandSource,
    /// Optional subdirectory for organization (e.g., "frontend" from frontend/component.md)
    pub subdirectory: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CommandSource {
    /// Personal command from ~/.codex/commands/
    User,
    /// Project-specific command from .codex/commands/
    Project,
}

impl CustomCommand {
    /// Create a new custom command
    pub fn new(
        name: String,
        content: String,
        source: CommandSource,
        subdirectory: Option<String>,
    ) -> Self {
        let supports_arguments = content.contains("$ARGUMENTS");
        Self {
            name,
            content,
            supports_arguments,
            source,
            subdirectory,
        }
    }

    /// Get the display name for the command including subdirectory context
    pub fn display_name(&self) -> String {
        match &self.subdirectory {
            Some(subdir) => format!("{} ({}:{})", self.name, self.source_label(), subdir),
            None => format!("{} ({})", self.name, self.source_label()),
        }
    }

    /// Get the source label for display
    pub fn source_label(&self) -> &'static str {
        match self.source {
            CommandSource::User => "user",
            CommandSource::Project => "project",
        }
    }

    /// Generate the final prompt by replacing $ARGUMENTS placeholder
    pub fn generate_prompt(&self, arguments: Option<&str>) -> String {
        if self.supports_arguments {
            if let Some(args) = arguments {
                self.content.replace("$ARGUMENTS", args)
            } else {
                self.content.replace("$ARGUMENTS", "")
            }
        } else {
            self.content.clone()
        }
    }

    /// Get a short description for the command (first line of content, truncated)
    pub fn description(&self) -> String {
        let first_line = self.content.lines().next().unwrap_or("").trim();

        if first_line.len() > 80 {
            format!("{}...", &first_line[..77])
        } else {
            first_line.to_string()
        }
    }
}

/// Loads custom commands from the filesystem
pub struct CustomCommandLoader {
    /// Cache of loaded commands
    commands: HashMap<String, CustomCommand>,
    /// Timestamp of last load for cache invalidation
    last_loaded: std::time::SystemTime,
}

impl CustomCommandLoader {
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
            last_loaded: std::time::UNIX_EPOCH,
        }
    }

    /// Load or reload all custom commands
    pub fn load_commands(&mut self, project_root: Option<&Path>) -> Result<()> {
        self.commands.clear();

        // Load personal commands from ~/.codex/commands/
        if let Some(home_dir) = dirs::home_dir() {
            let user_commands_dir = home_dir.join(".codex").join("commands");
            if user_commands_dir.exists() {
                self.load_commands_from_directory(&user_commands_dir, CommandSource::User, None)?;
            }
        }

        // Load project commands from .codex/commands/
        if let Some(root) = project_root {
            let project_commands_dir = root.join(".codex").join("commands");
            if project_commands_dir.exists() {
                self.load_commands_from_directory(
                    &project_commands_dir,
                    CommandSource::Project,
                    None,
                )?;
            }
        }

        self.last_loaded = std::time::SystemTime::now();
        Ok(())
    }

    /// Load commands from a specific directory
    fn load_commands_from_directory(
        &mut self,
        dir: &Path,
        source: CommandSource,
        subdirectory: Option<String>,
    ) -> Result<()> {
        let entries = fs::read_dir(dir)
            .with_context(|| format!("Failed to read directory: {}", dir.display()))?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("md") {
                // Load markdown file as command
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let content = fs::read_to_string(&path).with_context(|| {
                        format!("Failed to read command file: {}", path.display())
                    })?;

                    let command = CustomCommand::new(
                        stem.to_string(),
                        content,
                        source.clone(),
                        subdirectory.clone(),
                    );

                    self.commands.insert(stem.to_string(), command);
                }
            } else if path.is_dir() {
                // Recursively load commands from subdirectory
                let subdir_name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string());

                self.load_commands_from_directory(&path, source.clone(), subdir_name)?;
            }
        }

        Ok(())
    }

    /// Get all loaded commands
    pub fn get_commands(&self) -> &HashMap<String, CustomCommand> {
        &self.commands
    }

    /// Get a specific command by name
    pub fn get_command(&self, name: &str) -> Option<&CustomCommand> {
        self.commands.get(name)
    }

    /// Check if commands need to be reloaded based on filesystem changes
    pub fn needs_reload(&self, project_root: Option<&Path>) -> bool {
        // For simplicity, we'll reload every minute. In a real implementation,
        // you might want to use filesystem watching or check modification times
        match self.last_loaded.elapsed() {
            Ok(duration) => duration.as_secs() > 60,
            Err(_) => true,
        }
    }
}

impl Default for CustomCommandLoader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_custom_command_creation() {
        let command = CustomCommand::new(
            "test".to_string(),
            "This is a test command with $ARGUMENTS".to_string(),
            CommandSource::User,
            None,
        );

        assert_eq!(command.name, "test");
        assert!(command.supports_arguments);
        assert_eq!(command.source, CommandSource::User);
        assert_eq!(command.subdirectory, None);
    }

    #[test]
    fn test_argument_replacement() {
        let command = CustomCommand::new(
            "fix-issue".to_string(),
            "Fix issue #$ARGUMENTS in the codebase".to_string(),
            CommandSource::Project,
            None,
        );

        let prompt_with_args = command.generate_prompt(Some("123"));
        assert_eq!(prompt_with_args, "Fix issue #123 in the codebase");

        let prompt_without_args = command.generate_prompt(None);
        assert_eq!(prompt_without_args, "Fix issue # in the codebase");
    }

    #[test]
    fn test_display_name() {
        let user_command = CustomCommand::new(
            "review".to_string(),
            "Review this code".to_string(),
            CommandSource::User,
            None,
        );
        assert_eq!(user_command.display_name(), "review (user)");

        let project_command = CustomCommand::new(
            "component".to_string(),
            "Create a component".to_string(),
            CommandSource::Project,
            Some("frontend".to_string()),
        );
        assert_eq!(
            project_command.display_name(),
            "component (project:frontend)"
        );
    }

    #[test]
    fn test_command_loader() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let commands_dir = temp_dir.path().join("commands");
        fs::create_dir_all(&commands_dir)?;

        // Create a test command file
        fs::write(
            commands_dir.join("optimize.md"),
            "Analyze the performance of this code and suggest three specific optimizations:",
        )?;

        // Create a subdirectory with another command
        let frontend_dir = commands_dir.join("frontend");
        fs::create_dir_all(&frontend_dir)?;
        fs::write(
            frontend_dir.join("component.md"),
            "Create a React component for $ARGUMENTS",
        )?;

        let mut loader = CustomCommandLoader::new();
        loader.load_commands_from_directory(&commands_dir, CommandSource::Project, None)?;

        let commands = loader.get_commands();
        assert_eq!(commands.len(), 2);

        let optimize_cmd = commands.get("optimize").unwrap();
        assert_eq!(optimize_cmd.name, "optimize");
        assert!(!optimize_cmd.supports_arguments);

        let component_cmd = commands.get("component").unwrap();
        assert_eq!(component_cmd.name, "component");
        assert!(component_cmd.supports_arguments);
        assert_eq!(component_cmd.subdirectory, Some("frontend".to_string()));

        Ok(())
    }
}
