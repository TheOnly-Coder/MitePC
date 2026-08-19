/**
 * MitePC — A PC Simulator in Rust
 *
 * Simulates its own RAM, CPU, and Storage (MiteFS).
 * Runs MiteOS, a minimal GUI-based operating system
 * stored in the custom .mite file format.
 *
 * MiteOS and MiteFS are NOT based on Linux.
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

use config::SimulatorConfig;
use simulator::Simulator;
use std::path::Path;

fn main() {
    println!("MitePC Simulator v1.0.0");
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
    println!();

    // Build and run the simulator
    let mut sim = match Simulator::build(config) {
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
