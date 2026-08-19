/**
 * MiteOS — A minimal GUI-based operating system that runs inside the MitePC simulator.
 *
 * MiteOS is NOT based on Linux. It is a custom microkernel-style OS with:
 * - A simple windowing GUI system rendered to the virtual display
 * - Built-in applications (File Manager, System Info, Terminal, Settings, About)
 * - A desktop environment with icons, taskbar, and window management
 * - Direct access to the simulated hardware (CPU, RAM, Storage)
 *
 * Architecture:
 * - The MiteOS kernel initializes the hardware and starts the GUI
 * - The window manager handles overlapping windows with focus
 * - Each app is a struct implementing the App trait
 * - User input is routed through the window manager to the focused app
 */

use crate::display::Display;
use crate::mitefs::MiteFS;
use crate::ram::Ram;
use crate::cpu::Cpu;
use crossterm::style::Color;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use chrono::Local;

// ---- OS Context (read-only snapshot for app rendering) ----

/// Read-only snapshot of OS state passed to apps for rendering.
/// This avoids borrow conflicts when apps need OS data.
pub struct OsContext {
    pub ram_size: u64,
    pub ram_used_pages: usize,
    pub ram_total_pages: u64,
    pub ram_usage_percent: f64,
    pub cpu_mhz: u32,
    pub cpu_instructions: u64,
    pub cpu_cores: u32,
    pub uptime_secs: u64,
    pub fs_total_blocks: u64,
    pub fs_free_blocks: u64,
    pub fs_total_inodes: u64,
    pub fs_free_inodes: u64,
    pub fs_total_size: u64,
}

impl OsContext {
    fn from_os(os: &MiteOS) -> Self {
        let (tb, fb, ti, fi, _fdb, tsz) = os.fs.stats();
        Self {
            ram_size: os.ram.size(),
            ram_used_pages: os.ram.used_pages(),
            ram_total_pages: os.ram.total_pages(),
            ram_usage_percent: os.ram.usage_percent(),
            cpu_mhz: os.cpu.clock_mhz(),
            cpu_instructions: os.cpu.instruction_count(),
            cpu_cores: os.config_cpu_cores,
            uptime_secs: os.uptime_secs,
            fs_total_blocks: tb,
            fs_free_blocks: fb,
            fs_total_inodes: ti,
            fs_free_inodes: fi,
            fs_total_size: tsz,
        }
    }
}

// ---- Color Palette ----

pub const COLOR_DESKTOP_BG: Color = Color::Rgb { r: 30, g: 30, b: 60 };
pub const COLOR_TASKBAR_BG: Color = Color::Rgb { r: 20, g: 20, b: 40 };
pub const COLOR_TASKBAR_FG: Color = Color::Rgb { r: 180, g: 200, b: 255 };
pub const COLOR_WINDOW_BG: Color = Color::Rgb { r: 15, g: 15, b: 30 };
pub const COLOR_WINDOW_BORDER: Color = Color::Rgb { r: 80, g: 100, b: 180 };
pub const COLOR_WINDOW_TITLE: Color = Color::Rgb { r: 120, g: 150, b: 255 };
pub const COLOR_TEXT: Color = Color::Rgb { r: 200, g: 210, b: 230 };
pub const COLOR_TEXT_DIM: Color = Color::Rgb { r: 100, g: 110, b: 130 };
pub const COLOR_HIGHLIGHT: Color = Color::Rgb { r: 70, g: 130, b: 255 };
pub const COLOR_ACCENT: Color = Color::Rgb { r: 255, g: 180, b: 50 };
pub const COLOR_SUCCESS: Color = Color::Rgb { r: 50, g: 205, b: 100 };
pub const COLOR_ERROR: Color = Color::Rgb { r: 255, g: 80, b: 80 };
pub const COLOR_ICON_FG: Color = Color::Rgb { r: 140, g: 170, b: 255 };
pub const COLOR_ICON_BG: Color = Color::Rgb { r: 40, g: 40, b: 70 };
pub const COLOR_SELECTION: Color = Color::Rgb { r: 50, g: 80, b: 160 };
pub const COLOR_INPUT_BG: Color = Color::Rgb { r: 25, g: 25, b: 50 };

// ---- Window ----

/// A window in the MiteOS GUI.
#[derive(Debug, Clone)]
pub struct Window {
    pub id: usize,
    pub title: String,
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub focused: bool,
    pub visible: bool,
    pub app_type: AppType,
    /// Scroll offset for content.
    pub scroll_y: u16,
}

impl Window {
    pub fn new(id: usize, title: &str, x: u16, y: u16, w: u16, h: u16, app_type: AppType) -> Self {
        Self {
            id, title: title.to_string(), x, y, w, h,
            focused: false, visible: true, app_type, scroll_y: 0,
        }
    }

    /// Content area (inside the border and title bar).
    pub fn content_rect(&self) -> (u16, u16, u16, u16) {
        (self.x + 1, self.y + 2, self.w - 2, self.h - 3)
    }
}

// ---- App Type ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppType {
    Desktop,
    FileManager,
    SystemInfo,
    Terminal,
    Settings,
    About,
    TextViewer,
}

impl AppType {
    pub fn icon_char(&self) -> char {
        match self {
            AppType::FileManager => '\u{1F4C1}', // folder
            AppType::SystemInfo => '\u{1F4BB}', // computer
            AppType::Terminal => '\u{1F4A3}',     // terminal
            AppType::Settings => '\u{2699}',     // gear
            AppType::About => '\u{2139}',        // info
            AppType::Desktop => ' ',
            AppType::TextViewer => '\u{1F4C4}',  // page
        }
    }

    pub fn label(&self) -> &str {
        match self {
            AppType::Desktop => "Desktop",
            AppType::FileManager => "Files",
            AppType::SystemInfo => "System",
            AppType::Terminal => "Terminal",
            AppType::Settings => "Settings",
            AppType::About => "About",
            AppType::TextViewer => "Viewer",
        }
    }
}

// ---- App Trait ----

pub trait App {
    /// Called when the app is opened/created.
    fn on_open(&mut self, _window: &mut Window, _fs: &mut MiteFS) {}
    /// Handle a key event. Return true if the event was consumed.
    fn on_key(&mut self, key: KeyEvent, window: &mut Window, fs: &mut MiteFS, _ctx: &OsContext) -> bool;
    /// Render the app content into the display (crossterm backend).
    fn on_draw(&mut self, display: &mut Display, window: &Window, ctx: &OsContext);
    /// Get the app type.
    fn app_type(&self) -> AppType;
    /// Serialize app content as JSON for the WebView backend.
    /// Returns a JSON string representing the app's visual state.
    /// Each app should override this to provide its content for the HTML frontend.
    fn webview_content_json(&self, _window: &Window, _ctx: &OsContext) -> String {
        "{}".to_string()
    }
}

// ---- Built-in Applications ----

// -- File Manager --

pub struct FileManagerApp {
    current_path: String,
    entries: Vec<(u64, String, u8)>,
    selected: usize,
    scroll: u16,
}

impl FileManagerApp {
    pub fn new() -> Self {
        Self {
            current_path: "/".to_string(),
            entries: Vec::new(),
            selected: 0,
            scroll: 0,
        }
    }

    fn refresh(&mut self, fs: &mut MiteFS) {
        self.entries = fs.list_dir(&self.current_path).unwrap_or_default();
        // Add ".." for parent navigation
        if self.current_path != "/" {
            self.entries.insert(0, (0, "..".to_string(), 1));
        }
        if self.selected >= self.entries.len() {
            self.selected = 0;
        }
    }
}

impl App for FileManagerApp {
    fn on_open(&mut self, _window: &mut Window, fs: &mut MiteFS) {
        self.refresh(fs);
    }

    fn on_key(&mut self, key: KeyEvent, window: &mut Window, fs: &mut MiteFS, _ctx: &OsContext) -> bool {
        match key.code {
            KeyCode::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.selected < self.entries.len().saturating_sub(1) {
                    self.selected += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(entry) = self.entries.get(self.selected) {
                    if entry.1 == ".." {
                        // Go to parent
                        if let Some(pos) = self.current_path.rfind('/') {
                            self.current_path = if pos == 0 { "/".to_string() } else { self.current_path[..pos].to_string() };
                        }
                        self.refresh(fs);
                    } else if entry.2 == 1 {
                        // Directory - navigate into it
                        if self.current_path == "/" {
                            self.current_path = format!("/{}", entry.1);
                        } else {
                            self.current_path = format!("{}/{}", self.current_path, entry.1);
                        }
                        self.selected = 0;
                        self.scroll = 0;
                        self.refresh(fs);
                        window.title = format!("Files: {}", self.current_path);
                    }
                }
            }
            KeyCode::Backspace => {
                // Go up
                if self.current_path != "/" {
                    if let Some(pos) = self.current_path.rfind('/') {
                        self.current_path = if pos == 0 { "/".to_string() } else { self.current_path[..pos].to_string() };
                    }
                    self.selected = 0;
                    self.scroll = 0;
                    self.refresh(fs);
                    window.title = format!("Files: {}", self.current_path);
                }
            }
            _ => return false,
        }
        true
    }

    fn on_draw(&mut self, display: &mut Display, window: &Window, _ctx: &OsContext) {
        let (cx, cy, cw, ch) = window.content_rect();
        if cw == 0 || ch == 0 { return; }

        // Header
        let header = format!(" {} ", self.current_path);
        display.put_str_at(cx + 1, cy, &header);
        display.set_cell(cx + 1, cy, |c| { c.fg = COLOR_ACCENT; c.bold = true; });

        // Separator
        display.draw_hline(cx, cy + 1, cw, '─', COLOR_TEXT_DIM, COLOR_WINDOW_BG);

        // File list
        let visible = ch.saturating_sub(2) as usize;
        let start = self.scroll as usize;
        for i in 0..visible {
            let idx = start + i;
            if idx >= self.entries.len() { break; }
            let (_ino, name, itype) = &self.entries[idx];
            let row_y = cy + 2 + i as u16;

            let is_selected = idx == self.selected;
            let bg = if is_selected { COLOR_SELECTION } else { COLOR_WINDOW_BG };
            let fg = if is_selected { Color::White } else { COLOR_TEXT };

            // Fill row background
            display.draw_hline(cx, row_y, cw, ' ', fg, bg);

            // Icon
            let icon = if *itype == 1 { "[DIR] " } else { "[FILE]" };
            let icon_color = if *itype == 1 { COLOR_ACCENT } else { COLOR_HIGHLIGHT };
            display.put_str_at(cx + 1, row_y, icon);
            // Color the icon
            for j in 0..5 {
                display.set_cell(cx + 1 + j, row_y, |c| { c.fg = icon_color; });
            }

            // Name
            display.put_str_at(cx + 7, row_y, name);
        }

        // Status bar
        if ch > 2 {
            let status_y = cy + ch - 1;
            display.draw_hline(cx, status_y, cw, '─', COLOR_TEXT_DIM, COLOR_WINDOW_BG);
            let status = format!(" {} items | Sel: {}/{} ",
                self.entries.len(), self.selected + 1, self.entries.len());
            display.put_str_at(cx + 1, status_y, &status);
            for j in 0..status.len() {
                display.set_cell(cx + 1 + j as u16, status_y, |c| { c.fg = COLOR_TEXT_DIM; });
            }
        }
    }

    fn app_type(&self) -> AppType { AppType::FileManager }

    fn webview_content_json(&self, _window: &Window, _ctx: &OsContext) -> String {
        let entries_json: Vec<String> = self.entries.iter().map(|(_, name, itype)| {
            format!("{{\"name\":\"{}\",\"type\":{}}}", name.replace('"', "\\\""), itype)
        }).collect();
        format!(r#"{{"currentPath":"{}","selected":{},"entries":[{}],"status":" {} items | Sel: {}/{} "}}"#,
            self.current_path,
            self.selected,
            entries_json.join(","),
            self.entries.len(),
            self.selected + 1,
            self.entries.len()
        )
    }
}

// -- System Info --

pub struct SystemInfoApp {
    scroll: u16,
}

impl SystemInfoApp {
    pub fn new() -> Self { Self { scroll: 0 } }
}

impl App for SystemInfoApp {
    fn on_key(&mut self, key: KeyEvent, _window: &mut Window, _fs: &mut MiteFS, _ctx: &OsContext) -> bool {
        if key.code == KeyCode::Down { self.scroll = self.scroll.saturating_add(1); return true; }
        if key.code == KeyCode::Up { self.scroll = self.scroll.saturating_sub(1); return true; }
        false
    }

    fn on_draw(&mut self, display: &mut Display, window: &Window, ctx: &OsContext) {
        let (cx, cy, cw, _ch) = window.content_rect();
        if cw == 0 { return; }

        let mut y = cy;
        let draw_line = |display: &mut Display, y: &mut u16, cx: u16, cw: u16, text: &str, fg: Color, bg: Color| {
            if *y >= display.height() { return; }
            display.draw_hline(cx, *y, cw, ' ', fg, bg);
            display.put_str_at(cx + 1, *y, text);
            for j in 0..text.len().min(cw as usize) {
                display.set_cell(cx + 1 + j as u16, *y, |c| { c.fg = fg; });
            }
            *y += 1;
        };

        // Hardware section
        draw_line(display, &mut y, cx, cw, "HARDWARE", COLOR_ACCENT, COLOR_WINDOW_BG);
        draw_line(display, &mut y, cx, cw, &format!("  CPU: Mite-16 @ {} MHz", ctx.cpu_mhz), COLOR_TEXT, COLOR_WINDOW_BG);
        draw_line(display, &mut y, cx, cw, &format!("  CPU Cores: {}", ctx.cpu_cores), COLOR_TEXT, COLOR_WINDOW_BG);
        draw_line(display, &mut y, cx, cw, &format!("  Instructions: {}", ctx.cpu_instructions), COLOR_TEXT, COLOR_WINDOW_BG);
        draw_line(display, &mut y, cx, cw, "", COLOR_TEXT, COLOR_WINDOW_BG);

        let ram_mb = ctx.ram_size / (1024 * 1024);
        let used_pages = ctx.ram_used_pages;
        let total_pages = ctx.ram_total_pages;
        let usage = ctx.ram_usage_percent;
        draw_line(display, &mut y, cx, cw, &format!("  RAM: {} MB", ram_mb), COLOR_TEXT, COLOR_WINDOW_BG);
        draw_line(display, &mut y, cx, cw, &format!("  Pages: {}/{} used ({:.1}%)", used_pages, total_pages, usage), COLOR_TEXT, COLOR_WINDOW_BG);
        draw_line(display, &mut y, cx, cw, &format!("  Usage bar: {}{}",
            "#".repeat((usage / 5.0) as usize),
            "-".repeat(20usize.saturating_sub((usage / 5.0) as usize))),
            COLOR_SUCCESS, COLOR_WINDOW_BG);
        draw_line(display, &mut y, cx, cw, "", COLOR_TEXT, COLOR_WINDOW_BG);

        // Storage section
        draw_line(display, &mut y, cx, cw, "STORAGE", COLOR_ACCENT, COLOR_WINDOW_BG);
        let total_blks = ctx.fs_total_blocks;
        let free_blks = ctx.fs_free_blocks;
        let total_inos = ctx.fs_total_inodes;
        let free_inos = ctx.fs_free_inodes;
        let total_sz = ctx.fs_total_size;
        let storage_mb = total_sz / (1024 * 1024);
        let used_mb = (total_blks - free_blks) * 4096 / (1024 * 1024);
        draw_line(display, &mut y, cx, cw, &format!("  Image: {} MB", storage_mb), COLOR_TEXT, COLOR_WINDOW_BG);
        draw_line(display, &mut y, cx, cw, &format!("  Used: {}/{} MB", used_mb, storage_mb), COLOR_TEXT, COLOR_WINDOW_BG);
        draw_line(display, &mut y, cx, cw, &format!("  Inodes: {}/{} free", free_inos, total_inos), COLOR_TEXT, COLOR_WINDOW_BG);
        draw_line(display, &mut y, cx, cw, &format!("  Blocks: {}/{} free", free_blks, total_blks), COLOR_TEXT, COLOR_WINDOW_BG);
        draw_line(display, &mut y, cx, cw, "", COLOR_TEXT, COLOR_WINDOW_BG);

        // OS section
        draw_line(display, &mut y, cx, cw, "MITEOS", COLOR_ACCENT, COLOR_WINDOW_BG);
        draw_line(display, &mut y, cx, cw, "  Version: 1.0.0", COLOR_TEXT, COLOR_WINDOW_BG);
        draw_line(display, &mut y, cx, cw, "  Arch: mite-16 (custom ISA)", COLOR_TEXT, COLOR_WINDOW_BG);
        draw_line(display, &mut y, cx, cw, "  Kernel: MiteMicro", COLOR_TEXT, COLOR_WINDOW_BG);
        draw_line(display, &mut y, cx, cw, "  FS: MiteFS (.mite format)", COLOR_TEXT, COLOR_WINDOW_BG);
        draw_line(display, &mut y, cx, cw, &format!("  Uptime: {}s", ctx.uptime_secs), COLOR_TEXT, COLOR_WINDOW_BG);

        let datetime = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        draw_line(display, &mut y, cx, cw, &format!("  Time: {}", datetime), COLOR_TEXT, COLOR_WINDOW_BG);
    }

    fn app_type(&self) -> AppType { AppType::SystemInfo }

    fn webview_content_json(&self, _window: &Window, _ctx: &OsContext) -> String {
        // SystemInfo doesn't need app-specific content; the JS reads from state.systemInfo
        "{}".to_string()
    }
}

// -- Terminal --

pub struct TerminalApp {
    lines: Vec<String>,
    input: String,
    history: Vec<String>,
    history_idx: usize,
    cwd: String,
}

impl TerminalApp {
    pub fn new() -> Self {
        Self {
            lines: vec!["MiteOS Terminal v1.0".to_string(), "Type 'help' for commands.".to_string(), String::new()],
            input: String::new(),
            history: Vec::new(),
            history_idx: 0,
            cwd: "/".to_string(),
        }
    }

    fn execute(&mut self, fs: &mut MiteFS, ctx: &OsContext) {
        let cmd = self.input.trim().to_string();
        if cmd.is_empty() {
            self.lines.push(format!("{}> ", self.cwd));
            return;
        }

        self.history.push(cmd.clone());
        self.history_idx = self.history.len();

        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let output = match parts.get(0).map(|s| *s) {
            Some("help") => {
                "Commands: help, ls, cd, cat, stat, mkdir, touch, rm, echo, clear, sysinfo, exit".to_string()
            }
            Some("ls") => {
                let path = parts.get(1).map(|s| *s).unwrap_or(&self.cwd);
                let full = if path.starts_with('/') { path.to_string() } else { format!("{}/{}", self.cwd, path) };
                match fs.list_dir(&full) {
                    Ok(entries) => {
                        let mut out = String::new();
                        for (_ino, name, itype) in &entries {
                            let kind = if *itype == 1 { "DIR " } else { "FILE" };
                            out.push_str(&format!("  {} {}\n", kind, name));
                        }
                        if out.is_empty() { "  (empty directory)".to_string() } else { out }
                    }
                    Err(e) => format!("ls: {}", e)
                }
            }
            Some("cd") => {
                if let Some(path) = parts.get(1) {
                    let full = if path.starts_with('/') { path.to_string() } else { format!("{}/{}", self.cwd, path) };
                    match fs.stat(&full) {
                        Ok(ino) if ino.is_dir() => {
                            self.cwd = full;
                            String::new()
                        }
                        Ok(_) => "cd: not a directory".to_string(),
                        Err(e) => format!("cd: {}", e)
                    }
                } else {
                    self.cwd.clone()
                }
            }
            Some("cat") => {
                if let Some(path) = parts.get(1) {
                    let full = if path.starts_with('/') { path.to_string() } else { format!("{}/{}", self.cwd, path) };
                    match fs.read_file_string(&full) {
                        Ok(content) => content,
                        Err(e) => format!("cat: {}", e)
                    }
                } else {
                    "cat: missing file".to_string()
                }
            }
            Some("stat") => {
                if let Some(path) = parts.get(1) {
                    let full = if path.starts_with('/') { path.to_string() } else { format!("{}/{}", self.cwd, path) };
                    match fs.stat(&full) {
                        Ok(ino) => format!(
                            "  Name: {}\n  Type: {}\n  Size: {} bytes\n  Inode: {}",
                            ino.name,
                            if ino.is_dir() { "directory" } else { "file" },
                            ino.size, ino.inode_num
                        ),
                        Err(e) => format!("stat: {}", e)
                    }
                } else {
                    "stat: missing path".to_string()
                }
            }
            Some("mkdir") => {
                if let Some(name) = parts.get(1) {
                    let full = if name.starts_with('/') { name.to_string() } else { format!("{}/{}", self.cwd, name) };
                    match fs.create_dir(&full) {
                        Ok(_) => String::new(),
                        Err(e) => format!("mkdir: {}", e)
                    }
                } else {
                    "mkdir: missing name".to_string()
                }
            }
            Some("touch") => {
                if let Some(name) = parts.get(1) {
                    let full = if name.starts_with('/') { name.to_string() } else { format!("{}/{}", self.cwd, name) };
                    match fs.create_file(&full, b"") {
                        Ok(_) => String::new(),
                        Err(e) => format!("touch: {}", e)
                    }
                } else {
                    "touch: missing name".to_string()
                }
            }
            Some("rm") => {
                if let Some(path) = parts.get(1) {
                    let full = if path.starts_with('/') { path.to_string() } else { format!("{}/{}", self.cwd, path) };
                    match fs.delete(&full) {
                        Ok(_) => String::new(),
                        Err(e) => format!("rm: {}", e)
                    }
                } else {
                    "rm: missing path".to_string()
                }
            }
            Some("echo") => {
                let rest = parts[1..].join(" ");
                rest
            }
            Some("clear") => {
                self.lines.clear();
                String::new()
            }
            Some("sysinfo") => {
                let ram_mb = ctx.ram_size / (1024 * 1024);
                format!(
                    "CPU: Mite-16 @ {} MHz | RAM: {} MB | Cores: {}\nFS: MiteFS | OS: MiteOS 1.0.0\nUptime: {}s",
                    ctx.cpu_mhz, ram_mb, ctx.cpu_cores, ctx.uptime_secs
                )
            }
            Some("exit") => {
                "Use Esc or Alt+F4 to close window.".to_string()
            }
            Some(cmd) => format!("{}: command not found. Type 'help'.", cmd),
            None => String::new(),
        };

        self.lines.push(format!("{}> {}", self.cwd, cmd));
        if !output.is_empty() {
            for line in output.lines() {
                self.lines.push(line.to_string());
            }
        }
        self.input.clear();
    }
}

impl App for TerminalApp {
    fn on_key(&mut self, key: KeyEvent, _window: &mut Window, fs: &mut MiteFS, ctx: &OsContext) -> bool {
        match key.code {
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    if c == 'c' || c == 'd' {
                        return false; // let window close
                    }
                }
                self.input.push(c);
            }
            KeyCode::Backspace => { self.input.pop(); }
            KeyCode::Enter => { self.execute(fs, ctx); }
            KeyCode::Up => {
                if self.history_idx > 0 {
                    self.history_idx -= 1;
                    self.input = self.history.get(self.history_idx).cloned().unwrap_or_default();
                }
            }
            KeyCode::Down => {
                if self.history_idx < self.history.len() {
                    self.history_idx += 1;
                    self.input = self.history.get(self.history_idx).cloned().unwrap_or_default();
                }
            }
            _ => return false,
        }
        true
    }

    fn on_draw(&mut self, display: &mut Display, window: &Window, _ctx: &OsContext) {
        let (cx, cy, cw, ch) = window.content_rect();
        if cw == 0 || ch == 0 { return; }

        let visible_lines = ch.saturating_sub(1) as usize;
        let total_lines = self.lines.len();
        let scroll_offset = total_lines.saturating_sub(visible_lines);

        for i in 0..visible_lines {
            let line_idx = scroll_offset + i;
            let row_y = cy + i as u16;
            display.draw_hline(cx, row_y, cw, ' ', COLOR_TEXT, COLOR_WINDOW_BG);

            if line_idx < total_lines {
                let line = &self.lines[line_idx];
                let display_line = if line.len() > cw as usize - 2 {
                    &line[line.len() - cw as usize + 2..]
                } else {
                    line
                };
                display.put_str_at(cx + 1, row_y, display_line);
                for j in 0..display_line.len().min(cw as usize) {
                    display.set_cell(cx + 1 + j as u16, row_y, |c| { c.fg = COLOR_TEXT; });
                }
            }
        }

        // Input line
        let input_y = cy + ch - 1;
        display.draw_hline(cx, input_y, cw, ' ', COLOR_TEXT, COLOR_INPUT_BG);
        let prompt = format!("> {}", self.input);
        display.put_str_at(cx + 1, input_y, &prompt);
        for j in 0..prompt.len().min(cw as usize) {
            display.set_cell(cx + 1 + j as u16, input_y, |c| { c.fg = Color::Green; });
        }
        // Cursor blink
        let cursor_x = cx + 1 + prompt.len() as u16;
        if cursor_x < cx + cw {
            display.set_cell(cursor_x, input_y, |c| { c.ch = ' '; c.bg = COLOR_HIGHLIGHT; });
        }
    }

    fn app_type(&self) -> AppType { AppType::Terminal }

    fn webview_content_json(&self, _window: &Window, _ctx: &OsContext) -> String {
        let lines_json: Vec<String> = self.lines.iter().map(|l| l.replace('\\', "\\\\").replace('"', "\\\"")).collect();
        let input_escaped = self.input.replace('\\', "\\\\").replace('"', "\\\"");
        let lines_arr = lines_json.iter().map(|l| format!("\"{}\"", l)).collect::<Vec<_>>().join(",");
        format!("{{\"lines\":[{}],\"input\":\"{}\",\"prompt\":\">\",\"windowId\":{}}}",
            lines_arr, input_escaped, _window.id)
    }
}

// -- Settings --

pub struct SettingsApp {
    selected: usize,
    options: Vec<(String, String)>,
}

impl SettingsApp {
    pub fn new() -> Self {
        Self {
            selected: 0,
            options: vec![
                ("Theme".into(), "Dark".into()),
                ("Font Size".into(), "Normal".into()),
                ("Resolution".into(), "Auto".into()),
                ("Taskbar".into(), "Bottom".into()),
                ("Animations".into(), "On".into()),
            ],
        }
    }
}

impl App for SettingsApp {
    fn on_key(&mut self, key: KeyEvent, _window: &mut Window, _fs: &mut MiteFS, _ctx: &OsContext) -> bool {
        match key.code {
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => self.selected = (self.selected + 1).min(self.options.len() - 1),
            _ => return false,
        }
        true
    }

    fn on_draw(&mut self, display: &mut Display, window: &Window, _ctx: &OsContext) {
        let (cx, cy, cw, ch) = window.content_rect();
        if cw == 0 || ch == 0 { return; }

        display.put_str_at(cx + 1, cy, "Settings");
        for j in 0..8 { display.set_cell(cx + 1 + j, cy, |c| { c.fg = COLOR_ACCENT; c.bold = true; }); }
        display.draw_hline(cx, cy + 1, cw, '─', COLOR_TEXT_DIM, COLOR_WINDOW_BG);

        for (i, (key, val)) in self.options.iter().enumerate() {
            let row_y = cy + 2 + i as u16;
            if row_y >= cy + ch { break; }
            let is_sel = i == self.selected;
            let bg = if is_sel { COLOR_SELECTION } else { COLOR_WINDOW_BG };
            display.draw_hline(cx, row_y, cw, ' ', COLOR_TEXT, bg);

            let line = format!("  {:<15} : {}", key, val);
            display.put_str_at(cx + 1, row_y, &line);
            let fg = if is_sel { Color::White } else { COLOR_TEXT };
            for j in 0..line.len().min(cw as usize) {
                display.set_cell(cx + 1 + j as u16, row_y, |c| { c.fg = fg; });
            }
        }

        // Info at bottom
        let info_y = cy + ch - 1;
        display.draw_hline(cx, info_y, cw, '─', COLOR_TEXT_DIM, COLOR_WINDOW_BG);
        display.put_str_at(cx + 1, info_y, " Navigate: Up/Down | Values: read-only display");
        for j in 0..48 { display.set_cell(cx + 1 + j, info_y, |c| { c.fg = COLOR_TEXT_DIM; }); }
    }

    fn app_type(&self) -> AppType { AppType::Settings }

    fn webview_content_json(&self, _window: &Window, _ctx: &OsContext) -> String {
        let options_json: Vec<String> = self.options.iter().map(|(k, v)| {
            format!("{{\"key\":\"{}\",\"value\":\"{}\"}}", k.replace('"', "\\\""), v.replace('"', "\\\""))
        }).collect();
        format!("{{\"selected\":{},\"options\":[{}]}}", self.selected, options_json.join(","))
    }
}

// -- About --

pub struct AboutApp;

impl AboutApp {
    pub fn new() -> Self { Self }
}

impl App for AboutApp {
    fn on_key(&mut self, _key: KeyEvent, _window: &mut Window, _fs: &mut MiteFS, _ctx: &OsContext) -> bool { false }

    fn on_draw(&mut self, display: &mut Display, window: &Window, _ctx: &OsContext) {
        let (cx, cy, cw, ch) = window.content_rect();
        if cw == 0 || ch == 0 { return; }

        let mut y = cy;
        let lines = [
            ("  __  __ _       _ _   _ _   _ ", COLOR_HIGHLIGHT),
            (" |  \\/  (_)_ __ (_) | | | \\ | |", COLOR_HIGHLIGHT),
            (" | |/| | | '_ \\| | | | |  \\| |", COLOR_HIGHLIGHT),
            (" | |  | | | | | | | |_| | |\\  |", COLOR_HIGHLIGHT),
            (" |_|  |_|_|_| |_|_|\\___/|_| \\_|", COLOR_HIGHLIGHT),
            ("", COLOR_TEXT),
            ("     MiteOS v1.0.0", COLOR_ACCENT),
            ("", COLOR_TEXT),
            ("  A minimal GUI-based operating system", COLOR_TEXT),
            ("  built for the MitePC simulator.", COLOR_TEXT),
            ("", COLOR_TEXT),
            ("  Architecture:  Mite-16 (custom ISA)", COLOR_TEXT),
            ("  Kernel:        MiteMicro", COLOR_TEXT),
            ("  Filesystem:    MiteFS (.mite format)", COLOR_TEXT),
            ("  Display:       Text-mode GUI", COLOR_TEXT),
            ("  NOT based on Linux or any existing OS.", COLOR_TEXT_DIM),
            ("", COLOR_TEXT),
            ("  (c) MitePC Project", COLOR_TEXT_DIM),
        ];

        for (text, fg) in &lines {
            if y >= cy + ch { break; }
            display.draw_hline(cx, y, cw, ' ', *fg, COLOR_WINDOW_BG);
            display.put_str_at(cx + 1, y, text);
            for j in 0..text.len().min(cw as usize) {
                display.set_cell(cx + 1 + j as u16, y, |c| { c.fg = *fg; });
            }
            y += 1;
        }
    }

    fn app_type(&self) -> AppType { AppType::About }

    fn webview_content_json(&self, _window: &Window, _ctx: &OsContext) -> String {
        // About app content is rendered entirely by the JS frontend
        "{}".to_string()
    }
}

// -- Text Viewer --

pub struct TextViewerApp {
    content: String,
    scroll: u16,
    title: String,
}

impl TextViewerApp {
    pub fn new(title: &str, content: String) -> Self {
        Self { content, scroll: 0, title: title.to_string() }
    }
}

impl App for TextViewerApp {
    fn on_key(&mut self, key: KeyEvent, _window: &mut Window, _fs: &mut MiteFS, _ctx: &OsContext) -> bool {
        match key.code {
            KeyCode::Down => self.scroll = self.scroll.saturating_add(1),
            KeyCode::Up => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Char('q') => return false,
            _ => {}
        }
        true
    }

    fn on_draw(&mut self, display: &mut Display, window: &Window, _ctx: &OsContext) {
        let (cx, cy, cw, ch) = window.content_rect();
        if cw == 0 || ch == 0 { return; }

        let lines: Vec<&str> = self.content.lines().collect();
        let visible = ch as usize;
        let start = self.scroll as usize;

        for i in 0..visible {
            let line_idx = start + i;
            let row_y = cy + i as u16;
            display.draw_hline(cx, row_y, cw, ' ', COLOR_TEXT, COLOR_WINDOW_BG);
            if line_idx < lines.len() {
                let line = lines[line_idx];
                let trunc_len = line.len().min(cw as usize - 2);
                display.put_str_at(cx + 1, row_y, &line[..trunc_len]);
                for j in 0..trunc_len {
                    display.set_cell(cx + 1 + j as u16, row_y, |c| { c.fg = COLOR_TEXT; });
                }
            }
        }

        // Scrollbar indicator
        if lines.len() > visible {
            let bar_h = ch;
            let thumb_pos = (self.scroll as f64 / (lines.len() as f64 - visible as f64) * bar_h as f64) as u16;
            let bar_x = cx + cw - 1;
            for i in 0..bar_h {
                let bg = if i == thumb_pos { COLOR_HIGHLIGHT } else { COLOR_TEXT_DIM };
                display.set_cell(bar_x, cy + i, |c| { c.ch = ' '; c.bg = bg; });
            }
        }
    }

    fn app_type(&self) -> AppType { AppType::TextViewer }

    fn webview_content_json(&self, _window: &Window, _ctx: &OsContext) -> String {
        let lines_json: Vec<String> = self.content.lines()
            .map(|l| l.replace('\\', "\\\\").replace('"', "\\\""))
            .map(|l| format!("\"{}\"", l))
            .collect();
        format!("{{\"lines\":[{}],\"scroll\":{}}}", lines_json.join(","), self.scroll)
    }
}


// ---- MiteOS Kernel ----

/// The MiteOS kernel — manages hardware, the window manager, and applications.
pub struct MiteOS {
    pub ram: Ram,
    pub cpu: Cpu,
    pub fs: MiteFS,
    pub config_cpu_cores: u32,
    pub uptime_secs: u64,
    windows: Vec<Window>,
    apps: HashMap<usize, Box<dyn App>>,
    next_window_id: usize,
    desktop_icons: Vec<(AppType, String, String)>, // (type, label, description)
    selected_icon: usize,
    /// Boot phase messages for splash screen.
    boot_messages: Vec<String>,
    boot_complete: bool,
    boot_scroll: u16,
}


fn draw_window_chrome(display: &mut Display, window: &Window) {
    let border_color = if window.focused { COLOR_WINDOW_BORDER } else { COLOR_TEXT_DIM };
    let title_color = if window.focused { COLOR_WINDOW_TITLE } else { COLOR_TEXT_DIM };
    display.draw_box(window.x, window.y, window.w, window.h, border_color, COLOR_WINDOW_BG,
        if window.focused { Some(&window.title) } else { None });

    // Title bar buttons (close, minimize)
    let btn_x = window.x + window.w - 4;
    let btn_y = window.y;
    display.set_cell(btn_x, btn_y, |c| { c.ch = '['; c.fg = title_color; c.bg = COLOR_WINDOW_BG; });
    display.set_cell(btn_x + 1, btn_y, |c| { c.ch = 'X'; c.fg = COLOR_ERROR; c.bg = COLOR_WINDOW_BG; });
    display.set_cell(btn_x + 2, btn_y, |c| { c.ch = ']'; c.fg = title_color; c.bg = COLOR_WINDOW_BG; });
}

impl MiteOS {
    /// Initialize MiteOS with the given hardware components.
    pub fn new(ram: Ram, cpu: Cpu, fs: MiteFS, cpu_cores: u32) -> Self {
        let desktop_icons = vec![
            (AppType::FileManager, "Files".into(), "Browse MiteFS filesystem".into()),
            (AppType::SystemInfo, "System".into(), "View hardware info".into()),
            (AppType::Terminal, "Terminal".into(), "Command-line interface".into()),
            (AppType::Settings, "Settings".into(), "System preferences".into()),
            (AppType::About, "About".into(), "About MiteOS".into()),
        ];

        let mut os = Self {
            ram,
            cpu,
            fs,
            config_cpu_cores: cpu_cores,
            uptime_secs: 0,
            windows: Vec::new(),
            apps: HashMap::new(),
            next_window_id: 1,
            desktop_icons,
            selected_icon: 0,
            boot_messages: Vec::new(),
            boot_complete: false,
            boot_scroll: 0,
        };

        os.boot();
        os
    }

    /// Boot sequence — initializes OS subsystems.
    fn boot(&mut self) {
        self.boot_messages.push("MiteOS Bootloader v1.0".into());
        self.boot_messages.push("".into());
        self.boot_messages.push("Detecting hardware...".into());
        self.boot_messages.push(format!("  CPU: Mite-16 @ {} MHz", self.cpu.clock_mhz()));
        self.boot_messages.push(format!("  CPU Cores: {}", self.config_cpu_cores));
        self.boot_messages.push(format!("  RAM: {} MB", self.ram.size() / (1024 * 1024)));
        let (_, _, _, _, _, sz) = self.fs.stats();
        self.boot_messages.push(format!("  Storage: {} MB (.mite image)", sz / (1024 * 1024)));
        self.boot_messages.push("".into());

        self.boot_messages.push("Loading MiteMicro kernel...".into());
        self.boot_messages.push("Initializing MiteFS driver...".into());

        // Create default directory structure
        let _ = self.fs.create_dir("/system");
        let _ = self.fs.create_dir("/system/apps");
        let _ = self.fs.create_dir("/system/config");
        let _ = self.fs.create_dir("/users");
        let _ = self.fs.create_dir("/users/default");
        let _ = self.fs.create_dir("/users/default/documents");
        let _ = self.fs.create_dir("/users/default/desktop");
        let _ = self.fs.create_dir("/tmp");

        self.boot_messages.push("Mounting filesystems...".into());
        self.boot_messages.push("  /system   - System files".into());
        self.boot_messages.push("  /users    - User files".into());
        self.boot_messages.push("  /tmp      - Temporary files".into());
        self.boot_messages.push("".into());

        // Create some default files
        let _ = self.fs.create_file("/system/version", b"MiteOS 1.0.0\nKernel: MiteMicro\nArch: mite-16\n");
        let _ = self.fs.create_file("/system/config/hostname", b"mitepc\n");
        let _ = self.fs.create_file("/users/default/documents/readme.txt",
            b"Welcome to MiteOS!\n\nThis is a minimal GUI-based operating system running\nin the MitePC simulator.\n\nMiteOS features:\n- Custom Mite-16 CPU architecture\n- MiteFS filesystem (.mite format)\n- Windowed GUI environment\n- Built-in applications\n\nMiteOS is NOT based on Linux. It is a completely custom\noperating system designed for the MitePC virtual hardware.\n");
        let _ = self.fs.create_file("/users/default/documents/notes.txt",
            b"MiteOS Notes\n============\n\n- The .mite file format uses a custom on-disk structure\n- Block size is 4096 bytes\n- Inodes are 256 bytes each\n- Supports up to 4096 files and directories\n- Block chains allow files of any size\n\nKeyboard shortcuts:\n- Esc: Close window / go back\n- Arrow keys: Navigate\n- Enter: Select / Open\n");

        self.boot_messages.push("Starting window manager...".into());
        self.boot_messages.push("Starting desktop environment...".into());
        self.boot_messages.push("".into());
        self.boot_messages.push("Boot complete. Press Enter to start desktop.".into());
    }

    /// Open an application in a new window.
    pub fn open_app(&mut self, app_type: AppType, display: &Display) {
        let id = self.next_window_id;
        self.next_window_id += 1;

        let (title, w, h): (String, u16, u16) = match app_type {
            AppType::FileManager => ("Files: /".into(), 50, 20),
            AppType::SystemInfo => ("System Information".into(), 48, 22),
            AppType::Terminal => ("Terminal".into(), 60, 20),
            AppType::Settings => ("Settings".into(), 42, 16),
            AppType::About => ("About MiteOS".into(), 44, 22),
            AppType::TextViewer => ("Viewer".into(), 55, 20),
            AppType::Desktop => return,
        };

        // Center window on screen
        let x = (display.width().saturating_sub(w)) / 2;
        let y = ((display.height().saturating_sub(h)).saturating_sub(1)) / 2; // -1 for taskbar

        let mut window = Window::new(id, &title, x, y, w, h, app_type);

        // Create the app
        let mut app: Box<dyn App> = match app_type {
            AppType::FileManager => Box::new(FileManagerApp::new()),
            AppType::SystemInfo => Box::new(SystemInfoApp::new()),
            AppType::Terminal => Box::new(TerminalApp::new()),
            AppType::Settings => Box::new(SettingsApp::new()),
            AppType::About => Box::new(AboutApp::new()),
            AppType::TextViewer => Box::new(TextViewerApp::new(&title, String::new())),
            AppType::Desktop => unreachable!(),
        };

        app.on_open(&mut window, &mut self.fs);
        self.apps.insert(id, app);

        // Unfocus all other windows
        for w in &mut self.windows {
            w.focused = false;
        }
        window.focused = true;
        self.windows.push(window);
    }

    /// Close a window by ID.
    pub fn close_window(&mut self, id: usize) {
        self.apps.remove(&id);
        self.windows.retain(|w| w.id != id);
        // Focus the last remaining window
        if let Some(last) = self.windows.last_mut() {
            last.focused = true;
        }
    }

    /// Get the focused window ID, if any.
    fn focused_window_id(&self) -> Option<usize> {
        self.windows.iter().find(|w| w.focused).map(|w| w.id)
    }

    /// Handle a key event from the display.
    pub fn handle_key(&mut self, key: KeyEvent, display: &Display) {
        // During boot sequence
        if !self.boot_complete {
            if key.code == KeyCode::Enter {
                self.boot_complete = true;
            }
            return;
        }

        // Alt+F4 or Ctrl+W to close focused window
        if key.code == KeyCode::F(4) && key.modifiers.contains(KeyModifiers::ALT) {
            if let Some(id) = self.focused_window_id() {
                self.close_window(id);
                return;
            }
        }
        if key.code == KeyCode::Char('w') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if let Some(id) = self.focused_window_id() {
                self.close_window(id);
                return;
            }
        }

        // If a window is focused, route input to it
        if let Some(id) = self.focused_window_id() {
            // Build context snapshot first (immutable borrow, released immediately)
            let ctx = OsContext::from_os(self);
            // Extract the app temporarily to avoid double-borrow of self
            let mut app = self.apps.remove(&id);
            let consumed = if let Some(ref mut app) = app {
                let window = self.windows.iter_mut().find(|w| w.id == id).unwrap();
                app.on_key(key, window, &mut self.fs, &ctx)
            } else {
                false
            };
            if let Some(app) = app {
                self.apps.insert(id, app);
            }
            if consumed {
                return;
            }
        }

        // Desktop navigation
        if self.windows.is_empty() {
            match key.code {
                KeyCode::Left => self.selected_icon = self.selected_icon.saturating_sub(1),
                KeyCode::Right => self.selected_icon = (self.selected_icon + 1).min(self.desktop_icons.len() - 1),
                KeyCode::Up => self.selected_icon = self.selected_icon.saturating_sub(5),
                KeyCode::Down => self.selected_icon = (self.selected_icon + 5).min(self.desktop_icons.len() - 1),
                KeyCode::Enter => {
                    if let Some((app_type, _, _)) = self.desktop_icons.get(self.selected_icon) {
                        self.open_app(*app_type, display);
                    }
                }
                KeyCode::Esc => return,
                _ => {}
            }
        } else {
            // Window management keys (not consumed by app)
            match key.code {
                KeyCode::Esc => {
                    if let Some(id) = self.focused_window_id() {
                        self.close_window(id);
                    }
                }
                KeyCode::Tab => {
                    // Cycle focus
                    let n = self.windows.len();
                    if n > 1 {
                        let focused_idx = self.windows.iter().position(|w| w.focused).unwrap_or(0);
                        self.windows[focused_idx].focused = false;
                        let next = (focused_idx + 1) % n;
                        self.windows[next].focused = true;
                    }
                }
                _ => {}
            }
        }
    }

    /// Render the entire MiteOS desktop to the display.
    pub fn render(&mut self, display: &mut Display) {
        let w = display.width();
        let h = display.height();
        let taskbar_h = 1u16;
        let desktop_h = h - taskbar_h;

        if !self.boot_complete {
            self.render_boot(display);
            return;
        }

        // Desktop background
        display.fill_rect(0, 0, w, desktop_h, ' ', COLOR_TEXT_DIM, COLOR_DESKTOP_BG);

        // Desktop icons (if no windows or as background)
        let icon_cols = 5u16;
        let icon_rows = ((self.desktop_icons.len() as u16 + icon_cols - 1) / icon_cols).max(1);
        let icon_area_w = w / icon_cols;
        let icon_start_y = (desktop_h.saturating_sub(icon_rows * 4)) / 2;

        if self.windows.is_empty() {
            for (i, (app_type, label, _desc)) in self.desktop_icons.iter().enumerate() {
                let col = (i as u16) % icon_cols;
                let row = (i as u16) / icon_cols;
                let ix = col * icon_area_w + icon_area_w / 2 - 5;
                let iy = icon_start_y + row * 4;

                let is_sel = i == self.selected_icon;
                let bg = if is_sel { COLOR_SELECTION } else { COLOR_DESKTOP_BG };
                let fg = if is_sel { Color::White } else { COLOR_ICON_FG };

                // Icon box
                display.draw_box(ix, iy, 10, 3, fg, bg, None);
                // Icon symbol
                let sym = app_type.icon_char();
                display.set_cell(ix + 4, iy + 1, |c| { c.ch = sym; c.fg = fg; c.bg = bg; });
                // Label
                display.put_str_at(ix, iy + 3, label);
                for j in 0..label.len().min(10) {
                    display.set_cell(ix + j as u16, iy + 3, |c| { c.fg = fg; c.bg = COLOR_DESKTOP_BG; });
                }
            }
        }

        // Render windows (back to front)
        // Render windows (back to front)
        let ctx = OsContext::from_os(self);
        let window_snapshots: Vec<(usize, Window)> = self.windows.iter()
            .filter(|w| w.visible)
            .map(|w| (w.id, w.clone()))
            .collect();
        for (id, window) in window_snapshots {
            draw_window_chrome(display, &window);
            if let Some(app) = self.apps.get_mut(&id) {
                app.on_draw(display, &window, &ctx);
            }
        }
        // Taskbar
        self.render_taskbar(display);
    }

/// Draw window chrome (border, title bar, close button) — free function to avoid borrow issues.

    fn render_taskbar(&self, display: &mut Display) {
        let w = display.width();
        let h = display.height();
        let y = h - 1;

        // Background
        display.draw_hline(0, y, w, ' ', COLOR_TASKBAR_FG, COLOR_TASKBAR_BG);

        // Left: MiteOS logo
        let logo = " MiteOS ";
        display.put_str_at(0, y, logo);
        for j in 0..logo.len() {
            display.set_cell(j as u16, y, |c| { c.fg = COLOR_ACCENT; c.bg = COLOR_TASKBAR_BG; c.bold = true; });
        }

        // Center: Window buttons
        let center_start = 12u16;
        for (i, window) in self.windows.iter().enumerate() {
            let wx = center_start + (i as u16 * 16);
            if wx + 14 >= w { break; }
            let bg = if window.focused { COLOR_HIGHLIGHT } else { COLOR_TASKBAR_BG };
            let fg = if window.focused { Color::White } else { COLOR_TASKBAR_FG };
            let label = if window.title.len() > 12 { &window.title[..12] } else { &window.title };
            let btn = format!(" {} ", label);
            display.put_str_at(wx, y, &btn);
            for j in 0..btn.len() {
                display.set_cell(wx + j as u16, y, |c| { c.fg = fg; c.bg = bg; });
            }
        }

        // Right: Clock
        let datetime = Local::now().format("%H:%M").to_string();
        let clock = format!(" {} ", datetime);
        let clock_start = (w as i16 - clock.len() as i16).max(0) as u16;
        display.put_str_at(clock_start, y, &clock);
        for j in 0..clock.len() {
            display.set_cell(clock_start + j as u16, y, |c| { c.fg = COLOR_TASKBAR_FG; c.bg = COLOR_TASKBAR_BG; });
        }

        // System tray indicators
        let tray = format!("RAM:{:.0}% ", self.ram.usage_percent());
        let tray_start = clock_start.saturating_sub(tray.len() as u16);
        display.put_str_at(tray_start, y, &tray);
        for j in 0..tray.len() {
            let color = if self.ram.usage_percent() > 80.0 { COLOR_ERROR } else { COLOR_SUCCESS };
            display.set_cell(tray_start + j as u16, y, |c| { c.fg = color; c.bg = COLOR_TASKBAR_BG; });
        }
    }

    fn render_boot(&mut self, display: &mut Display) {
        let w = display.width();
        let h = display.height();

        // Dark background
        display.fill_rect(0, 0, w, h, ' ', COLOR_TEXT_DIM, Color::Black);

        // Boot messages
        let start_y = 2u16;
        for (i, msg) in self.boot_messages.iter().enumerate() {
            let y = start_y + i as u16;
            if y >= h { break; }
            display.put_str_at(2, y, msg);
            let fg = if msg.contains("...") { COLOR_TEXT } else if msg.contains("complete") { COLOR_SUCCESS } else { COLOR_TEXT_DIM };
            for j in 0..msg.len().min((w - 4) as usize) {
                display.set_cell(2 + j as u16, y, |c| { c.fg = fg; });
            }
        }

        // Blinking cursor at the end
        let cursor_y = start_y + self.boot_messages.len() as u16;
        if cursor_y < h {
            let ch = if self.uptime_secs % 2 == 0 { '_' } else { ' ' };
            display.set_cell(2, cursor_y, |c| { c.ch = ch; c.fg = COLOR_SUCCESS; });
        }

        // Bottom info
        let info = "MitePC Simulator - Mite-16 Architecture - MiteFS Filesystem";
        let info_y = h - 2;
        display.put_str_at(2, info_y, info);
        for j in 0..info.len().min((w - 4) as usize) {
            display.set_cell(2 + j as u16, info_y, |c| { c.fg = COLOR_TEXT_DIM; });
        }
    }

    /// Update internal state (uptime, etc.). Called every second.
    pub fn tick(&mut self) {
        self.uptime_secs += 1;
    }

    // ---- WebView backend accessors ----
    // These methods expose internal state for the webview.rs module
    // to build JSON state updates for the HTML/CSS/JS frontend.
    // They are only used when the "gui" feature is enabled.
    #[allow(dead_code)]
    /// Whether the boot sequence is complete.
    pub fn boot_complete(&self) -> bool {
        self.boot_complete
    }

    /// Get a copy of the boot messages.
    #[allow(dead_code)]
    pub fn boot_messages(&self) -> Vec<String> {
        self.boot_messages.clone()
    }

    /// Force the boot sequence to complete (called from webview on Enter).
    #[allow(dead_code)]
    pub fn force_boot_complete(&mut self) {
        self.boot_complete = true;
    }

    /// Get the desktop icons (type, label, description).
    #[allow(dead_code)]
    pub fn desktop_icons(&self) -> &[(AppType, String, String)] {
        &self.desktop_icons
    }

    /// Get the currently selected desktop icon index.
    #[allow(dead_code)]
    pub fn selected_icon(&self) -> usize {
        self.selected_icon
    }

    /// Get a reference to all windows.
    #[allow(dead_code)]
    pub fn windows(&self) -> &[Window] {
        &self.windows
    }

    /// Iterate over windows and their apps simultaneously for state serialization.
    /// Returns (window_id, window_ref, app_ref) tuples.
    #[allow(dead_code)]
    pub fn windows_and_apps(&self) -> Vec<(usize, Window, &dyn App)> {
        self.windows.iter().filter_map(|w| {
            self.apps.get(&w.id).map(|app| (w.id, w.clone(), app.as_ref()))
        }).collect()
    }

    /// Open an app by desktop icon index (called from webview on icon click/double-click).
    #[allow(dead_code)]
    pub fn open_app_by_index(&mut self, index: usize) {
        if let Some((app_type, _, _)) = self.desktop_icons.get(index) {
            // Create a dummy display for window sizing (webview doesn't use it for rendering)
            let display = Display::new(120, 40);
            self.open_app(*app_type, &display);
        }
    }

    /// Focus a specific window by ID (called from webview on window click).
    #[allow(dead_code)]
    pub fn focus_window(&mut self, id: usize) {
        for w in &mut self.windows {
            w.focused = w.id == id;
        }
    }

    /// Handle a key event (called from the webview backend).
    /// Uses a synthetic display for window sizing since the webview
    /// doesn't use the crossterm Display for rendering.
    #[allow(dead_code)]
    pub fn handle_key_webview(&mut self, key: KeyEvent) {
        let display = Display::new(120, 40);
        self.handle_key(key, &display);
    }
}
