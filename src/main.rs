use anyhow::{Context, Result};
use chrono::{Local, TimeZone, Timelike};
use clap::Parser;
use crossterm::{
    cursor, execute,
    style::{Color, Stylize},
    terminal::{self, ClearType},
};
use std::{
    io::{self, Write},
    path::PathBuf,
};
use unicode_width::UnicodeWidthStr;

// Declare modules
mod cli;
mod days;
mod history;
mod interactive;
mod stats;
mod ui_utils;
// Use items from modules
use cli::{Cli, Commands};
use days::display_today_stats;
use history::get_history_entries;
use interactive::run_interactive_viewer;
use stats::display_stats;

#[derive(Debug, Clone)]
struct HistoryEntry {
    timestamp: i64,
    command: String,
    directory: Option<String>,
    duration: Option<i64>,
    exit_code: Option<i32>,
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

fn format_timestamp(timestamp: i64) -> String {
    if timestamp == 0 {
        return "Timestamp not available".to_string();
    }
    match Local.timestamp_opt(timestamp, 0) {
        chrono::LocalResult::Single(dt) => dt.format("%b %d %Y at %I:%M %P").to_string(),
        _ => "Invalid timestamp".to_string(),
    }
}

// Define box drawing characters
const TOP_LEFT: &str = "┌";
const TOP_RIGHT: &str = "┐";
const BOTTOM_LEFT: &str = "└";
const BOTTOM_RIGHT: &str = "┘";
const HORIZONTAL: &str = "─";
const VERTICAL: &str = "│";

// Helper function to draw a box
fn draw_box(
    stdout: &mut io::Stdout,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    title: Option<&str>,
) -> Result<()> {
    // Ensure minimum dimensions for a proper box
    let width = width.max(4); // Minimum width to draw a proper box
    let height = height.max(3); // Minimum height for a proper box

    // Draw top border with optional title
    execute!(stdout, cursor::MoveTo(x, y))?;
    write!(stdout, "{}", TOP_LEFT)?;

    if let Some(title_text) = title {
        let title_display = format!(" {} ", title_text);
        let title_width = title_display.width();
        // Ensure we have enough space for title and borders
        let remaining_width = width as usize - 2;

        if title_width < remaining_width {
            let left_border = (remaining_width - title_width) / 2;
            let right_border = remaining_width - left_border - title_width;

            write!(stdout, "{}", HORIZONTAL.repeat(left_border))?;
            write!(stdout, "{}", title_display.cyan())?;
            write!(stdout, "{}", HORIZONTAL.repeat(right_border))?;
        } else {
            // Title too long, just draw border
            write!(stdout, "{}", HORIZONTAL.repeat(remaining_width))?;
        }
    } else {
        write!(stdout, "{}", HORIZONTAL.repeat((width - 2) as usize))?;
    }

    write!(stdout, "{}", TOP_RIGHT)?;

    // Draw sides
    for i in 1..height - 1 {
        execute!(stdout, cursor::MoveTo(x, y + i))?;
        write!(stdout, "{}", VERTICAL)?;
        execute!(stdout, cursor::MoveTo(x + width - 1, y + i))?;
        write!(stdout, "{}", VERTICAL)?;
    }

    // Draw bottom
    execute!(stdout, cursor::MoveTo(x, y + height - 1))?;
    write!(stdout, "{}", BOTTOM_LEFT)?;
    write!(stdout, "{}", HORIZONTAL.repeat((width - 2) as usize))?;
    write!(stdout, "{}", BOTTOM_RIGHT)?;

    Ok(())
}

// Write text inside a box area with an x offset
fn write_in_box(stdout: &mut io::Stdout, x: u16, y: u16, text: &str, x_offset: u16) -> Result<()> {
    execute!(stdout, cursor::MoveTo(x + 1 + x_offset, y))?;
    write!(stdout, "{}", text)?;
    Ok(())
}

fn display_detail_view(
    stdout: &mut io::Stdout,
    entry: &HistoryEntry,
    entries: &[HistoryEntry],
    current_index: usize,
) -> Result<()> {
    // Clear screen first
    execute!(stdout, terminal::Clear(ClearType::All))?;

    // Get terminal size
    let (term_width, term_height) = terminal::size()?;

    // Ensure minimum size requirements
    let min_width = 80;
    let min_height = 24;
    if term_width < min_width || term_height < min_height {
        execute!(stdout, cursor::MoveTo(0, 0))?;
        write!(
            stdout,
            "Terminal too small. Please resize to at least {}x{}",
            min_width, min_height
        )?;
        stdout.flush()?;
        return Ok(());
    }

    // Correctly assign previous and next commands
    // Previous command comes before current (is newer, has lower index)
    let prev_cmd = if current_index > 0 {
        &entries[current_index - 1].command
    } else {
        "No previous command"
    };

    // Next command comes after current (is older, has higher index)
    let next_cmd = if current_index < entries.len() - 1 {
        &entries[current_index + 1].command
    } else {
        "No next command"
    };

    // Header
    execute!(stdout, cursor::MoveTo(0, 0))?;
    write!(
        stdout,
        "{}                                                                    {: <67}                                                               {}",
        "CLI Wrapped".cyan().bold(),
        "<esc>: back, ↑/↓: navigate".dark_grey(),
        format!("history count: {}", entries.len()).cyan()
    )?;

    // Command navigation section - top row with 3 boxes
    let box_height = 5;
    let prev_width = term_width / 3;
    let cmd_width = term_width / 3;
    let next_width = term_width - prev_width - cmd_width;

    // Previous command box (older command) - normal styling
    draw_box(
        stdout,
        1,
        2,
        prev_width,
        box_height,
        Some("Previous command"),
    )?;

    // Write previous command with normal color
    write_in_box(stdout, 1, 3, prev_cmd, 1)?;

    // Current command box
    draw_box(
        stdout,
        prev_width + 1,
        2,
        cmd_width,
        box_height,
        Some("Command"),
    )?;
    write_in_box(stdout, prev_width + 1, 3, &entry.command, 1)?;

    // Next command box (newer command) - normal styling
    draw_box(
        stdout,
        prev_width + cmd_width + 1,
        2,
        next_width - 1, // Adjust width to fix alignment
        box_height,
        Some("Next command"),
    )?;

    // Write next command with normal color
    write_in_box(stdout, prev_width + cmd_width + 1, 3, next_cmd, 1)?;

    // Calculate command stats
    let username = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());
    let hostname = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "unknown".to_string());

    // Find the current working directory
    let current_dir = if let Some(dir) = &entry.directory {
        dir.clone()
    } else {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string())
    };

    // Count how many times this command appears in history
    let total_runs = entries
        .iter()
        .filter(|e| e.command == entry.command)
        .count();

    // Calculate command position in history (starting from 1 for oldest)
    let history_position = current_index + 1;

    // Gather real stats from the history entries and environment
    let stats = [
        ("History number", history_position.to_string()),
        ("User", username),
        ("Time", format_timestamp(entry.timestamp)),
        ("Directory", current_dir),
        ("Total runs", total_runs.to_string()),
        (
            "Recent runs",
            format!(
                "{}",
                entries
                    .iter()
                    .filter(|e| e.command == entry.command && e.timestamp > entry.timestamp - 86400)
                    .count()
            ),
        ),
    ];

    // Command stats box - left column
    let stats_height = 14;
    let stats_width = term_width / 2;
    draw_box(
        stdout,
        1,
        box_height + 2,
        stats_width,
        stats_height,
        Some("Command stats"),
    )?;

    for (i, (key, value)) in stats.iter().enumerate() {
        let line = box_height + 4 + i as u16;
        execute!(stdout, cursor::MoveTo(3, line))?;
        write!(stdout, "{:<14} {}", key.with(Color::DarkGrey), value)?;
    }

    // List of similar commands - right top
    draw_box(
        stdout,
        stats_width + 1,
        box_height + 2,
        term_width - stats_width - 1,
        5,
        Some("Similar commands"),
    )?;

    // Find similar commands (commands that start with the same word)
    let first_word = entry.command.split_whitespace().next().unwrap_or("");
    let similar_commands: Vec<&HistoryEntry> = entries
        .iter()
        .filter(|e| {
            let e_first_word = e.command.split_whitespace().next().unwrap_or("");
            e_first_word == first_word && e.command != entry.command
        })
        .take(3)
        .collect();

    for (i, similar) in similar_commands.iter().enumerate() {
        let line = box_height + 3 + i as u16;
        let display = if similar.command.len() > 40 {
            format!("{}...", &similar.command[..37])
        } else {
            similar.command.clone()
        };
        write_in_box(stdout, stats_width + 1, line, &display, 1)?;
    }

    // Command frequency by hour - right middle
    draw_box(
        stdout,
        stats_width + 1,
        box_height + 7,
        term_width - stats_width - 1,
        4,
        Some("Command frequency by hour"),
    )?;

    // Count commands by hour of day (based on timestamps)
    let mut hour_counts = vec![0; 24];
    for e in entries
        .iter()
        .filter(|e| e.command == entry.command && e.timestamp > 0)
    {
        let dt = Local.timestamp_opt(e.timestamp, 0);
        if let chrono::LocalResult::Single(dt) = dt {
            let hour = dt.hour() as usize;
            if hour < 24 {
                hour_counts[hour] += 1;
            }
        }
    }

    // Find max for scaling
    let max_count = hour_counts.iter().max().copied().unwrap_or(1);

    // Calculate average usage
    let total_usage: i32 = hour_counts.iter().sum();
    let active_hours = hour_counts.iter().filter(|&&count| count > 0).count();
    let avg_usage = if active_hours > 0 {
        total_usage as f64 / active_hours as f64
    } else {
        0.0
    };

    // Create a simpler +/- visualization where + is above average and - is below
    let mut hour_viz = String::new();
    hour_viz.push_str("[Hours] ");
    for i in 0..24 {
        // Use - for below average, + for above average, · for zeros
        let symbol = if hour_counts[i] == 0 {
            "·"
        } else if (hour_counts[i] as f64) < avg_usage {
            "-"
        } else {
            "+"
        };
        hour_viz.push_str(symbol);
    }

    write_in_box(stdout, stats_width + 1, box_height + 8, &hour_viz, 1)?;
    write_in_box(
        stdout,
        stats_width + 1,
        box_height + 9,
        &format!(
            "Peak times: {}",
            hour_counts
                .iter()
                .enumerate()
                .filter(|(_, &count)| count > 2 * max_count / 3)
                .map(|(hour, _)| format!("{:02}:00", hour))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        1,
    )?;

    // Command usage over time - right bottom
    draw_box(
        stdout,
        stats_width + 1,
        box_height + 11,
        term_width - stats_width - 1,
        5,
        Some("Command usage over time"),
    )?;

    // Group commands by day for a simple timeline
    let mut days = std::collections::HashMap::new();
    for e in entries
        .iter()
        .filter(|e| e.command == entry.command && e.timestamp > 0)
    {
        let dt = Local.timestamp_opt(e.timestamp, 0);
        if let chrono::LocalResult::Single(dt) = dt {
            let day = dt.format("%m/%d").to_string();
            *days.entry(day).or_insert(0) += 1;
        }
    }

    // Sort by date and take most recent
    let mut days: Vec<(String, i32)> = days.into_iter().collect();
    days.sort_by(|a, b| b.0.cmp(&a.0)); // Sort descending by date
    days.truncate(7); // Keep only the 7 most recent days
    days.reverse(); // Show oldest to newest

    // Create a sparkline-style visualization
    let max_day_count = days.iter().map(|(_, count)| *count).max().unwrap_or(1);
    let days_viz = days
        .iter()
        .map(|(day, count)| {
            let intensity = (*count as f64 / max_day_count as f64 * 5.0).round() as usize;
            let symbol = match intensity {
                0 => "▁",
                1 => "▂",
                2 => "▃",
                3 => "▄",
                4 => "▅",
                _ => "▆",
            };
            format!("{}: {}", day, symbol)
        })
        .collect::<Vec<_>>()
        .join("  ");

    write_in_box(stdout, stats_width + 1, box_height + 12, &days_viz, 1)?;

    // Show most frequent day
    if !days.is_empty() {
        let most_frequent = days
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(day, count)| format!("Most active: {} ({} times)", day, count))
            .unwrap_or_else(|| "No data".to_string());
        write_in_box(stdout, stats_width + 1, box_height + 13, &most_frequent, 1)?;
    }

    // Footer
    execute!(stdout, cursor::MoveTo(1, term_height - 1))?;
    write!(stdout, "Press ESC to go back to command list")?;

    stdout.flush().context("Failed to flush stdout")
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::History => {
            let entries = get_history_entries()?;
            run_interactive_viewer(entries)?;
        }
        Commands::Stats => {
            let entries = get_history_entries()?;
            display_stats(&entries)?;
        }
        Commands::Today => {
            let entries = get_history_entries()?;
            display_today_stats(&entries)?;
        }
    }

    Ok(())
}
