use anyhow::{Context, Result};
use chrono::{Local, TimeZone};
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::PathBuf,
};

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub timestamp: i64,
    pub command: String,
    pub directory: Option<String>,
    pub duration: Option<i64>,  // Keep for potential future use
    pub exit_code: Option<i32>, // Keep for potential future use
}

fn get_zsh_history_path() -> Result<PathBuf> {
    let home = home::home_dir().context("Could not find home directory")?;
    Ok(home.join(".zsh_history"))
}

fn get_cli_stats_log_path() -> Result<PathBuf> {
    let home = home::home_dir().context("Could not find home directory")?;
    Ok(home.join(".cli_stats_log"))
}

fn parse_history_line(line: &str) -> Option<HistoryEntry> {
    // Handle Zsh history format: ": timestamp:0;command"
    if line.starts_with(": ") {
        let parts: Vec<&str> = line.splitn(3, ';').collect();
        if parts.len() < 2 {
            return None;
        }

        let ts_part = parts[0].strip_prefix(": ")?.trim();
        let timestamp = ts_part.splitn(2, ':').next()?.parse().ok()?;
        let command = parts[1].trim().to_string();

        if !command.is_empty() {
            return Some(HistoryEntry {
                timestamp,
                command,
                directory: None, // Standard Zsh history doesn't store directory
                duration: None,
                exit_code: None,
            });
        }
    } else {
        // Plain command
        let command = line.trim().to_string();
        if !command.is_empty() {
            return Some(HistoryEntry {
                timestamp: 0,
                command,
                directory: None,
                duration: None,
                exit_code: None,
            });
        }
    }

    None
}

fn parse_cli_stats_line(line: &str) -> Option<HistoryEntry> {
    // Three possible formats:
    // 1. Plain command: "git branch --show-current"
    // 2. Zsh format: ": 1744405541:0;nvim ~/.zshrc"
    // 3. Stats format: "1744686489|cargo run -- stats|/Users/uzair/01-dev/cli-wrapped"
    //    Alternative: "1744686489:cargo run -- stats:/Users/uzair/01-dev/cli-wrapped"

    // Try to parse as pipe-delimited format first
    let pipe_parts: Vec<&str> = line.split('|').collect();
    if pipe_parts.len() == 3 {
        let timestamp = pipe_parts[0].parse::<i64>().ok()?;
        let command = pipe_parts[1].to_string();
        let dir_str = pipe_parts[2].trim();

        // Validate the directory - it should start with / or ~ to be valid
        let directory = if is_valid_directory(dir_str) {
            Some(dir_str.to_string())
        } else {
            None
        };

        if !command.is_empty() {
            return Some(HistoryEntry {
                timestamp,
                command,
                directory,
                duration: None,
                exit_code: None,
            });
        }
    }

    // Try to parse as timestamp:command:directory format
    if !line.starts_with(": ") {
        let parts: Vec<&str> = line.splitn(3, ':').collect();
        if parts.len() >= 3 {
            // Check if first part looks like a timestamp (all digits)
            if parts[0].chars().all(|c| c.is_digit(10)) {
                let timestamp = parts[0].parse::<i64>().ok()?;

                // Get the directory (last part)
                let dir_str = parts.last().unwrap().trim();

                // Validate the directory
                let directory = if is_valid_directory(dir_str) {
                    Some(dir_str.to_string())
                } else {
                    None
                };

                // Join parts after the timestamp and before the last part as the command
                let command_parts = &parts[1..parts.len() - 1];
                let command = command_parts.join(":").trim().to_string();

                if !command.is_empty() {
                    return Some(HistoryEntry {
                        timestamp,
                        command,
                        directory,
                        duration: None,
                        exit_code: None,
                    });
                }
            }
        }
    }

    // Try to parse as Zsh history format
    if line.starts_with(": ") {
        let timestamp_part = line.strip_prefix(": ")?;
        let parts: Vec<&str> = timestamp_part.splitn(2, ';').collect();
        if parts.len() < 2 {
            return None;
        }

        // Get timestamp from first part (timestamp:0)
        let ts_parts: Vec<&str> = parts[0].splitn(2, ':').collect();
        let timestamp = ts_parts[0].parse::<i64>().ok()?;

        // Now try to extract command and possibly directory
        let cmd_dir: Vec<&str> = parts[1].splitn(2, ':').collect();
        let command = cmd_dir[0].to_string();

        // If there's a directory part
        let directory = if cmd_dir.len() > 1 {
            let dir_str = cmd_dir[1].trim();
            // Validate the directory
            if is_valid_directory(dir_str) {
                Some(dir_str.to_string())
            } else {
                None
            }
        } else {
            None
        };

        if !command.is_empty() {
            return Some(HistoryEntry {
                timestamp,
                command,
                directory,
                duration: None,
                exit_code: None,
            });
        }
    } else {
        // Last resort: treat as just a plain command
        let command = line.trim().to_string();
        if !command.is_empty() {
            return Some(HistoryEntry {
                timestamp: 0,
                command,
                directory: None,
                duration: None,
                exit_code: None,
            });
        }
    }

    None
}

// Helper function to validate if a string looks like a valid directory path
fn is_valid_directory(path: &str) -> bool {
    // Valid directories should:
    // 1. Start with / (absolute path) or ~ (home directory)
    // 2. Not contain characters that are invalid in paths

    if path.is_empty() {
        return false;
    }

    // Check if starts with / or ~
    if path.starts_with('/') || path.starts_with('~') {
        // Do additional checks to exclude URLs
        if path.contains("://") || path.contains("github.com") {
            return false;
        }
        return true;
    }

    false
}

pub fn get_history_entries() -> Result<Vec<HistoryEntry>> {
    // Try to read from CLI stats log first
    let stats_path = get_cli_stats_log_path()?;
    if let Ok(file) = File::open(&stats_path) {
        let reader = BufReader::new(file);
        let entries: Vec<HistoryEntry> = reader
            .lines()
            .filter_map(|line| line.ok())
            .filter_map(|line| parse_cli_stats_line(&line))
            .collect();

        if !entries.is_empty() {
            return Ok(entries);
        }
    }

    // Fall back to zsh history if stats log is empty or not available
    let history_path = get_zsh_history_path()?;
    let file = File::open(history_path).context("Failed to open zsh history file")?;
    let reader = BufReader::new(file);

    let entries: Vec<HistoryEntry> = reader
        .lines()
        .filter_map(|line| line.ok())
        .filter_map(|line| parse_history_line(&line))
        .collect();

    Ok(entries)
}

pub fn format_timestamp(timestamp: i64) -> String {
    if timestamp == 0 {
        return "Timestamp not available".to_string();
    }
    match Local.timestamp_opt(timestamp, 0) {
        chrono::LocalResult::Single(dt) => dt.format("%b %d %Y at %I:%M %P").to_string(),
        _ => "Invalid timestamp".to_string(),
    }
}
