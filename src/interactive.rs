use anyhow::{Context, Result};
use chrono::{Datelike, Local, TimeZone, Timelike};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    style::{self, Color, Stylize},
    terminal::{self, ClearType},
};
use std::io::{self, Write};
use unicode_width::UnicodeWidthStr;

use crate::history::{format_timestamp, HistoryEntry};
use crate::ui_utils::{draw_box, write_in_box};

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
                0 => " ",
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

pub fn run_interactive_viewer(entries: Vec<HistoryEntry>) -> Result<()> {
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
                                if modifiers.contains(KeyModifiers::CONTROL) {
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
                        if modifiers.contains(KeyModifiers::CONTROL) {
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
