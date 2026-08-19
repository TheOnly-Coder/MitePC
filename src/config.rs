/// Configuration for the MitePC simulator, parsed from setup.conf

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// The display mode for the simulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsType {
    /// Real GUI window via web-view (for GUI-based OSes like MiteOS)
    Gui,
    /// Host terminal via crossterm (for terminal-only OSes)
    Terminal,
}

impl Default for OsType {
    fn default() -> Self {
        Self::Gui
    }
}

impl std::fmt::Display for OsType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OsType::Gui => write!(f, "gui"),
            OsType::Terminal => write!(f, "terminal"),
        }
    }
}

impl OsType {
    fn from_str_opt(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "gui" => Some(OsType::Gui),
            "terminal" => Some(OsType::Terminal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SimulatorConfig {
    pub ram_mb: u64,
    pub cpu_cores: u32,
    pub cpu_mhz: u32,
    pub storage_mb: u64,
    pub os_image: PathBuf,
    pub storage_dir: PathBuf,
    pub os_type: OsType,
}

impl Default for SimulatorConfig {
    fn default() -> Self {
        Self {
            ram_mb: 1024,
            cpu_cores: 1,
            cpu_mhz: 800,
            storage_mb: 4096,
            os_image: PathBuf::from("./miteos.mite"),
            storage_dir: PathBuf::from("./mite"),
            os_type: OsType::Gui,
        }
    }
}

impl SimulatorConfig {
    /// Parse a setup.conf file and return the configuration.
    /// Falls back to defaults for missing or invalid values.
    pub fn from_file(path: &std::path::Path) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file '{}': {}", path.display(), e))?;
        Self::parse(&content)
    }

    pub fn parse(content: &str) -> Result<Self, String> {
        let mut map: HashMap<String, String> = HashMap::new();

        for (lineno, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().to_lowercase();
                let value = value.trim().to_string();
                if !key.is_empty() && !value.is_empty() {
                    map.insert(key, value);
                }
            } else {
                return Err(format!("Invalid config line {}: '{}' (expected key = value)", lineno + 1, line));
            }
        }

        let mut config = SimulatorConfig::default();

        if let Some(v) = map.get("ram_mb") {
            config.ram_mb = v.parse::<u64>().unwrap_or(config.ram_mb)
                .clamp(64, 16384);
        }
        if let Some(v) = map.get("cpu_cores") {
            config.cpu_cores = v.parse::<u32>().unwrap_or(config.cpu_cores)
                .clamp(1, 16);
        }
        if let Some(v) = map.get("cpu_mhz") {
            config.cpu_mhz = v.parse::<u32>().unwrap_or(config.cpu_mhz)
                .clamp(100, 4000);
        }
        if let Some(v) = map.get("storage_mb") {
            config.storage_mb = v.parse::<u64>().unwrap_or(config.storage_mb)
                .clamp(256, 131072);
        }
        if let Some(v) = map.get("os_image") {
            config.os_image = PathBuf::from(v);
        }
        if let Some(v) = map.get("storage_dir") {
            config.storage_dir = PathBuf::from(v);
        }
        if let Some(v) = map.get("os_type") {
            if let Some(ot) = OsType::from_str_opt(v) {
                config.os_type = ot;
            }
        }

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SimulatorConfig::default();
        assert_eq!(config.ram_mb, 1024);
        assert_eq!(config.cpu_cores, 1);
        assert_eq!(config.os_type, OsType::Gui);
    }

    #[test]
    fn test_parse_config() {
        let input = r#"
# Comment line
ram_mb = 2048
cpu_cores = 4
storage_mb = 8192
os_image = /path/to/os.mite
storage_dir = /path/to/mite
os_type = terminal
"#;
        let config = SimulatorConfig::parse(input).unwrap();
        assert_eq!(config.ram_mb, 2048);
        assert_eq!(config.cpu_cores, 4);
        assert_eq!(config.storage_mb, 8192);
        assert_eq!(config.os_image, PathBuf::from("/path/to/os.mite"));
        assert_eq!(config.os_type, OsType::Terminal);
    }

    #[test]
    fn test_os_type_gui() {
        let input = "os_type = gui";
        let config = SimulatorConfig::parse(input).unwrap();
        assert_eq!(config.os_type, OsType::Gui);
    }

    #[test]
    fn test_clamping() {
        let input = "ram_mb = 999999";
        let config = SimulatorConfig::parse(input).unwrap();
        assert_eq!(config.ram_mb, 16384);
    }
}
