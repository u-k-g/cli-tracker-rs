use anyhow::Result;
use crossterm::{cursor, execute, style::Stylize};
use std::io::{self, Write};
use unicode_width::UnicodeWidthStr;

// Define box drawing characters
pub const TOP_LEFT: &str = "┌";
pub const TOP_RIGHT: &str = "┐";
pub const BOTTOM_LEFT: &str = "└";
pub const BOTTOM_RIGHT: &str = "┘";
pub const HORIZONTAL: &str = "─";
pub const VERTICAL: &str = "│";

// Helper function to draw a box
pub fn draw_box(
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
pub fn write_in_box(
    stdout: &mut io::Stdout,
    x: u16,
    y: u16,
    text: &str,
    x_offset: u16,
) -> Result<()> {
    execute!(stdout, cursor::MoveTo(x + 1 + x_offset, y))?;
    write!(stdout, "{}", text)?;
    Ok(())
}
