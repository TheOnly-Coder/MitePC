/**
 * MitePC — A PC Simulator in Rust
 *
 * Simulates its own RAM, CPU, and Storage (MiteFS).
 * Runs MiteOS, a minimal GUI-based operating system
 * stored in the custom .mite file format.
 *
 * MiteOS and MiteFS are NOT based on Linux.
 *
 * Display backends:
 *   - gui (default): Opens a real GUI window using Boscop/web-view.
 *     MiteOS is rendered as HTML/CSS/JS inside a native browser window.
 *     Requires the "gui" feature flag (enabled by default) and system
 *     WebView libraries (WebKitGTK on Linux, WebView2 on Windows,
 *     WKWebView on macOS).
 *   - terminal: Renders MiteOS in the host terminal using crossterm.
 *     Used for terminal-only operating systems. No extra dependencies.
 *
 * Configuration is read from setup.conf in the current directory.
 */

mod config;
mod cpu;
mod display;
mod mitefs;
mod miteos;
mod ram;
mod simulator;
#[cfg(feature = "gui")]
mod webview;

use config::{OsType, SimulatorConfig};
use std::path::Path;

fn main() {
    println!("MitePC Simulator v0.2.0");
    println!("====================");
    println!();

    // Locate and parse setup.conf
    let config_path = Path::new("setup.conf");
    let config = match SimulatorConfig::from_file(config_path) {
        Ok(c) => {
            println!("Loaded configuration from setup.conf");
            c
        }
        Err(e) => {
            eprintln!("Warning: {}", e);
            eprintln!("Using default configuration.");
            SimulatorConfig::default()
        }
    };

    println!("  RAM:      {} MB", config.ram_mb);
    println!("  CPU:      {} core(s) @ {} MHz", config.cpu_cores, config.cpu_mhz);
    println!("  Storage:  {} MB", config.storage_mb);
    println!("  OS image: {}", config.os_image.display());
    println!("  Store dir:{}", config.storage_dir.display());
    println!("  Display:  {}", match config.os_type {
        OsType::Gui => "GUI (web-view)",
        OsType::Terminal => "Terminal (crossterm)",
    });
    println!();

    // Route to the appropriate display backend
    match config.os_type {
        OsType::Gui => {
            #[cfg(feature = "gui")]
            {
                if let Err(e) = webview::run_webview(config) {
                    eprintln!("FATAL: {}", e);
                    std::process::exit(1);
                }
            }
            #[cfg(not(feature = "gui"))]
            {
                eprintln!("FATAL: GUI mode requires the 'gui' feature flag.");
                eprintln!("Rebuild with: cargo build --features gui");
                eprintln!("Or set os_type = terminal in setup.conf.");
                std::process::exit(1);
            }
        }
        OsType::Terminal => {
            // Use the crossterm-based terminal backend
            let mut sim = match simulator::Simulator::build(config) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("FATAL: Failed to initialize simulator: {}", e);
                    std::process::exit(1);
                }
            };
            if let Err(e) = sim.run() {
                eprintln!("Simulator error: {}", e);
                std::process::exit(1);
            }
        }
    }
}
