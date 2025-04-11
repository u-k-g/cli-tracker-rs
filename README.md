# CLI Tracker & Analyzer

A Rust-based CLI tool designed to silently track your terminal command usage and provide insightful statistics and analysis, including a fun "Spotify Wrapped" style summary.

## Overview

This tool runs discreetly in the background, capturing every command you execute in your terminal. It logs detailed information about each command, such as the command itself, timestamp, execution duration, working directory, and exit status. The tracking process is designed to be completely invisible during normal terminal usage.

When you want to explore your command habits, you can use the CLI interface to view various statistics, analyze trends, and generate periodic summaries of your activity.

## Planned Features

*   **Silent Background Tracking:** Monitors shell activity and logs commands without interrupting your workflow or altering terminal appearance.
*   **Comprehensive Data Logging:** Records command text, timestamp, duration, working directory, exit status, and potentially shell type.
*   **Secure Local Storage:** Saves tracked data locally using an efficient storage mechanism (e.g., SQLite).
*   **Statistical Analysis:**
    *   Most frequently used commands.
    *   Command usage patterns over time (hourly, daily, weekly).
    *   Analysis by directory.
    *   Command success/error rates.
    *   Average command length and duration.
*   **"Terminal Wrapped" Summary:** A fun, periodic (e.g., yearly) recap of your command usage, highlighting top commands, busiest periods, unique stats, and interesting trends, similar to Spotify Wrapped.
*   **CLI Interface:**
    *   Commands to view various statistics and reports.
    *   Interactive history viewer (currently partially implemented).
    *   Command to generate the "Terminal Wrapped" summary.
    *   Commands to manage the tracking service (start/stop/status).
*   **Cross-Shell Compatibility:** Aiming for support across popular shells like Zsh, Bash, and Fish (initially focusing on Zsh).
*   **Performance Focused:** Built with Rust and `tokio` for efficient, low-overhead background operation.

## Planned Usage

```bash
# View command history interactively
cli-tracker history

# View command frequency statistics
cli-tracker stats frequency

# View stats for a specific time period
cli-tracker stats --period last-month

# Generate the "Terminal Wrapped" summary for the year
cli-tracker wrapped --year 2024

# Check the status of the background tracker
cli-tracker status

# Start the background tracker (if not running)
cli-tracker start-tracker

# Stop the background tracker
cli-tracker stop-tracker
```
*(Note: These commands represent the intended final functionality and may change during development.)*

## Installation

Once released, installation will likely be via Cargo:

```bash
cargo install cli-tracker
```

Or by building from source:

```bash
git clone <repository-url>
cd cli-tracker
cargo build --release
# Find the binary in ./target/release/cli-tracker
```

## Technology Stack

*   **Language:** Rust
*   **Async Runtime:** Tokio
*   **CLI Parsing:** Clap
*   **Database:** SQLx (with SQLite)
*   **Terminal UI:** Crossterm / Tui-rs (potentially)
*   **Date/Time:** Chrono

---

*This project is currently under development.*
