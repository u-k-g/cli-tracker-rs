use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent},
    execute,
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

#[derive(Debug)]
struct HistoryEntry {
    command: String,
}

fn get_zsh_history_path() -> Result<PathBuf> {
    let home = home::home_dir().context("Could not find home directory")?;
    Ok(home.join(".zsh_history"))
}

fn parse_history_line(line: &str) -> Option<HistoryEntry> {
    // If line starts with ': ', it's an extended history format
    let command = if line.starts_with(": ") {
        // Skip timestamp and duration, get everything after the semicolon
        line.split(';').nth(1)?
    } else {
        line
    };

    // Clean up the command
    let clean_command = command.trim().to_string();

    if !clean_command.is_empty() {
        Some(HistoryEntry {
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

fn run_interactive_viewer(entries: Vec<HistoryEntry>) -> Result<()> {
    let mut stdout = io::stdout();

    // Enter alternate screen and hide cursor
    execute!(stdout, terminal::EnterAlternateScreen)?;
    terminal::enable_raw_mode()?;
    execute!(stdout, cursor::Hide)?;

    let mut current_index = entries.len().saturating_sub(1);
    let mut running = true;

    while running {
        // Clear screen and reset cursor position
        execute!(stdout, terminal::Clear(ClearType::All))?;
        execute!(stdout, cursor::MoveTo(0, 0))?;

        // Display header
        writeln!(stdout, "Command History (↑/k: up, ↓/j: down, q: quit)\n")?;

        // Calculate display window
        let window_size = 10;
        let start_idx = current_index.saturating_sub(window_size / 2);
        let end_idx = (start_idx + window_size).min(entries.len());

        // Display commands with proper alignment, each starting at column 0
        for (idx, entry) in entries[start_idx..end_idx].iter().enumerate() {
            let line_num = entries.len() - (start_idx + idx);
            let prefix = if idx + start_idx == current_index {
                ">"
            } else {
                " "
            };

            // Move cursor to start of line for each entry
            execute!(stdout, cursor::MoveTo(0, (idx + 3) as u16))?;

            // Write the entry with fixed formatting
            write!(stdout, "{} {:4} │ {}", prefix, line_num, entry.command)?;
        }

        stdout.flush()?;

        // Handle input
        if let Event::Key(KeyEvent { code, .. }) = event::read()? {
            match code {
                KeyCode::Char('q') | KeyCode::Esc => running = false,
                KeyCode::Up | KeyCode::Char('k') => {
                    current_index = current_index.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    current_index = (current_index + 1).min(entries.len().saturating_sub(1));
                }
                _ => {}
            }
        }
    }

    // Cleanup: show cursor and exit alternate screen
    execute!(stdout, cursor::Show)?;
    terminal::disable_raw_mode()?;
    execute!(stdout, terminal::LeaveAlternateScreen)?;

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
