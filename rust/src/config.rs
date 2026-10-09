//! 配置读写（config.json）

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const AB_CANDIDATES: [&str; 4] = [
    r"D:\ComputerTools\msiafterburner\MSI Afterburner\MSIAfterburner.exe",
    r"C:\Program Files (x86)\MSI Afterburner\MSIAfterburner.exe",
    r"C:\Program Files\MSI Afterburner\MSIAfterburner.exe",
    r"D:\Program Files (x86)\MSI Afterburner\MSIAfterburner.exe",
];

pub const RTSS_CANDIDATES: [&str; 4] = [
    r"D:\ComputerTools\RivaTuner Statistics Server\RTSS.exe",
    r"C:\Program Files (x86)\RivaTuner Statistics Server\RTSS.exe",
    r"C:\Program Files\RivaTuner Statistics Server\RTSS.exe",
    r"D:\Program Files (x86)\RivaTuner Statistics Server\RTSS.exe",
];

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    #[serde(default = "d_port")]
    pub port: u16,
    #[serde(default)]
    pub ab_path: String,
    #[serde(default)]
    pub rtss_path: String,
    #[serde(default = "d_true")]
    pub auto_start_ab: bool,
    #[serde(default = "d_true")]
    pub auto_start_rtss: bool,
}

fn d_port() -> u16 {
    8777
}
fn d_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 8777,
            ab_path: String::new(),
            rtss_path: String::new(),
            auto_start_ab: true,
            auto_start_rtss: true,
        }
    }
}

fn config_path() -> PathBuf {
    crate::util::exe_dir().join("config.json")
}

pub fn load() -> Config {
    let p = config_path();
    if let Ok(text) = std::fs::read_to_string(&p) {
        if let Ok(cfg) = serde_json::from_str::<Config>(&text) {
            return cfg;
        }
    }
    Config::default()
}

pub fn save(cfg: &Config) {
    let p = config_path();
    if let Ok(text) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(p, text);
    }
}
