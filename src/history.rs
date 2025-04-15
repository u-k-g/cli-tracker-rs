use anyhow::{Context, Result};
use chrono::{Local, TimeZone};
use std::{
    fs::File,
    io::{self, BufRead, BufReader},
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
    let (timestamp, command_part) = if line.starts_with(": ") {
        let parts: Vec<&str> = line.splitn(3, ';').collect();
        if parts.len() < 2 {
            return None;
        }
        let ts_part = parts[0].strip_prefix(": ")?.trim();
        let timestamp = ts_part.splitn(2, ':').next()?.parse().ok()?;
        (timestamp, parts.get(1).unwrap_or(&"").to_string())
    } else {
        // For basic history entries, use a default timestamp (e.g., 0 or current time)
        (0, line.to_string()) // Or use chrono::Utc::now().timestamp()
    };

    let clean_command = command_part.trim().to_string();

    if !clean_command.is_empty() {
        // Extract directory from cd commands
        let directory = if clean_command.starts_with("cd ") {
            clean_command
                .strip_prefix("cd ")
                .map(|s| s.trim().to_string())
        } else {
            None
        };

        Some(HistoryEntry {
            timestamp,
            command: clean_command,
            directory,
            duration: None,  // We don't have duration info in zsh history
            exit_code: None, // We don't have exit code info in zsh history
        })
    } else {
        None
    }
}

fn parse_cli_stats_line(line: &str) -> Option<HistoryEntry> {
    // Format 1: ": timestamp:0;command:directory"
    // Format 2: "command" (just the command)
    // Format 3: ": timestamp:0;command" (no directory)

    if line.starts_with(": ") {
        // Format 1 or 3 with timestamp
        let timestamp_part = line.strip_prefix(": ")?;
        let parts: Vec<&str> = timestamp_part.splitn(2, ';').collect();
        if parts.len() < 2 {
            return None;
        }

        // Get timestamp from first part (timestamp:0)
        let ts_parts: Vec<&str> = parts[0].splitn(2, ':').collect();
        let timestamp = ts_parts[0].parse::<i64>().ok()?;

        // Get command and possibly directory
        let cmd_dir_parts: Vec<&str> = parts[1].splitn(2, ':').collect();
        let command = cmd_dir_parts[0].to_string();

        // If we have a directory part
        let directory = if cmd_dir_parts.len() > 1 {
            Some(cmd_dir_parts[1].to_string())
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
        // Format 2: just the command
        let command = line.trim().to_string();
        if !command.is_empty() {
            return Some(HistoryEntry {
                timestamp: 0, // No timestamp available
                command,
                directory: None,
                duration: None,
                exit_code: None,
            });
        }
    }

    None
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
