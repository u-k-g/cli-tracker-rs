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

fn display_detail_view(stdout: &mut io::Stdout, entry: &HistoryEntry) -> Result<()> {
    // Clear screen
    execute!(stdout, terminal::Clear(ClearType::All))?;

    // Each line explicitly positioned at column 0
    execute!(stdout, cursor::MoveTo(0, 0))?;
    write!(stdout, "{}", "Command Details".cyan().bold())?;

    execute!(stdout, cursor::MoveTo(0, 2))?;
    write!(
        stdout,
        "  Timestamp: {}",
        format_timestamp(entry.timestamp).green()
    )?;

    execute!(stdout, cursor::MoveTo(0, 4))?;
    write!(stdout, "  Command:")?;

    execute!(stdout, cursor::MoveTo(0, 5))?;
    write!(stdout, "    {}", entry.command.as_str().yellow())?;

    execute!(stdout, cursor::MoveTo(0, 8))?;
    write!(stdout, "{}", "(Press Esc or q to go back)".dark_grey())?;

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
            if let Some(entry) = entries.get(detail_index) {
                display_detail_view(&mut stdout, entry)?;
            } else {
                // Handle case where index is out of bounds (shouldn't happen ideally)
                view_mode = None;
                continue;
            }

            // Input handling for Detail View
            if let Event::Key(KeyEvent { code, .. }) = event::read()? {
                match code {
                    KeyCode::Char('q') | KeyCode::Esc => view_mode = None,
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
