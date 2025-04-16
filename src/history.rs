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

fn parse_history_line(line: &str) -> Vec<HistoryEntry> {
    let mut entries = Vec::new();
    // Handle Zsh history format: ": timestamp:0;command"
    if line.starts_with(": ") {
        let parts: Vec<&str> = line.splitn(3, ';').collect();
        if parts.len() < 2 {
            return entries;
        }
        let ts_part = match parts[0].strip_prefix(": ") {
            Some(s) => s.trim(),
            None => return entries,
        };
        let timestamp = match ts_part.splitn(2, ':').next().and_then(|s| s.parse().ok()) {
            Some(ts) => ts,
            None => return entries,
        };
        let command = parts[1].trim();
        for subcmd in command.split("&&") {
            let clean = subcmd.trim();
            if !clean.is_empty() {
                entries.push(HistoryEntry {
                    timestamp,
                    command: clean.to_string(),
                    directory: None,
                    duration: None,
                    exit_code: None,
                });
            }
        }
        return entries;
    } else {
        // Plain command
        for subcmd in line.trim().split("&&") {
            let clean = subcmd.trim();
            if !clean.is_empty() {
                entries.push(HistoryEntry {
                    timestamp: 0,
                    command: clean.to_string(),
                    directory: None,
                    duration: None,
                    exit_code: None,
                });
            }
        }
        return entries;
    }
}

fn parse_cli_stats_line(line: &str) -> Vec<HistoryEntry> {
    let mut entries = Vec::new();
    // Pipe-delimited format
    let pipe_parts: Vec<&str> = line.split('|').collect();
    if pipe_parts.len() == 3 {
        let timestamp = pipe_parts[0].parse::<i64>().unwrap_or(0);
        let directory = if is_valid_directory(pipe_parts[2].trim()) {
            Some(pipe_parts[2].trim().to_string())
        } else {
            None
        };
        for subcmd in pipe_parts[1].split("&&") {
            let clean = subcmd.trim();
            if !clean.is_empty() {
                entries.push(HistoryEntry {
                    timestamp,
                    command: clean.to_string(),
                    directory: directory.clone(),
                    duration: None,
                    exit_code: None,
                });
            }
        }
        return entries;
    }
    // Colon-delimited format
    if !line.starts_with(": ") {
        let parts: Vec<&str> = line.splitn(3, ':').collect();
        if parts.len() >= 3 && parts[0].chars().all(|c| c.is_digit(10)) {
            let timestamp = parts[0].parse::<i64>().unwrap_or(0);
            let dir_str = parts.last().unwrap().trim();
            let directory = if is_valid_directory(dir_str) {
                Some(dir_str.to_string())
            } else {
                None
            };
            let command_parts = &parts[1..parts.len() - 1];
            let command = command_parts.join(":").trim().to_string();
            for subcmd in command.split("&&") {
                let clean = subcmd.trim();
                if !clean.is_empty() {
                    entries.push(HistoryEntry {
                        timestamp,
                        command: clean.to_string(),
                        directory: directory.clone(),
                        duration: None,
                        exit_code: None,
                    });
                }
            }
            return entries;
        }
    }
    // Zsh history format
    if line.starts_with(": ") {
        let timestamp_part = line.strip_prefix(": ").unwrap_or("");
        let parts: Vec<&str> = timestamp_part.splitn(2, ';').collect();
        if parts.len() < 2 {
            return entries;
        }
        let ts_parts: Vec<&str> = parts[0].splitn(2, ':').collect();
        let timestamp = ts_parts[0].parse::<i64>().unwrap_or(0);
        let cmd_dir: Vec<&str> = parts[1].splitn(2, ':').collect();
        let directory = if cmd_dir.len() > 1 && is_valid_directory(cmd_dir[1].trim()) {
            Some(cmd_dir[1].trim().to_string())
        } else {
            None
        };
        for subcmd in cmd_dir[0].split("&&") {
            let clean = subcmd.trim();
            if !clean.is_empty() {
                entries.push(HistoryEntry {
                    timestamp,
                    command: clean.to_string(),
                    directory: directory.clone(),
                    duration: None,
                    exit_code: None,
                });
            }
        }
        return entries;
    } else {
        // Plain command
        for subcmd in line.trim().split("&&") {
            let clean = subcmd.trim();
            if !clean.is_empty() {
                entries.push(HistoryEntry {
                    timestamp: 0,
                    command: clean.to_string(),
                    directory: None,
                    duration: None,
                    exit_code: None,
                });
            }
        }
        return entries;
    }
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
            .flat_map(|line| parse_cli_stats_line(&line))
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
        .flat_map(|line| parse_history_line(&line))
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
