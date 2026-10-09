//! 共享内存读取：MSI Afterburner (MAHM) + RTSS
//!
//! 两个数据源都是 Windows 命名共享内存 + 裸指针偏移读取。
//! 移植自 Python 版已验证的偏移常量。


// ============ MAHM (MSI Afterburner Hardware Monitor) ============

pub const MAHM_SIG: u32 = 0x4D41484D; // 'MAHM'
pub const MAHM_FLAG_SHOW_IN_OSD: u32 = 0x0000_0001;

/// Afterburner 用 FLT_MAX 表示"数据当前不可用"
pub const FLT_MAX: f32 = 3.4028235e38;

const MAHM_OFF_SIG: usize = 0;
const MAHM_OFF_HDR_SIZE: usize = 8;
const MAHM_OFF_NUM_ENTRIES: usize = 12;
const MAHM_OFF_ENTRY_SIZE: usize = 16;

const M: usize = 260; // MAX_PATH
const MAHM_E_SRCNAME: usize = 0;
const MAHM_E_UNITS: usize = M;
const MAHM_E_DATA: usize = M * 5; // float
const MAHM_E_FLAGS: usize = MAHM_E_DATA + 12;
const MAHM_E_GPU: usize = MAHM_E_FLAGS + 4;
const MAHM_E_SRCID: usize = MAHM_E_GPU + 4;

// ---- 源 ID 常量 ----
pub const SRC_GPU_TEMP: u32 = 0x0000_0000;
pub const SRC_GPU_MEM_TEMP: u32 = 0x0000_0002;
pub const SRC_VRM_TEMP: u32 = 0x0000_0003;
pub const SRC_FAN_SPEED: u32 = 0x0000_0010;
pub const SRC_FAN_RPM: u32 = 0x0000_0011;
pub const SRC_CORE_CLOCK: u32 = 0x0000_0020;
pub const SRC_MEM_CLOCK: u32 = 0x0000_0022;
pub const SRC_GPU_USAGE: u32 = 0x0000_0030;
pub const SRC_MEM_USAGE: u32 = 0x0000_0031;
pub const SRC_FB_USAGE: u32 = 0x0000_0032;
pub const SRC_VID_USAGE: u32 = 0x0000_0033;
pub const SRC_BUS_USAGE: u32 = 0x0000_0034;
pub const SRC_FRAMERATE: u32 = 0x0000_0050;
pub const SRC_FRAMETIME: u32 = 0x0000_0051;
pub const SRC_FPS_MIN: u32 = 0x0000_0052;
pub const SRC_FPS_AVG: u32 = 0x0000_0053;
pub const SRC_FPS_MAX: u32 = 0x0000_0054;
pub const SRC_FPS_1LOW: u32 = 0x0000_0055;
pub const SRC_FPS_01LOW: u32 = 0x0000_0056;
pub const SRC_GPU_POWER_REL: u32 = 0x0000_0060;
pub const SRC_GPU_POWER_ABS: u32 = 0x0000_0061;
pub const SRC_CPU_TEMP: u32 = 0x0000_0080;
pub const SRC_CPU_USAGE: u32 = 0x0000_0090;
pub const SRC_RAM_USAGE: u32 = 0x0000_0091;
pub const SRC_PAGEFILE: u32 = 0x0000_0092;
pub const SRC_CPU_CLOCK: u32 = 0x0000_00A0;
pub const SRC_CPU_POWER: u32 = 0x0000_0100;

#[derive(serde::Serialize, Clone, Debug)]
pub struct SensorItem {
    pub name: String,
    pub source: String,
    pub group: String,
    pub value: f32,
    pub unit: String,
    pub gpu: u32,
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct MahmData {
    pub ok: bool,
    pub all_count: u32,
    pub osd_count: u32,
    pub items: Vec<SensorItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Default for MahmData {
    fn default() -> Self {
        Self {
            ok: false,
            all_count: 0,
            osd_count: 0,
            items: Vec::new(),
            error: None,
        }
    }
}

/// 源 ID -> 分组
pub fn source_group(src: u32) -> &'static str {
    match src {
        SRC_FRAMERATE | SRC_FRAMETIME | SRC_FPS_MIN | SRC_FPS_AVG | SRC_FPS_MAX | SRC_FPS_1LOW
        | SRC_FPS_01LOW => "fps",
        SRC_CPU_TEMP | SRC_CPU_USAGE | SRC_CPU_CLOCK | SRC_CPU_POWER => "cpu",
        SRC_RAM_USAGE | SRC_PAGEFILE => "ram",
        SRC_GPU_TEMP | SRC_GPU_MEM_TEMP | SRC_VRM_TEMP | SRC_FAN_SPEED | SRC_FAN_RPM
        | SRC_CORE_CLOCK | SRC_MEM_CLOCK | SRC_GPU_USAGE | SRC_MEM_USAGE | SRC_FB_USAGE
        | SRC_VID_USAGE | SRC_BUS_USAGE | SRC_GPU_POWER_REL | SRC_GPU_POWER_ABS => "gpu",
        _ => "other",
    }
}

/// 源 ID -> 中文标签
pub fn source_label(src: u32) -> &'static str {
    match src {
        SRC_GPU_TEMP => "温度",
        SRC_GPU_MEM_TEMP => "显存温度",
        SRC_VRM_TEMP => "供电温度",
        SRC_FAN_SPEED => "风扇转速",
        SRC_FAN_RPM => "风扇转速",
        SRC_CORE_CLOCK => "核心频率",
        SRC_MEM_CLOCK => "显存频率",
        SRC_GPU_USAGE => "占用率",
        SRC_MEM_USAGE => "显存占用",
        SRC_FB_USAGE => "显存占用",
        SRC_VID_USAGE => "视频引擎",
        SRC_BUS_USAGE => "总线负载",
        SRC_FRAMERATE => "帧率",
        SRC_FRAMETIME => "帧时间",
        SRC_FPS_MIN => "最低帧",
        SRC_FPS_AVG => "平均帧",
        SRC_FPS_MAX => "最高帧",
        SRC_FPS_1LOW => "1% Low",
        SRC_FPS_01LOW => "0.1% Low",
        SRC_GPU_POWER_REL => "功耗(TDP%)",
        SRC_GPU_POWER_ABS => "功耗",
        SRC_CPU_TEMP => "温度",
        SRC_CPU_USAGE => "占用率",
        SRC_RAM_USAGE => "内存占用",
        SRC_PAGEFILE => "页面文件",
        SRC_CPU_CLOCK => "频率",
        SRC_CPU_POWER => "功耗",
        _ => "",
    }
}

/// 单位归一化：Afterburner 的 °C 常见乱码，统一成 °C
pub fn norm_unit(raw: &str) -> String {
    let t = raw.trim().trim_matches('\0');
    if t.is_empty() {
        return String::new();
    }
    // GBK 误解码后的乱码，或各种摄氏写法
    if t.contains('C') || t.contains('癈') || t == "\u{b0}" {
        return "°C".to_string();
    }
    t.to_string()
}
