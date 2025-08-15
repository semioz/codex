//! Session management utilities for listing, resuming, and managing conversation sessions.

use serde::Serialize;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use uuid::Uuid;

use crate::config::Config;
use crate::rollout::SessionMeta;

const SESSIONS_SUBDIR: &str = "sessions";

#[derive(Debug, Clone, Serialize)]
pub struct SessionListItem {
    pub id: Uuid,
    pub path: PathBuf,
    pub timestamp: String,
    pub instructions: Option<String>,
    pub message_count: usize,
    pub last_modified: std::time::SystemTime,
    pub created_time: std::time::SystemTime,
    pub git_branch: Option<String>,
}

/// Lists all available conversation sessions in the codex home directory.
pub fn list_sessions(config: &Config) -> std::io::Result<Vec<SessionListItem>> {
    let sessions_dir = config.codex_home.join(SESSIONS_SUBDIR);

    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();

    // Walk through the nested directory structure: YYYY/MM/DD/
    for year_entry in fs::read_dir(&sessions_dir)? {
        let year_entry = year_entry?;
        if !year_entry.file_type()?.is_dir() {
            continue;
        }

        for month_entry in fs::read_dir(year_entry.path())? {
            let month_entry = month_entry?;
            if !month_entry.file_type()?.is_dir() {
                continue;
            }

            for day_entry in fs::read_dir(month_entry.path())? {
                let day_entry = day_entry?;
                if !day_entry.file_type()?.is_dir() {
                    continue;
                }

                for file_entry in fs::read_dir(day_entry.path())? {
                    let file_entry = file_entry?;
                    let path = file_entry.path();

                    if path.extension().map_or(false, |ext| ext == "jsonl")
                        && path
                            .file_name()
                            .map_or(false, |name| name.to_string_lossy().starts_with("rollout-"))
                    {
                        if let Ok(session_info) = parse_session_file(&path) {
                            sessions.push(session_info);
                        }
                    }
                }
            }
        }
    }

    // Sort by timestamp (newest first)
    sessions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));

    Ok(sessions)
}

/// Gets the most recent session.
pub fn get_last_session(config: &Config) -> std::io::Result<Option<SessionListItem>> {
    let sessions = list_sessions(config)?;
    Ok(sessions.into_iter().next())
}

/// Finds a session by ID (partial match supported) or exact path.
pub fn find_session(config: &Config, session_id_or_path: &str) -> std::io::Result<Option<PathBuf>> {
    // If it's a path, check if it exists
    let path = Path::new(session_id_or_path);
    if path.exists() && path.extension().map_or(false, |ext| ext == "jsonl") {
        return Ok(Some(path.to_path_buf()));
    }

    // Otherwise, search by session ID (support partial matching)
    let sessions = list_sessions(config)?;
    let query = session_id_or_path.to_lowercase();

    for session in sessions {
        let id_str = session.id.to_string();
        if id_str.to_lowercase().starts_with(&query) || id_str == session_id_or_path {
            return Ok(Some(session.path));
        }
    }

    Ok(None)
}

/// Parses a session file to extract metadata and count messages.
fn parse_session_file(path: &Path) -> std::io::Result<SessionListItem> {
    let content = fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();

    if lines.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Empty session file",
        ));
    }

    // First line should contain the session metadata with potential git info
    let metadata_value: serde_json::Value = serde_json::from_str(lines[0]).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Failed to parse session metadata: {}", e),
        )
    })?;

    // Extract session metadata
    let session_meta: SessionMeta =
        serde_json::from_value(metadata_value.clone()).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to parse session metadata: {}", e),
            )
        })?;

    // Extract git info if present
    let git_branch = metadata_value
        .get("git")
        .and_then(|git| git.get("branch"))
        .and_then(|branch| branch.as_str())
        .map(|s| s.to_string());

    // Count message items (excluding metadata and state records)
    let message_count = lines[1..]
        .iter()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|item| {
            // Count actual conversation items, not state records
            !item.get("record_type").is_some()
        })
        .count();

    let metadata = fs::metadata(path)?;
    let last_modified = metadata.modified()?;
    let created_time = metadata.created().unwrap_or(last_modified);

    Ok(SessionListItem {
        id: session_meta.id,
        path: path.to_path_buf(),
        timestamp: session_meta.timestamp,
        instructions: session_meta.instructions,
        message_count,
        last_modified,
        created_time,
        git_branch,
    })
}

/// Prints a formatted list of sessions for interactive selection.
pub fn print_session_list(sessions: &[SessionListItem]) {
    if sessions.is_empty() {
        println!("No conversation sessions found.");
        return;
    }

    println!(
        "    {:10} {:10} {:>10} {:15} {}",
        "Modified", "Created", "# Messages", "Git Branch", "Summary"
    );

    for (index, session) in sessions.iter().enumerate() {
        let modified_ago = format_time_ago(session.last_modified);
        let created_ago = format_time_ago(session.created_time);

        let git_branch = session
            .git_branch
            .as_ref()
            .map(|b| {
                if b.len() > 14 {
                    format!("{}...", &b[..11])
                } else {
                    b.clone()
                }
            })
            .unwrap_or_else(|| "-".to_string());

        let summary = session
            .instructions
            .as_ref()
            .map(|s| {
                if s.len() > 50 {
                    format!("{}...", &s[..47])
                } else {
                    s.clone()
                }
            })
            .unwrap_or_else(|| "No summary available".to_string());

        let marker = if index == 0 { "❯" } else { " " };

        println!(
            "{} {}. {:10} {:10} {:>10} {:15} {}",
            marker,
            index + 1,
            modified_ago,
            created_ago,
            session.message_count,
            git_branch,
            summary
        );
    }

    println!();
    println!("Use arrow keys to navigate and press Enter to select a session");
    println!("Use --resume <session_id> to resume a specific session");
    println!("Use --continue to resume the most recent session");
}

fn format_time_ago(time: std::time::SystemTime) -> String {
    let duration = time.elapsed().unwrap_or_default();
    let secs = duration.as_secs();

    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}
