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

pub const HWINFO_CANDIDATES: [&str; 5] = [
    r"D:\ComputerTools\HWiNFO\HWiNFO64.exe",
    r"D:\ComputerTools\HWiNFO64\HWiNFO64.exe",
    r"C:\Program Files\HWiNFO64\HWiNFO64.exe",
    r"C:\Program Files (x86)\HWiNFO64\HWiNFO64.exe",
    r"D:\Program Files\HWiNFO64\HWiNFO64.exe",
];

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    #[serde(default = "d_port")]
    pub port: u16,
    #[serde(default)]
    pub ab_path: String,
    #[serde(default)]
    pub rtss_path: String,
    #[serde(default)]
    pub hwinfo_path: String,
    /// 数据源选择：
    ///   "ab"        小飞机（Afterburner OSD 勾选项）
    ///   "hwfull"    HWiNFO 全显示（共享内存里所有读数）
    ///   "hwcustom"  HWiNFO 自选（用户在页面上勾选的项）
    #[serde(default = "d_source")]
    pub source: String,
    /// HWiNFO 自选项（RawRow.key 列表）
    ///   None    = 未自定义，跟随「智能筛选」的默认集合
    ///   Some(v) = 用户明确勾选的项（可以为空 = 一项都不显示）
    #[serde(default)]
    pub hwinfo_selected: Option<Vec<String>>,
    #[serde(default = "d_true")]
    pub auto_start_ab: bool,
    #[serde(default = "d_true")]
    pub auto_start_rtss: bool,
    #[serde(default)]
    pub auto_start_hwinfo: bool,
    #[serde(default = "d_true")]
    pub auto_start_app: bool,
}

fn d_port() -> u16 {
    8777
}
fn d_true() -> bool {
    true
}
fn d_source() -> String {
    "ab".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 8777,
            ab_path: String::new(),
            rtss_path: String::new(),
            hwinfo_path: String::new(),
            source: "ab".to_string(),
            hwinfo_selected: None,
            auto_start_ab: true,
            auto_start_rtss: true,
            auto_start_hwinfo: false,
            auto_start_app: true,
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
