/// Virtual display — a text-mode display buffer rendered via the host terminal.
///
/// The simulated display is a grid of character cells, each with a foreground
/// color, background color, and a Unicode character. MiteOS writes to this
/// buffer and the host terminal renders it.

use crossterm::{
    execute, queue,
    terminal::{self},
    cursor,
    style::{self, Color, SetForegroundColor, SetBackgroundColor, ResetColor, SetAttribute, Attribute},
    event::{self, Event, KeyEvent},
};
use std::io::{self, Write};

/// A single character cell on the display.
#[derive(Debug, Clone, Copy)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub dim: bool,
    pub underline: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: Color::Grey,
            bg: Color::Black,
            bold: false,
            dim: false,
            underline: false,
        }
    }
}

/// The virtual display.
pub struct Display {
    width: u16,
    height: u16,
    buffer: Vec<Vec<Cell>>,
    cursor_x: u16,
    cursor_y: u16,
    /// Whether the display content has changed since last flush.
    dirty: bool,
}

impl Display {
    /// Create a new display with the given dimensions.
    pub fn new(width: u16, height: u16) -> Self {
        let buffer = (0..height)
            .map(|_| (0..width).map(|_| Cell::default()).collect())
            .collect();
        Self {
            width,
            height,
            buffer,
            cursor_x: 0,
            cursor_y: 0,
            dirty: true,
        }
    }

    /// Create a display sized to the current terminal.
    pub fn from_terminal() -> io::Result<Self> {
        let (w, h) = terminal::size()?;
        Ok(Self::new(w, h))
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// Check if the display is dirty (needs re-rendering).
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Clear the entire display.
    pub fn clear(&mut self) {
        for row in &mut self.buffer {
            for cell in row {
                *cell = Cell::default();
            }
        }
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.dirty = true;
    }

    /// Write a character at the cursor position and advance the cursor.
    pub fn put_char(&mut self, ch: char) {
        if self.cursor_x >= self.width {
            self.cursor_x = 0;
            self.cursor_y = self.cursor_y.saturating_add(1);
        }
        if self.cursor_y >= self.height {
            // Scroll up
            self.buffer.remove(0);
            self.buffer.push((0..self.width).map(|_| Cell::default()).collect());
            self.cursor_y = self.height - 1;
        }
        self.set_cell(self.cursor_x, self.cursor_y, |c| c.ch = ch);
        self.cursor_x = self.cursor_x.saturating_add(1);
        self.dirty = true;
    }

    /// Write a string at the cursor position.
    pub fn put_str(&mut self, s: &str) {
        for ch in s.chars() {
            match ch {
                '\n' => {
                    self.cursor_x = 0;
                    self.cursor_y = self.cursor_y.saturating_add(1);
                    if self.cursor_y >= self.height {
                        self.scroll_up(1);
                        self.cursor_y = self.height - 1;
                    }
                }
                '\r' => {
                    self.cursor_x = 0;
                }
                _ => self.put_char(ch),
            }
        }
    }

    /// Write a string at a specific position without moving the cursor.
    pub fn put_str_at(&mut self, x: u16, y: u16, s: &str) {
        let mut cx = x;
        for ch in s.chars() {
            if cx >= self.width || y >= self.height {
                break;
            }
            self.set_cell(cx, y, |c| c.ch = ch);
            cx += 1;
        }
        self.dirty = true;
    }

    /// Set the cursor position.
    pub fn set_cursor(&mut self, x: u16, y: u16) {
        self.cursor_x = x.min(self.width - 1);
        self.cursor_y = y.min(self.height - 1);
    }

    /// Get cursor position.
    pub fn cursor_pos(&self) -> (u16, u16) {
        (self.cursor_x, self.cursor_y)
    }

    /// Set foreground color for subsequent writes (state tracking).
    pub fn set_fg(&mut self, _color: Color) {
        // Color state tracking - applied per-cell in full implementation
    }

    /// Set a cell's properties using a closure.
    pub fn set_cell<F>(&mut self, x: u16, y: u16, f: F)
    where
        F: FnOnce(&mut Cell),
    {
        if (x as usize) < self.width as usize && (y as usize) < self.height as usize {
            f(&mut self.buffer[y as usize][x as usize]);
            self.dirty = true;
        }
    }

    /// Get a cell's character at a position.
    pub fn get_char(&self, x: u16, y: u16) -> char {
        if (x as usize) < self.width as usize && (y as usize) < self.height as usize {
            self.buffer[y as usize][x as usize].ch
        } else {
            ' '
        }
    }

    /// Draw a horizontal line of characters.
    pub fn draw_hline(&mut self, x: u16, y: u16, len: u16, ch: char, fg: Color, bg: Color) {
        for i in 0..len {
            let cx = x + i;
            if cx >= self.width {
                break;
            }
            self.set_cell(cx, y, |c| {
                c.ch = ch;
                c.fg = fg;
                c.bg = bg;
            });
        }
        self.dirty = true;
    }

    /// Draw a vertical line of characters.
    pub fn draw_vline(&mut self, x: u16, y: u16, len: u16, ch: char, fg: Color, bg: Color) {
        for i in 0..len {
            let cy = y + i;
            if cy >= self.height {
                break;
            }
            self.set_cell(x, cy, |c| {
                c.ch = ch;
                c.fg = fg;
                c.bg = bg;
            });
        }
        self.dirty = true;
    }

    /// Draw a box (rectangle) with optional title.
    pub fn draw_box(
        &mut self,
        x: u16, y: u16, w: u16, h: u16,
        fg: Color, bg: Color,
        title: Option<&str>,
    ) {
        if w < 2 || h < 2 {
            return;
        }
        // Top-left corner
        if x < self.width && y < self.height {
            self.set_cell(x, y, |c| { c.ch = '┌'; c.fg = fg; c.bg = bg; });
        }
        // Top-right corner
        let rx = x + w - 1;
        if rx < self.width && y < self.height {
            self.set_cell(rx, y, |c| { c.ch = '┐'; c.fg = fg; c.bg = bg; });
        }
        // Bottom-left corner
        let by = y + h - 1;
        if x < self.width && by < self.height {
            self.set_cell(x, by, |c| { c.ch = '└'; c.fg = fg; c.bg = bg; });
        }
        // Bottom-right corner
        if rx < self.width && by < self.height {
            self.set_cell(rx, by, |c| { c.ch = '┘'; c.fg = fg; c.bg = bg; });
        }
        // Horizontal lines
        if w > 2 {
            self.draw_hline(x + 1, y, w - 2, '─', fg, bg);
            self.draw_hline(x + 1, by, w - 2, '─', fg, bg);
        }
        // Vertical lines
        if h > 2 {
            self.draw_vline(x, y + 1, h - 2, '│', fg, bg);
            self.draw_vline(rx, y + 1, h - 2, '│', fg, bg);
        }
        // Title
        if let Some(title) = title {
            let tx = x + 2;
            let title_str = format!(" {} ", title);
            for (i, ch) in title_str.chars().enumerate() {
                let cx = tx + i as u16;
                if cx < rx && y < self.height {
                    self.set_cell(cx, y, |c| { c.ch = ch; c.fg = fg; c.bg = bg; c.bold = true; });
                }
            }
        }
        self.dirty = true;
    }

    /// Fill a rectangular area with a character and colors.
    pub fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, ch: char, fg: Color, bg: Color) {
        for row in y..(y + h) {
            if row >= self.height {
                break;
            }
            for col in x..(x + w) {
                if col >= self.width {
                    break;
                }
                self.set_cell(col, row, |c| { c.ch = ch; c.fg = fg; c.bg = bg; });
            }
        }
        self.dirty = true;
    }

    /// Scroll the display up by n lines.
    pub fn scroll_up(&mut self, n: u16) {
        for _ in 0..n {
            if self.buffer.len() > 1 {
                self.buffer.remove(0);
                self.buffer.push((0..self.width).map(|_| Cell::default()).collect());
            }
        }
        self.dirty = true;
    }

    /// Render the entire display buffer to the terminal.
    /// This is the main rendering function that syncs the virtual
    /// display to the actual terminal output.
    pub fn flush(&mut self, stdout: &mut impl Write) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }

        queue!(stdout, cursor::Hide)?;

        for (y, row) in self.buffer.iter().enumerate() {
            queue!(stdout, cursor::MoveTo(0, y as u16))?;
            for cell in row.iter() {
                queue!(stdout, SetForegroundColor(cell.fg))?;
                queue!(stdout, SetBackgroundColor(cell.bg))?;
                if cell.bold {
                    queue!(stdout, SetAttribute(Attribute::Bold))?;
                }
                if cell.underline {
                    queue!(stdout, SetAttribute(Attribute::Underlined))?;
                }
                queue!(stdout, style::Print(cell.ch))?;
                if cell.bold {
                    queue!(stdout, SetAttribute(Attribute::Reset))?;
                }
                if cell.underline {
                    queue!(stdout, SetAttribute(Attribute::Reset))?;
                }
            }
        }

        queue!(stdout, ResetColor)?;
        queue!(stdout, cursor::MoveTo(self.cursor_x, self.cursor_y))?;
        queue!(stdout, cursor::Show)?;
        stdout.flush()?;
        self.dirty = false;
        Ok(())
    }

    /// Read a single key event from the terminal.
    pub fn read_key(timeout_ms: u64) -> io::Result<Option<KeyEvent>> {
        if event::poll(std::time::Duration::from_millis(timeout_ms))? {
            if let Event::Key(key) = event::read()? {
                return Ok(Some(key));
            }
        }
        Ok(None)
    }

    /// Enter raw/alternate screen mode.
    pub fn enter_raw_mode(stdout: &mut impl Write) -> io::Result<()> {
        execute!(stdout, terminal::EnterAlternateScreen)?;
        terminal::enable_raw_mode()?;
        Ok(())
    }

    /// Exit raw/alternate screen mode.
    pub fn exit_raw_mode(stdout: &mut impl Write) -> io::Result<()> {
        terminal::disable_raw_mode()?;
        execute!(stdout, terminal::LeaveAlternateScreen)?;
        Ok(())
    }
}
