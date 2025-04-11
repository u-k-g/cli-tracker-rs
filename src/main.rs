use anyhow::{Context, Result};
use chrono::{DateTime, Local, TimeZone};
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
}

#[derive(Debug, Clone)]
struct HistoryEntry {
    timestamp: i64,
    command: String,
}

fn get_zsh_history_path() -> Result<PathBuf> {
    let home = home::home_dir().context("Could not find home directory")?;
    Ok(home.join(".zsh_history"))
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
        Some(HistoryEntry {
            timestamp,
            command: clean_command,
        })
    } else {
        None
    }
}

fn get_history_entries() -> Result<Vec<HistoryEntry>> {
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
    // Draw top border with optional title
    execute!(stdout, cursor::MoveTo(x, y))?;
    write!(stdout, "{}", TOP_LEFT)?;

    if let Some(title_text) = title {
        let title_display = format!(" {} ", title_text);
        let remaining_width = width as usize - 2 - title_display.width();
        let left_border = remaining_width / 2;
        let right_border = remaining_width - left_border;

        write!(stdout, "{}", HORIZONTAL.repeat(left_border))?;
        write!(stdout, "{}", title_display.cyan())?;
        write!(stdout, "{}", HORIZONTAL.repeat(right_border))?;
    } else {
        write!(stdout, "{}", HORIZONTAL.repeat((width - 2) as usize))?;
    }

    write!(stdout, "{}", TOP_RIGHT)?;

    // Draw sides
    for i in 1..height {
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
        "CLI Tracker".cyan().bold(),
        "<esc>: back, ↑/↓: navigate".dark_grey(),
        format!("history count: {}", entries.len()).cyan()
    )?;

    execute!(stdout, cursor::MoveTo(2, 1))?;
    write!(
        stdout,
        "{} {} {}",
        "Search".dark_grey(),
        VERTICAL,
        "Inspect".bold()
    )?;

    // Command navigation section - top row with 3 boxes
    let box_height = 5;
    let prev_width = term_width / 3;
    let cmd_width = term_width / 3;
    let next_width = term_width - prev_width - cmd_width;

    // Previous command box (older command)
    draw_box(
        stdout,
        1,
        2,
        prev_width,
        box_height,
        Some("Previous command"),
    )?;
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

    // Next command box (newer command)
    draw_box(
        stdout,
        prev_width + cmd_width + 1,
        2,
        next_width,
        box_height,
        Some("Next command"),
    )?;
    write_in_box(stdout, prev_width + cmd_width + 1, 3, next_cmd, 1)?;

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

    // Let's include some mock stats data typical of shell command executions
    let stats = [
        ("Host", "laptop"),
        ("User", "uzair"),
        ("Time", &format_timestamp(entry.timestamp)),
        ("Duration", "36s"),
        ("Avg duration", "36s"),
        ("Exit", "0"),
        ("Directory", "/Users/uzair/01-dev/cli-tracker-rs"),
        ("Session", "019622b68ffe7452975ef38a2cbd7953"),
        ("Total runs", "5"),
    ];

    for (i, (key, value)) in stats.iter().enumerate() {
        let line = box_height + 4 + i as u16;
        execute!(stdout, cursor::MoveTo(3, line))?;
        write!(stdout, "{:<14} {}", key.with(Color::DarkGrey), value)?;
    }

    // Exit code distribution box - right top
    draw_box(
        stdout,
        stats_width + 1,
        box_height + 2,
        term_width - stats_width - 1,
        5,
        Some("Exit code distribution"),
    )?;
    write_in_box(stdout, stats_width + 1, box_height + 3, "███", 1)?;
    write_in_box(stdout, stats_width + 1, box_height + 4, "█0█ ▄130▄", 1)?;

    // Runs per day box - right middle
    draw_box(
        stdout,
        stats_width + 1,
        box_height + 7,
        term_width - stats_width - 1,
        4,
        Some("Runs per day"),
    )?;
    write_in_box(stdout, stats_width + 1, box_height + 8, "█5█", 1)?;
    write_in_box(stdout, stats_width + 1, box_height + 9, "Fri", 1)?;

    // Duration over time box - right bottom
    draw_box(
        stdout,
        stats_width + 1,
        box_height + 11,
        term_width - stats_width - 1,
        5,
        Some("Duration over time"),
    )?;
    write_in_box(stdout, stats_width + 1, box_height + 12, "█████", 1)?;
    write_in_box(stdout, stats_width + 1, box_height + 13, "█36s█", 1)?;
    write_in_box(stdout, stats_width + 1, box_height + 14, "04/25", 1)?;

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
    let mut view_mode: Option<usize> = None; // None for list view, Some(index) for detail view

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
            if let Event::Key(KeyEvent { code, .. }) = event::read()? {
                match code {
                    KeyCode::Char('q') | KeyCode::Esc => break, // Exit the loop
                    KeyCode::Up | KeyCode::Char('k') => {
                        current_index = current_index.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        current_index = (current_index + 1).min(entries.len().saturating_sub(1));
                    }
                    KeyCode::Enter => {
                        view_mode = Some(current_index); // Switch to detail view
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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::History => {
            let entries = get_history_entries()?;
            run_interactive_viewer(entries)?;
        }
    }

    Ok(())
}
