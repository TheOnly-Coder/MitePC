/**
 * The MitePC Simulator — ties all hardware and software components together.
 *
 * The simulator supports two display backends:
 * 1. **WebView (GUI mode)** — When `os_type = gui` in setup.conf, opens a
 *    native window via Boscop/web-view (WebKitGTK/WebView2/WKWebView) and
 *    renders MiteOS as a real HTML/CSS/JS GUI. This is used for GUI-based
 *    operating systems like MiteOS.
 * 2. **Crossterm (Terminal mode)** — When `os_type = terminal`, renders
 *    MiteOS as a character-cell display in the host terminal. This is used
 *    for terminal-only operating systems.
 *
 * The backend is selected at startup based on the configuration.
 */

use crate::config::SimulatorConfig;
use crate::display::Display;
use crate::miteos::MiteOS;
use crossterm::event::{KeyCode, KeyModifiers};
use std::io;
use std::thread;
use std::time::{Duration, Instant};

/// The main simulator engine (crossterm/terminal backend).
/// When os_type = gui, the webview module handles the simulation instead.
pub struct Simulator {
    pub config: SimulatorConfig,
    pub display: Display,
    pub os: MiteOS,
    running: bool,
    stdout: io::Stdout,
}

impl Simulator {
    /// Build the simulator from configuration (crossterm backend).
    pub fn build(config: SimulatorConfig) -> Result<Self, String> {
        let ram = crate::ram::Ram::new(config.ram_mb);
        let cpu = crate::cpu::Cpu::new(config.cpu_mhz);

        // Create storage directory if needed
        std::fs::create_dir_all(&config.storage_dir)
            .map_err(|e| format!("Failed to create storage dir '{}': {}",
                config.storage_dir.display(), e))?;

        // Initialize MiteFS (create or open .mite image)
        let fs = crate::mitefs::MiteFS::open_or_create(&config.os_image, config.storage_mb)?;

        // Initialize display (before entering raw mode)
        let display = Display::from_terminal()
            .map_err(|e| format!("Failed to create display: {}", e))?;

        // Initialize MiteOS (takes ownership of ram, cpu, fs)
        let os = MiteOS::new(ram, cpu, fs, config.cpu_cores);

        Ok(Self {
            config,
            display,
            os,
            running: true,
            stdout: io::stdout(),
        })
    }

    /// Print boot information to stderr (visible before raw mode).
    fn print_boot_info(&self) {
        let ram_mb = self.os.ram.size() / (1024 * 1024);
        let storage_path = self.config.os_image.display();
        eprintln!("╔══════════════════════════════════════════╗");
        eprintln!("║         MitePC Simulator v0.2.0           ║");
        eprintln!("╠══════════════════════════════════════════╣");
        eprintln!("║  Display:  Crossterm (host terminal)      ║");
        eprintln!("║  CPU:      Mite-16 @ {:>4} MHz           ║", self.os.cpu.clock_mhz());
        eprintln!("║  Cores:    {:>4}                            ║", self.os.config_cpu_cores);
        eprintln!("║  RAM:      {:>4} MB                       ║", ram_mb);
        eprintln!("║  Storage:  {:>4} MB (.mite image)       ║", self.config.storage_mb);
        eprintln!("║  Image:    {:<30}║", storage_path);
        eprintln!("╠══════════════════════════════════════════╣");
        eprintln!("║  Starting MiteOS...  Ctrl+C to exit      ║");
        eprintln!("╚══════════════════════════════════════════╝");
        eprintln!();
    }

    /// Run the main simulation loop (crossterm backend). Blocks until the user exits.
    pub fn run(&mut self) -> Result<(), String> {
        self.print_boot_info();

        // Enter raw terminal mode (alternate screen)
        Display::enter_raw_mode(&mut self.stdout)
            .map_err(|e| format!("Failed to enter raw mode: {}", e))?;

        let mut last_tick = Instant::now();

        let result = self.main_loop(&mut last_tick);

        // Always restore terminal on exit
        let _ = Display::exit_raw_mode(&mut self.stdout);
        eprintln!("\nMitePC simulator stopped.");

        result
    }

    fn main_loop(&mut self, last_tick: &mut Instant) -> Result<(), String> {
        loop {
            if !self.running {
                break;
            }

            // Read input with short timeout (non-blocking)
            match Display::read_key(16) {
                Ok(Some(key)) => {
                    match (key.code, key.modifiers) {
                        // Global: Ctrl+C exits
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                            self.running = false;
                            break;
                        }
                        _ => {
                            self.os.handle_key(key, &self.display);
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    if e.kind() == io::ErrorKind::Interrupted {
                        self.running = false;
                        break;
                    }
                }
            }

            // Tick the OS every second (uptime counter)
            if last_tick.elapsed() >= Duration::from_secs(1) {
                self.os.tick();
                *last_tick = Instant::now();
            }

            // Run simulated CPU cycles for realism
            self.os.cpu.run_cycles(&mut self.os.ram, 100);

            // Render the OS to the display, then flush to terminal
            self.os.render(&mut self.display);
            self.display.flush(&mut self.stdout)
                .map_err(|e| format!("Display error: {}", e))?;

            // ~30 FPS frame cap
            thread::sleep(Duration::from_millis(33));
        }
        Ok(())
    }

    /// Stop the simulator (can be called from another thread).
    pub fn stop(&mut self) {
        self.running = false;
    }
}

impl Drop for Simulator {
    fn drop(&mut self) {
        let _ = Display::exit_raw_mode(&mut self.stdout);
    }
}