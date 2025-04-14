use anyhow::{Context, Result};
use chrono::{Datelike, Local, TimeZone, Timelike};
use clap::{Parser, Subcommand};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent},
    execute,
    style::{self, Color, Stylize},
    terminal::{self, ClearType},
};
use std::{
    fs::File,
    io::{self, BufRead, BufReader, Write},
    path::PathBuf,
};
use unicode_width::UnicodeWidthStr;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show command history in an interactive viewer
    History,
    /// Show summary statistics about command usage
    Stats,
}

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

fn get_history_entries() -> Result<Vec<HistoryEntry>> {
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

fn run_interactive_viewer(entries: Vec<HistoryEntry>) -> Result<()> {
    let mut stdout = io::stdout();
    execute!(stdout, terminal::EnterAlternateScreen)?;
    terminal::enable_raw_mode()?;
    execute!(stdout, cursor::Hide)?;

    let mut current_index = entries.len().saturating_sub(1);
    // Start directly in detail view mode with the most recent command
    let mut view_mode: Option<usize> = Some(current_index);

    // Theme colors
    let header_color = Color::Cyan;
    let selected_bg = Color::DarkBlue;
    let selected_fg = Color::White;
    let number_color = Color::DarkGrey;
    let separator_color = Color::DarkGrey;
    let command_color = Color::White;

    loop {
        if let Some(detail_index) = view_mode {
            // --- Detail View ---
            display_detail_view(&mut stdout, &entries[detail_index], &entries, detail_index)?;

            // Input handling for Detail View
            if let Event::Key(KeyEvent { code, .. }) = event::read()? {
                match code {
                    KeyCode::Char('q') | KeyCode::Esc => view_mode = None,
                    KeyCode::Up | KeyCode::Char('k') => {
                        // Navigate to previous command in history (newer)
                        if detail_index > 0 {
                            view_mode = Some(detail_index - 1);
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        // Navigate to next command in history (older)
                        if detail_index < entries.len() - 1 {
                            view_mode = Some(detail_index + 1);
                        }
                    }
                    KeyCode::Char('c') | KeyCode::Char('C') => {
                        if event::poll(std::time::Duration::from_millis(100))? {
                            if let Event::Key(KeyEvent {
                                code: KeyCode::Char('c'),
                                modifiers,
                                ..
                            }) = event::read()?
                            {
                                if modifiers.contains(event::KeyModifiers::CONTROL) {
                                    break;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        } else {
            // --- List View ---
            execute!(
                stdout,
                terminal::Clear(ClearType::All),
                cursor::MoveTo(0, 0)
            )?;
            let header = "Command History".with(header_color).bold();
            let controls = "(↑/k: up, ↓/j: down, Enter: details, q: quit)".with(Color::DarkGrey);
            writeln!(stdout, "{} {}\n", header, controls)?;

            let window_size = 10;
            let start_idx = current_index.saturating_sub(window_size / 2);
            let end_idx = (start_idx + window_size).min(entries.len());

            for (idx, entry) in entries[start_idx..end_idx].iter().enumerate() {
                let absolute_index = start_idx + idx;
                let line_num = entries.len() - absolute_index;
                let is_selected = absolute_index == current_index;

                execute!(stdout, cursor::MoveTo(0, (idx + 3) as u16))?;

                let prefix = if is_selected {
                    "▶".with(selected_fg).bold()
                } else {
                    " ".with(Color::Reset)
                };
                let num = format!("{:4}", line_num).with(number_color);
                let separator = "│".with(separator_color);

                let command_text = if is_selected {
                    execute!(stdout, style::SetBackgroundColor(selected_bg))?;
                    entry.command.as_str().with(selected_fg).bold()
                } else {
                    entry.command.as_str().with(command_color)
                };

                write!(stdout, "{} {} {} {}", prefix, num, separator, command_text)?;

                if is_selected {
                    execute!(stdout, style::ResetColor)?;
                }
            }
            stdout.flush()?;

            // Input handling for List View
            if let Event::Key(KeyEvent {
                code, modifiers, ..
            }) = event::read()?
            {
                match code {
                    KeyCode::Char('q') | KeyCode::Esc => break, // Exit the loop
                    KeyCode::Up | KeyCode::Char('k') => {
                        current_index = current_index.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        current_index = (current_index + 1).min(entries.len().saturating_sub(1));
                    }
                    KeyCode::Enter | KeyCode::Char('l') => {
                        view_mode = Some(current_index); // Switch to detail view
                    }
                    KeyCode::Char('h') => {
                        // In list view, 'h' doesn't do anything special
                    }
                    KeyCode::Char('c') => {
                        if modifiers.contains(event::KeyModifiers::CONTROL) {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Cleanup
    execute!(stdout, cursor::Show, terminal::LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    Ok(())
}

fn display_stats(entries: &[HistoryEntry]) -> Result<()> {
    let mut stdout = io::stdout();

    // Set up terminal
    execute!(stdout, terminal::EnterAlternateScreen)?;
    terminal::enable_raw_mode()?;
    execute!(stdout, cursor::Hide)?;

    // Track current view: -1 = lifetime stats, 0 = current week, 1 = last week, etc.
    let mut week_offset: i64 = -1;

    loop {
        // Get terminal size
        let (term_width, term_height) = terminal::size()?;

        // Check minimum terminal size requirements
        let min_width = 100;
        let min_height = 20;
        if term_width < min_width || term_height < min_height {
            execute!(
                stdout,
                terminal::Clear(ClearType::All),
                cursor::MoveTo(0, 0)
            )?;
            write!(
                stdout,
                "Terminal too small. Please resize to at least {}x{}",
                min_width, min_height
            )?;
            stdout.flush()?;

            // Wait for input and check if terminal has been resized
            if let Event::Key(KeyEvent {
                code, modifiers, ..
            }) = event::read()?
            {
                match code {
                    KeyCode::Esc => break,
                    KeyCode::Char('c') => {
                        if modifiers.contains(event::KeyModifiers::CONTROL) {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }

        // Clear screen
        execute!(stdout, terminal::Clear(ClearType::All))?;

        // Calculate line allocation based on available height
        // 1 line for header
        // 6 lines for borders (3 box layers * 2 border lines each)
        // Remaining lines for content

        // Total available height
        let usable_height = term_height;
        let header_lines = 1;
        let border_lines = 6; // 3 box layers * 2 border lines each

        // Calculate remaining lines for content
        let content_lines = usable_height
            .saturating_sub(header_lines)
            .saturating_sub(border_lines);

        // Time patterns content (reduced from 4 to 3 since we're removing a line)
        let time_patterns_min = 3;
        let time_patterns_max = 4;

        // Middle layer content (start with 3, max 10)
        let middle_layer_min = 3;
        let middle_layer_max = 10;

        // Apply priority-based allocation:
        // 1. Ensure we have enough lines for minimum allocation
        // 2. First allocate minimum to each layer
        // 3. Then grow Time Patterns to max if possible
        // 4. Then grow middle layer up to max
        // 5. Any extra goes to top layer (though it's capped at its max)

        // Update top layer max to 6 to accommodate additional content line
        let top_layer_max = 6; // Changed from 5 to 6

        // When terminal height is limited, reduce middle box height
        let adjusted_middle_layer_min = if term_height <= 20 {
            2 // Reduce by 1 when height is limited
        } else {
            middle_layer_min
        };

        let adjusted_middle_layer_max = if term_height <= 20 {
            middle_layer_max - 1 // Reduce max by 1 for limited height
        } else {
            middle_layer_max
        };

        // Start with minimum allocation using adjusted values
        let base_allocation = top_layer_max + adjusted_middle_layer_min + time_patterns_min;

        // Determine how many extra lines we have beyond base allocation
        let extra_lines = content_lines.saturating_sub(base_allocation).min(20); // Cap extra at 20 to avoid excessive growth

        // Allocate additional lines according to priority
        let time_patterns_extra = (time_patterns_max - time_patterns_min).min(extra_lines);
        let time_patterns_content = time_patterns_min + time_patterns_extra;

        let middle_extra = if extra_lines > time_patterns_extra {
            (adjusted_middle_layer_max - adjusted_middle_layer_min)
                .min(extra_lines - time_patterns_extra)
        } else {
            0
        };
        let middle_layer_content = adjusted_middle_layer_min + middle_extra;

        // Top layer stays at max (already allocated in base_allocation)
        let top_layer_content = top_layer_max;

        // Calculate box heights (content + borders)
        let top_box_height = top_layer_content + 2; // +2 for borders
        let middle_box_height = middle_layer_content + 2; // +2 for borders
        let bottom_box_height = time_patterns_content + 2; // +2 for borders

        // Set command list limits based on available space
        let commands_box_height = middle_box_height;
        let max_commands = middle_layer_content as usize;
        let max_categories = max_commands;

        // Calculate widths to use the full terminal width
        // Account for the border between columns (1 character)
        let usable_width = term_width;
        let half_width = usable_width / 2;

        // Calculate precise widths for left and right boxes
        let left_box_width = half_width;
        let right_box_width = usable_width - half_width;

        // Define the active entries based on current view
        let (view_name, active_entries): (String, Vec<&HistoryEntry>) = if week_offset < 0 {
            // Lifetime stats view
            ("All-time Stats".to_string(), entries.iter().collect())
        } else {
            // Week-specific view
            let now = chrono::Local::now();

            // Calculate the start of the current week (Monday at 00:00:00)
            let days_since_monday = now.weekday().num_days_from_monday() as i64;
            let start_of_week = now
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_local_timezone(chrono::Local)
                .unwrap()
                - chrono::Duration::days(days_since_monday)
                - chrono::Duration::days(7 * week_offset);

            // End of week is start of next week minus 1 second
            let end_of_week =
                start_of_week + chrono::Duration::days(7) - chrono::Duration::seconds(1);

            // Get ISO week number of the year (1-52/53)
            let week_number = start_of_week.iso_week().week();

            // Format month abbreviation
            let month_name = start_of_week.format("%b").to_string();

            // Create view name in format "Week # [Month]"
            let view_name = format!("Week {} [{}]", week_number, month_name);

            // Filter entries for specific week
            let week_entries = entries
                .iter()
                .filter(|e| {
                    let ts = e.timestamp;
                    ts >= start_of_week.timestamp() && ts <= end_of_week.timestamp()
                })
                .collect();

            (view_name, week_entries)
        };

        // Header with view name
        execute!(stdout, cursor::MoveTo(0, 0))?;

        // Get the terminal width to properly center the controls text
        let controls_text = "<←/h: prev, →/l: next, esc/q: exit>".dark_grey();
        let left_text = format!("CLI Wrapped: {}", view_name).cyan().bold();
        let right_text = format!("commands: {}", active_entries.len()).cyan();

        // Calculate positions to ensure proper centering
        let right_start = term_width.saturating_sub(right_text.to_string().width() as u16);
        let center_start = half_width - (controls_text.to_string().width() as u16 / 2);

        // Write the left part
        write!(stdout, "{}", left_text)?;

        // Write the centered controls
        execute!(stdout, cursor::MoveTo(center_start, 0))?;
        write!(stdout, "{}", controls_text)?;

        // Write the right part
        execute!(stdout, cursor::MoveTo(right_start, 0))?;
        write!(stdout, "{}", right_text)?;

        // Calculate time span and metrics for the active view
        let oldest = active_entries
            .iter()
            .map(|e| e.timestamp)
            .filter(|&ts| ts > 0)
            .min()
            .unwrap_or(0);
        let newest = active_entries
            .iter()
            .map(|e| e.timestamp)
            .filter(|&ts| ts > 0)
            .max()
            .unwrap_or(0);
        let days = if newest > 0 && oldest > 0 {
            ((newest - oldest) / 86400) + 1
        } else if active_entries.len() > 0 {
            // If we have entries but no valid timestamps, assume at least 1 day
            1
        } else {
            0
        };

        // Count commands with valid timestamps
        let commands_with_timestamps = active_entries.iter().filter(|e| e.timestamp > 0).count();

        // Total commands including those without timestamps
        let total_commands = active_entries.len();

        // Time metrics for the current week
        let now = chrono::Local::now();

        // For specific week view, calculate the start/end of the selected week
        let (this_week_start, this_week_end) = if week_offset >= 0 {
            let days_since_monday = now.weekday().num_days_from_monday() as i64;
            let start_of_week = now
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_local_timezone(chrono::Local)
                .unwrap()
                - chrono::Duration::days(days_since_monday)
                - chrono::Duration::days(7 * week_offset);

            let end_of_week =
                start_of_week + chrono::Duration::days(7) - chrono::Duration::seconds(1);

            (start_of_week.timestamp(), end_of_week.timestamp())
        } else {
            // For all-time view, use current week
            let days_since_monday = now.weekday().num_days_from_monday() as i64;
            let start_of_week = now
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_local_timezone(chrono::Local)
                .unwrap()
                - chrono::Duration::days(days_since_monday);

            let end_of_week =
                start_of_week + chrono::Duration::days(7) - chrono::Duration::seconds(1);

            (start_of_week.timestamp(), end_of_week.timestamp())
        };

        // For specific week view, calculate the start/end of the month containing the selected week
        let (this_month_start, this_month_end) = if week_offset >= 0 {
            let days_since_monday = now.weekday().num_days_from_monday() as i64;
            let selected_week_day = now
                - chrono::Duration::days(days_since_monday)
                - chrono::Duration::days(7 * week_offset);

            let start_of_month = selected_week_day
                .with_day(1)
                .unwrap()
                .with_hour(0)
                .unwrap()
                .with_minute(0)
                .unwrap()
                .with_second(0)
                .unwrap();

            // End of month is start of next month minus 1 second
            let next_month = if start_of_month.month() == 12 {
                start_of_month
                    .with_month(1)
                    .unwrap()
                    .with_year(start_of_month.year() + 1)
                    .unwrap()
            } else {
                start_of_month
                    .with_month(start_of_month.month() + 1)
                    .unwrap()
            };

            let end_of_month = next_month - chrono::Duration::seconds(1);

            (start_of_month.timestamp(), end_of_month.timestamp())
        } else {
            // For all-time view, use current month
            let start_of_month = now
                .with_day(1)
                .unwrap()
                .with_hour(0)
                .unwrap()
                .with_minute(0)
                .unwrap()
                .with_second(0)
                .unwrap();

            // End of month is start of next month minus 1 second
            let next_month = if start_of_month.month() == 12 {
                start_of_month
                    .with_month(1)
                    .unwrap()
                    .with_year(start_of_month.year() + 1)
                    .unwrap()
            } else {
                start_of_month
                    .with_month(start_of_month.month() + 1)
                    .unwrap()
            };

            let end_of_month = next_month - chrono::Duration::seconds(1);

            (start_of_month.timestamp(), end_of_month.timestamp())
        };

        // Get today's date for the "today" metric
        let today_start = now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(chrono::Local)
            .unwrap()
            .timestamp();

        // Count commands for different time periods, specific to the view
        let commands_today = entries
            .iter()
            .filter(|e| e.timestamp >= today_start)
            .count();

        let commands_this_week = entries
            .iter()
            .filter(|e| e.timestamp >= this_week_start && e.timestamp <= this_week_end)
            .count();

        let commands_this_month = entries
            .iter()
            .filter(|e| e.timestamp >= this_month_start && e.timestamp <= this_month_end)
            .count();

        // Top Left Box - General Statistics
        draw_box(
            &mut stdout,
            0,
            1,
            left_box_width,
            top_box_height,
            Some("General Statistics"),
        )?;

        // Different stats depending on view
        let general_stats = if week_offset < 0 {
            // Lifetime stats
            [
                ("Today", commands_today.to_string()),
                ("This week", commands_this_week.to_string()),
                ("This month", commands_this_month.to_string()),
                ("Weekly average", {
                    if days == 0 {
                        "0".to_string()
                    } else {
                        // Calculate weeks since first command
                        let weeks = (days as f64 / 7.0).ceil().max(1.0);
                        // Use commands_with_timestamps for accurate time-based average
                        format!("{:.1}", commands_with_timestamps as f64 / weeks)
                    }
                }),
                (
                    "Unique commands",
                    active_entries
                        .iter()
                        .map(|e| &e.command)
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        .to_string(),
                ),
                ("Usage trend", {
                    if week_offset < 0 && active_entries.len() > 10 {
                        // For lifetime view, show if usage is increasing or decreasing
                        let halfway = active_entries.len() / 2;
                        let old_half_count = active_entries.iter().take(halfway).count();
                        let new_half_count = active_entries.len() - old_half_count;

                        if new_half_count > old_half_count * 12 / 10 {
                            "↑ Increasing".to_string()
                        } else if new_half_count < old_half_count * 8 / 10 {
                            "↓ Decreasing".to_string()
                        } else {
                            "→ Steady".to_string()
                        }
                    } else {
                        "N/A".to_string()
                    }
                }),
            ]
        } else {
            // Weekly stats
            [
                ("Today", commands_today.to_string()),
                ("This week", commands_this_week.to_string()),
                ("This month", commands_this_month.to_string()),
                ("Commands per day", {
                    if days > 0 {
                        format!("{:.1}", active_entries.len() as f64 / days as f64)
                    } else {
                        "0".to_string()
                    }
                }),
                (
                    "Unique commands",
                    active_entries
                        .iter()
                        .map(|e| &e.command)
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        .to_string(),
                ),
                ("Usage trend", {
                    if active_entries.len() > 10 {
                        // For weekly view, compare to previous week
                        let prev_week_start = this_week_start - 86400 * 7;
                        let prev_week_end = this_week_end - 86400 * 7;

                        let prev_week_count = entries
                            .iter()
                            .filter(|e| {
                                e.timestamp >= prev_week_start && e.timestamp <= prev_week_end
                            })
                            .count();

                        if commands_this_week > prev_week_count * 12 / 10 {
                            "↑ Increasing".to_string()
                        } else if commands_this_week < prev_week_count * 8 / 10 {
                            "↓ Decreasing".to_string()
                        } else {
                            "→ Steady".to_string()
                        }
                    } else {
                        "N/A".to_string()
                    }
                }),
            ]
        };

        for (i, (key, value)) in general_stats.iter().enumerate() {
            execute!(stdout, cursor::MoveTo(3, 2 + i as u16))?;
            write!(stdout, "{:<14} {}", key.with(Color::DarkGrey), value)?;
        }

        // Top Right Box - Activity Breakdown
        draw_box(
            &mut stdout,
            left_box_width,
            1,
            right_box_width,
            top_box_height,
            Some("Activity Breakdown"),
        )?;

        // Activity stats moved to here
        let activity_stats = [
            (
                "First command",
                if oldest > 0 {
                    format_timestamp(oldest)
                } else {
                    "N/A".to_string()
                },
            ),
            (
                "Last command",
                if newest > 0 {
                    format_timestamp(newest)
                } else {
                    "N/A".to_string()
                },
            ),
        ];

        for (i, (key, value)) in activity_stats.iter().enumerate() {
            execute!(stdout, cursor::MoveTo(left_box_width + 3, 2 + i as u16))?;
            write!(stdout, "{:<14} {}", key.with(Color::DarkGrey), value)?;
        }

        // Middle Left Box - Most Used Commands
        draw_box(
            &mut stdout,
            0,
            top_box_height + 1,
            left_box_width,
            commands_box_height,
            Some("Most Used Commands"),
        )?;

        // Count command frequency
        let mut command_counts: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for entry in &active_entries {
            *command_counts.entry(&entry.command).or_insert(0) += 1;
        }

        // Sort by frequency
        let mut command_counts: Vec<_> = command_counts.into_iter().collect();
        command_counts.sort_by(|a, b| b.1.cmp(&a.1));

        // Calculate how many commands we can show based on available space
        // Display top commands (limited by max_commands)
        for (i, (cmd, count)) in command_counts.iter().take(max_commands).enumerate() {
            let display_width = left_box_width.saturating_sub(15) as usize;
            let truncated_cmd = if cmd.len() > display_width {
                format!("{}...", &cmd[0..display_width - 3])
            } else {
                cmd.to_string()
            };

            execute!(stdout, cursor::MoveTo(3, top_box_height + 2 + i as u16))?;
            write!(stdout, "{:2}. {} ", i + 1, truncated_cmd)?;

            execute!(
                stdout,
                cursor::MoveTo(left_box_width - 10, top_box_height + 2 + i as u16)
            )?;
            write!(stdout, "{}", count.to_string().with(Color::DarkGrey))?;
        }

        // Middle Right Box - Command Categories
        draw_box(
            &mut stdout,
            left_box_width,
            top_box_height + 1,
            right_box_width,
            commands_box_height,
            Some("Command Categories"),
        )?;

        let mut categories: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for entry in &active_entries {
            let first_word = entry.command.split_whitespace().next().unwrap_or("other");
            *categories.entry(first_word).or_insert(0) += 1;
        }

        // Sort by frequency
        let mut categories: Vec<_> = categories.into_iter().collect();
        categories.sort_by(|a, b| b.1.cmp(&a.1));

        // Display top categories with percentage bars (limited by max_categories)
        for (i, (category, count)) in categories.iter().take(max_categories).enumerate() {
            let percentage = if active_entries.is_empty() {
                0
            } else {
                (*count as f64 / active_entries.len() as f64 * 100.0) as usize
            };

            // Ensure we have a fixed width for the category name
            let category_display = if category.len() > 10 {
                format!("{}...", &category[..7])
            } else {
                format!("{:<10}", category)
            };

            execute!(
                stdout,
                cursor::MoveTo(left_box_width + 3, top_box_height + 2 + i as u16)
            )?;
            write!(stdout, "{} ", category_display)?;

            // Calculate bar width based on available space
            let max_bar_width = (right_box_width as usize).saturating_sub(20);
            let bar_width = (percentage * max_bar_width / 100).min(max_bar_width);
            // Use a clearer bar character for better visibility
            let dots = "█".repeat(bar_width);
            write!(stdout, "{} {}%", dots, percentage)?;
        }

        // Bottom Box - Time Patterns
        let bottom_y = 1 + top_box_height + commands_box_height;
        draw_box(
            &mut stdout,
            0,
            bottom_y,
            usable_width, // Use the full width for the bottom box
            bottom_box_height,
            Some("Time Patterns"),
        )?;

        // Count by hour of day
        let mut hour_counts = vec![0; 24];
        for entry in active_entries.iter().filter(|e| e.timestamp > 0) {
            let dt = Local.timestamp_opt(entry.timestamp, 0);
            if let chrono::LocalResult::Single(dt) = dt {
                let hour = dt.hour() as usize;
                if hour < 24 {
                    hour_counts[hour] += 1;
                }
            }
        }

        // Calculate average usage per hour
        let total_usage: i32 = hour_counts.iter().sum();
        let active_hours = hour_counts.iter().filter(|&&count| count > 0).count();
        let avg_usage = if active_hours > 0 {
            total_usage as f64 / active_hours as f64
        } else {
            0.0
        };

        // Draw hourly pattern with +/- style
        execute!(stdout, cursor::MoveTo(3, bottom_y + 1))?;
        write!(stdout, "Hourly activity: ")?;

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
        write!(stdout, "{}", hour_viz)?;

        // Find peak hour of day
        let (peak_hour, peak_count) = hour_counts
            .iter()
            .enumerate()
            .max_by_key(|&(_, count)| count)
            .unwrap_or((0, &0));

        // Find peak day of week
        let mut day_of_week_counts = vec![0; 7];

        // Filter to get only entries with valid timestamps
        let entries_with_timestamps: Vec<&HistoryEntry> = active_entries
            .iter()
            .filter(|e| e.timestamp > 0)
            .copied()
            .collect();

        for entry in &entries_with_timestamps {
            let dt = Local.timestamp_opt(entry.timestamp, 0);
            if let chrono::LocalResult::Single(dt) = dt {
                let weekday = dt.weekday().num_days_from_monday() as usize;
                if weekday < 7 {
                    day_of_week_counts[weekday] += 1;
                }
            }
        }

        let (peak_day_idx, peak_day_count) = day_of_week_counts
            .iter()
            .enumerate()
            .max_by_key(|&(_, count)| count)
            .unwrap_or((0, &0));

        let weekdays = [
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
            "Sunday",
        ];
        let peak_day = weekdays[peak_day_idx];

        // Display peak times with consistent spacing
        execute!(stdout, cursor::MoveTo(3, bottom_y + 2))?;
        if *peak_count > 0 {
            write!(
                stdout,
                "Peak hour: {:02}:00 ({} commands)",
                peak_hour, peak_count
            )?;
        } else {
            write!(stdout, "Peak hour: None")?;
        }

        execute!(stdout, cursor::MoveTo(3, bottom_y + 3))?;
        if *peak_day_count > 0 {
            write!(
                stdout,
                "Peak day: {} ({} commands)",
                peak_day, peak_day_count
            )?;
        } else {
            write!(stdout, "Peak day: None")?;
        }

        // Day of week distribution with better alignment
        execute!(stdout, cursor::MoveTo(3, bottom_y + 4))?;
        write!(stdout, "Day distribution: ")?;

        let days = ["M", "T", "W", "T", "F", "S", "S"];
        let distribution_start_x = 22; // Slightly adjust the starting position
        let day_spacing = 7; // Consistent spacing between day percentages

        // Calculate total from day_of_week_counts to ensure percentages add up to 100%
        let total_days_count: usize = day_of_week_counts.iter().sum();

        for (i, &count) in day_of_week_counts.iter().enumerate() {
            let percentage = if total_days_count == 0 {
                0
            } else {
                count * 100 / total_days_count.max(1)
            };
            execute!(
                stdout,
                cursor::MoveTo(distribution_start_x + i as u16 * day_spacing, bottom_y + 4)
            )?;
            write!(stdout, "{}:{}%", days[i], percentage)?;
        }

        // Wait for user input
        stdout.flush()?;

        // Handle key presses
        match event::read()? {
            Event::Key(KeyEvent {
                code: KeyCode::Esc, ..
            })
            | Event::Key(KeyEvent {
                code: KeyCode::Char('q'),
                ..
            }) => break,
            Event::Key(KeyEvent {
                code: KeyCode::Left,
                ..
            })
            | Event::Key(KeyEvent {
                code: KeyCode::Char('h'),
                ..
            }) => {
                // Go back (all-time -> current week -> previous weeks)
                if week_offset < 0 {
                    // When in all-time view, switch to current week
                    week_offset = 0;
                } else {
                    // When in a week view, go back one week (increase offset)
                    week_offset += 1;
                }
                continue; // Force immediate refresh of the display
            }
            Event::Key(KeyEvent {
                code: KeyCode::Right,
                ..
            })
            | Event::Key(KeyEvent {
                code: KeyCode::Char('l'),
                ..
            }) => {
                // Go forward (previous weeks -> current week -> all-time)
                if week_offset > 0 {
                    // When viewing past weeks, move forward one week (decrease offset)
                    week_offset -= 1;
                } else if week_offset == 0 {
                    // When viewing current week, go to all-time view
                    week_offset = -1;
                }
                continue; // Force immediate refresh of the display
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers,
                ..
            }) => {
                if modifiers.contains(event::KeyModifiers::CONTROL) {
                    break;
                }
            }
            _ => {}
        }
    }

    // Clean up
    execute!(stdout, cursor::Show, terminal::LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    Ok(())
}

// Add this function before main to test our parsing logic
fn test_cli_stats_parsing() {
    println!("Testing .cli_stats_log parsing:");

    let test_lines = [
        ": 1744408370:0;bat ~/.cli_stats_log:/Users/uzair/01-dev/cli-wrapped",
        "bunx create-expo-app@latest",
        ": 1744341672:0;cargo run -- history",
    ];

    for (i, line) in test_lines.iter().enumerate() {
        println!("\nTest case {}: {}", i + 1, line);
        if let Some(entry) = parse_cli_stats_line(line) {
            println!("✅ Parsed successfully:");
            println!("  Timestamp: {}", entry.timestamp);
            println!("  Command: {}", entry.command);
            println!("  Directory: {:?}", entry.directory);
        } else {
            println!("❌ Failed to parse");
        }
    }
    println!("\nParsing test completed\n");
}

#[tokio::main]
async fn main() -> Result<()> {
    // Test the parsing function
    test_cli_stats_parsing();

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
    }

    Ok(())
}
