/**
 * WebView display backend — renders MiteOS as a real GUI using Boscop/web-view.
 *
 * When `os_type = gui` in setup.conf, the simulator opens a native window
 * containing an embedded browser (WebKitGTK on Linux, WebView2 on Windows,
 * WKWebView on macOS) that renders the MiteOS desktop environment using
 * HTML, CSS, and JavaScript.
 *
 * Terminal-only OSes (`os_type = terminal`) use the crossterm backend instead.
 *
 * Communication protocol:
 *   JS → Rust: window.external.invoke(JSON.stringify({type, ...params}))
 *   Rust → JS: webview.eval("updateState(" + JSON.stringify(state) + ")")
 */

#[cfg(feature = "gui")]
pub use gui_backend::*;

#[cfg(feature = "gui")]
mod gui_backend {
    use crate::config::SimulatorConfig;
    use crate::mitefs::MiteFS;
    use crate::miteos::{
        MiteOS, OsContext,
        COLOR_DESKTOP_BG, COLOR_WINDOW_BG, COLOR_WINDOW_BORDER, COLOR_WINDOW_TITLE,
        COLOR_TEXT, COLOR_TEXT_DIM, COLOR_HIGHLIGHT, COLOR_ACCENT, COLOR_SUCCESS,
        COLOR_ERROR, COLOR_ICON_FG, COLOR_ICON_BG, COLOR_SELECTION, COLOR_INPUT_BG,
        COLOR_TASKBAR_BG, COLOR_TASKBAR_FG,
    };
    use crate::ram::Ram;
    use crate::cpu::Cpu;
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use web_view::*;

    /// State held inside the webview. Contains the MiteOS instance and
    /// a shared flag so the main thread can signal shutdown.
    pub struct WvState {
        pub os: MiteOS,
        pub config: SimulatorConfig,
        pub running: Arc<AtomicBool>,
    }

    // ------------------------------------------------------------------
    // JSON helper — convert crossterm Color to a CSS hex string
    // ------------------------------------------------------------------

    fn color_to_css(c: crossterm::style::Color) -> String {
        match c {
            crossterm::style::Color::Rgb { r, g, b } => {
                format!("#{:02x}{:02x}{:02x}", r, g, b)
            }
            crossterm::style::Color::Black => "#000000".into(),
            crossterm::style::Color::White => "#ffffff".into(),
            crossterm::style::Color::Grey => "#808080".into(),
            crossterm::style::Color::DarkGrey => "#404040".into(),
            crossterm::style::Color::Red => "#ff0000".into(),
            crossterm::style::Color::DarkRed => "#800000".into(),
            crossterm::style::Color::Green => "#00ff00".into(),
            crossterm::style::Color::DarkGreen => "#008000".into(),
            crossterm::style::Color::Yellow => "#ffff00".into(),
            crossterm::style::Color::DarkYellow => "#808000".into(),
            crossterm::style::Color::Blue => "#0000ff".into(),
            crossterm::style::Color::DarkBlue => "#000080".into(),
            crossterm::style::Color::Magenta => "#ff00ff".into(),
            crossterm::style::Color::DarkMagenta => "#800080".into(),
            crossterm::style::Color::Cyan => "#00ffff".into(),
            crossterm::style::Color::DarkCyan => "#008080".into(),
            _ => "#c0c0c0".into(),
        }
    }

    // ------------------------------------------------------------------
    // Build the full render state as JSON
    // ------------------------------------------------------------------

    fn build_state(os: &MiteOS) -> Value {
        let ctx = OsContext::from_os(os);

        if !os.boot_complete() {
            return json!({
                "phase": "boot",
                "boot": {
                    "messages": os.boot_messages(),
                    "cursorVisible": ctx.uptime_secs % 2 == 0
                }
            });
        }

        // Desktop icons
        let icons: Vec<Value> = os.desktop_icons().iter().enumerate().map(|(i, (at, label, desc))| {
            json!({
                "index": i,
                "type": at.label(),
                "label": label,
                "description": desc,
                "iconChar": at.icon_char().to_string(),
            })
        }).collect();

        // Windows with their app content
        let mut windows_json = Vec::new();
        for (id, window, app) in os.windows_and_apps() {
            let content_json = app.webview_content_json(&window, &ctx);
            let content_val: Value = serde_json::from_str(&content_json).unwrap_or(json!({}));
            windows_json.push(json!({
                "id": id,
                "title": window.title,
                "x": window.x,
                "y": window.y,
                "w": window.w,
                "h": window.h,
                "focused": window.focused,
                "visible": window.visible,
                "appType": window.app_type.label(),
                "content": content_val,
            }));
        }

        let datetime = chrono::Local::now().format("%H:%M").to_string();
        let ram_pct = ctx.ram_usage_percent;

        json!({
            "phase": "desktop",
            "desktop": {
                "icons": icons,
                "selectedIcon": os.selected_icon(),
                "bgColor": color_to_css(COLOR_DESKTOP_BG),
            },
            "windows": windows_json,
            "taskbar": {
                "time": datetime,
                "ramPercent": ram_pct,
                "windowButtons": os.windows().iter().map(|w| json!({
                    "id": w.id,
                    "title": w.title.clone(),
                    "focused": w.focused,
                })).collect::<Vec<_>>(),
                "bgColor": color_to_css(COLOR_TASKBAR_BG),
                "fgColor": color_to_css(COLOR_TASKBAR_FG),
                "accentColor": color_to_css(COLOR_ACCENT),
            },
            "colors": {
                "desktopBg": color_to_css(COLOR_DESKTOP_BG),
                "windowBg": color_to_css(COLOR_WINDOW_BG),
                "windowBorder": color_to_css(COLOR_WINDOW_BORDER),
                "windowTitle": color_to_css(COLOR_WINDOW_TITLE),
                "text": color_to_css(COLOR_TEXT),
                "textDim": color_to_css(COLOR_TEXT_DIM),
                "highlight": color_to_css(COLOR_HIGHLIGHT),
                "accent": color_to_css(COLOR_ACCENT),
                "success": color_to_css(COLOR_SUCCESS),
                "error": color_to_css(COLOR_ERROR),
                "iconFg": color_to_css(COLOR_ICON_FG),
                "iconBg": color_to_css(COLOR_ICON_BG),
                "selection": color_to_css(COLOR_SELECTION),
                "inputBg": color_to_css(COLOR_INPUT_BG),
            },
            "systemInfo": {
                "cpuMhz": ctx.cpu_mhz,
                "cpuCores": ctx.cpu_cores,
                "cpuInstructions": ctx.cpu_instructions,
                "ramSize": ctx.ram_size,
                "ramUsedPages": ctx.ram_used_pages,
                "ramTotalPages": ctx.ram_total_pages,
                "ramUsagePercent": ctx.ram_usage_percent,
                "uptimeSecs": ctx.uptime_secs,
                "fsTotalBlocks": ctx.fs_total_blocks,
                "fsFreeBlocks": ctx.fs_free_blocks,
                "fsTotalInodes": ctx.fs_total_inodes,
                "fsFreeInodes": ctx.fs_free_inodes,
                "fsTotalSize": ctx.fs_total_size,
            }
        })
    }

    // ------------------------------------------------------------------
    // Convert a JS key event into a crossterm KeyEvent
    // ------------------------------------------------------------------

    fn json_to_keyevent(val: &Value) -> crossterm::event::KeyEvent {
        use crossterm::event::{KeyCode, KeyModifiers};

        let code = val["code"].as_str().unwrap_or("");
        let key = val["key"].as_str().unwrap_or("");
        let ctrl = val["ctrlKey"].as_bool().unwrap_or(false);
        let alt = val["altKey"].as_bool().unwrap_or(false);
        let shift = val["shiftKey"].as_bool().unwrap_or(false);

        let mut mods = KeyModifiers::NONE;
        if ctrl { mods.insert(KeyModifiers::CONTROL); }
        if alt { mods.insert(KeyModifiers::ALT); }
        if shift { mods.insert(KeyModifiers::SHIFT); }

        let kc = match code {
            "Enter" => KeyCode::Enter,
            "Backspace" => KeyCode::Backspace,
            "Tab" => KeyCode::Tab,
            "Escape" => KeyCode::Esc,
            "ArrowUp" => KeyCode::Up,
            "ArrowDown" => KeyCode::Down,
            "ArrowLeft" => KeyCode::Left,
            "ArrowRight" => KeyCode::Right,
            "F1" => KeyCode::F(1),
            "F2" => KeyCode::F(2),
            "F3" => KeyCode::F(3),
            "F4" => KeyCode::F(4),
            "F5" => KeyCode::F(5),
            "F6" => KeyCode::F(6),
            "F7" => KeyCode::F(7),
            "F8" => KeyCode::F(8),
            "F9" => KeyCode::F(9),
            "F10" => KeyCode::F(10),
            "F11" => KeyCode::F(11),
            "F12" => KeyCode::F(12),
            "Home" => KeyCode::Home,
            "End" => KeyCode::End,
            "PageUp" => KeyCode::PageUp,
            "PageDown" => KeyCode::PageDown,
            "Delete" => KeyCode::Delete,
            "Insert" => KeyCode::Insert,
            _ => {
                // Character key
                if key.len() == 1 {
                    KeyCode::Char(key.chars().next().unwrap())
                } else {
                    KeyCode::Null
                }
            }
        };

        crossterm::event::KeyEvent::new(kc, mods)
    }

    // ------------------------------------------------------------------
    // Invoke handler — JS calls this with JSON commands
    // ------------------------------------------------------------------

    fn handle_invoke(webview: &mut WebView<WvState>, arg: &str) -> WVResult {
        let cmd: Value = match serde_json::from_str(arg) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };

        let cmd_type = cmd["type"].as_str().unwrap_or("");

        match cmd_type {
            "init" | "poll" => {
                // On poll: tick the OS, run CPU cycles, then send state
                let running = webview.user_data().running.load(Ordering::SeqCst);
                if !running {
                    let _ = webview.eval("if(window.close) window.close();");
                    return Ok(());
                }
                if cmd_type == "poll" {
                    webview.user_data_mut().os.tick();
                }
                {
                    let data = webview.user_data_mut();
                    data.os.cpu.run_cycles(&mut data.os.ram, 100);
                }
                let state = build_state(&webview.user_data().os);
                let state_str = serde_json::to_string(&state)
                    .map_err(|e| WVError::Custom(e.to_string()))?;
                let _ = webview.eval(&format!("updateState({})", state_str));
            }

            "key" => {
                let key = json_to_keyevent(&cmd);
                let _state_before = webview.user_data().os.boot_complete();
                webview.user_data_mut().os.handle_key_webview(key);
                let state = build_state(&webview.user_data().os);
                let state_str = serde_json::to_string(&state)
                    .map_err(|e| WVError::Custom(e.to_string()))?;
                let _ = webview.eval(&format!("updateState({})", state_str));
            }

            "click_icon" => {
                if let Some(idx) = cmd["index"].as_u64() {
                    webview.user_data_mut().os.open_app_by_index(idx as usize);
                    let state = build_state(&webview.user_data().os);
                    let state_str = serde_json::to_string(&state)
                    .map_err(|e| WVError::Custom(e.to_string()))?;
                    let _ = webview.eval(&format!("updateState({})", state_str));
                }
            }

            "close_window" => {
                if let Some(id) = cmd["id"].as_u64() {
                    webview.user_data_mut().os.close_window(id as usize);
                    let state = build_state(&webview.user_data().os);
                    let state_str = serde_json::to_string(&state)
                    .map_err(|e| WVError::Custom(e.to_string()))?;
                    let _ = webview.eval(&format!("updateState({})", state_str));
                }
            }

            "focus_window" => {
                if let Some(id) = cmd["id"].as_u64() {
                    webview.user_data_mut().os.focus_window(id as usize);
                    let state = build_state(&webview.user_data().os);
                    let state_str = serde_json::to_string(&state)
                    .map_err(|e| WVError::Custom(e.to_string()))?;
                    let _ = webview.eval(&format!("updateState({})", state_str));
                }
            }

            "boot_enter" => {
                webview.user_data_mut().os.force_boot_complete();
                let state = build_state(&webview.user_data().os);
                let state_str = serde_json::to_string(&state)
                    .map_err(|e| WVError::Custom(e.to_string()))?;
                let _ = webview.eval(&format!("updateState({})", state_str));
            }

            _ => {}
        }

        Ok(())
    }

    // ------------------------------------------------------------------
    // The embedded HTML/CSS/JS frontend
    // ------------------------------------------------------------------

    fn get_html() -> &'static str {
        include_str!("miteos_gui.html")
    }

    // ------------------------------------------------------------------
    // Public entry point: build and run the webview simulator
    // ------------------------------------------------------------------

    /// Launch the MitePC simulator in GUI mode using web-view.
    /// This opens a native window with an embedded browser rendering the
    /// MiteOS desktop environment. Blocks until the window is closed.
    pub fn run_webview(config: SimulatorConfig) -> Result<(), String> {
        let ram = Ram::new(config.ram_mb);
        let cpu = Cpu::new(config.cpu_mhz);

        std::fs::create_dir_all(&config.storage_dir)
            .map_err(|e| format!("Failed to create storage dir '{}': {}",
                config.storage_dir.display(), e))?;

        let fs = MiteFS::open_or_create(&config.os_image, config.storage_mb)?;
        let os = MiteOS::new(ram, cpu, fs, config.cpu_cores);
        let running = Arc::new(AtomicBool::new(true));

        let state = WvState { os, config, running: running.clone() };

        // Print boot info to stderr (visible in terminal before window opens)
        eprintln!("╔══════════════════════════════════════════╗");
        eprintln!("║       MitePC Simulator v0.2.0 (GUI)      ║");
        eprintln!("╠══════════════════════════════════════════╣");
        eprintln!("║  Display:  WebView (native GUI window)   ║");
        eprintln!("║  Backend:  {}                ║",
            if cfg!(target_os = "linux") { "WebKitGTK" }
            else if cfg!(target_os = "windows") { "WebView2" }
            else { "WKWebView" });
        eprintln!("║  Close the window to exit.               ║");
        eprintln!("╚══════════════════════════════════════════╝");
        eprintln!();

        web_view::builder()
            .title("MitePC — MiteOS")
            .content(Content::Html(get_html()))
            .size(1024, 700)
            .resizable(true)
            .debug(true)
            .user_data(state)
            .invoke_handler(handle_invoke)
            .run()
            .map_err(|e| format!("WebView error: {}", e))?;

        running.store(false, Ordering::SeqCst);
        eprintln!("MitePC simulator stopped.");
        Ok(())
    }
}
