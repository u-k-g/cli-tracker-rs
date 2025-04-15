use anyhow::Result;
use chrono::{Datelike, Local, TimeZone, Timelike};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    style::{Color, Stylize},
    terminal::{self, ClearType},
};
use std::io::{self, Write};
use unicode_width::UnicodeWidthStr;

use crate::history::{format_timestamp, HistoryEntry};
use crate::ui_utils::draw_box;

pub fn display_stats(entries: &[HistoryEntry]) -> Result<()> {
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
                        if modifiers.contains(KeyModifiers::CONTROL) {
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
                if modifiers.contains(KeyModifiers::CONTROL) {
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
