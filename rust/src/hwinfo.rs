//! HWiNFO 共享内存读取
//!
//! HWiNFO 在「设置 → Shared Memory Support」勾选并重启后，会创建命名共享内存
//! `Global\HWiNFO_SENS_SM2`。结构体定义见 HWiNFO 公开的 SDK 头文件：
//!   - `HWiNFO_SENSORS_SHARED_MEM2`  头部
//!   - `HWiNFO_SENSORS_SENSOR_ELEMENT` 传感器表项
//!   - `HWiNFO_SENSORS_READING_ELEMENT` 读数表项
//!
//! 两个坑（与 RTSS 那次同类）：
//!   1. 元素**大小**会被填充到较大值（实测 8.54：sensor 392B、reading 460B），
//!      但字段**偏移**仍沿用经典布局。绝不能按 struct 大小反推偏移 ——
//!      460 会误导成"8 字节对齐所以 Value@288"，实际 Value@284，读出来全是垃圾。
//!      这里只从头部取元素大小（当 stride），字段偏移写死经典值。
//!   2. 免费版 HWiNFO 的共享内存支持会在运行约 12 小时后停止更新，
//!      重启 HWiNFO 即可恢复（Pro 版无此限制）。
//!
//! 输出的结构与 Afterburner 的一致（复用 `MahmData`），方便前端统一渲染。

use crate::shared_mem::{norm_unit, MahmData, SensorItem};
use crate::util::Mapping;
use std::collections::HashMap;

/// 共享内存名（先试 Global\ 命名空间）
const NAMES: [&str; 3] = [
    "Global\\HWiNFO_SENS_SM2",
    "HWiNFO_SENS_SM2",
    "Global\\HWiNFO_SENS_SM", // 更老的版本
];

/// 'HWiS'
const SIG: u32 = 0x53695748;

// ---- 读数类型（SENSOR_READING_TYPE） ----
const RT_TEMP: u32 = 1;
const RT_VOLT: u32 = 2;
const RT_FAN: u32 = 3;
const RT_CURRENT: u32 = 4;
const RT_POWER: u32 = 5;
const RT_CLOCK: u32 = 6;
const RT_USAGE: u32 = 7;

/// 头部（已按实际对齐方式归一化后的偏移）
#[derive(Debug, Clone, Copy)]
struct Hdr {
    ver: u32,
    rev: u32,
    poll: i64,
    s_off: usize,
    s_sz: usize,
    s_n: usize,
    r_off: usize,
    r_sz: usize,
    r_n: usize,
    /// 读数项内 Value 字段的偏移
    v_off: usize,
    /// 传感器名在表项内的偏移（user 名 / orig 名）
    name_user_off: usize,
    name_orig_off: usize,
    /// 传感器名的 UTF-8 版本（8.5x 新增，旧版没有）
    name_utf8_off: Option<usize>,
    /// 读数项内各字段偏移
    r_type: usize,
    r_sensor_idx: usize,
    r_label_user: usize,
    r_label_orig: usize,
    /// 读数标签的 UTF-8 版本（8.5x 新增，旧版没有）
    r_label_utf8: Option<usize>,
    r_unit: usize,
    r_unit_utf8: Option<usize>,
}

fn err(msg: &str) -> MahmData {
    MahmData {
        ok: false,
        all_count: 0,
        osd_count: 0,
        items: Vec::new(),
        error: Some(msg.to_string()),
    }
}

/// 尝试两种候选布局，返回数值合理的那一个
fn parse_header(map: &Mapping) -> Option<Hdr> {
    let sig = map.u32_at(0);
    if sig != SIG {
        return None;
    }

    // 候选：pack=1（poll_time 紧跟 revision）与默认对齐（poll_time 前有 4B padding）
    for base in [0usize, 4usize] {
        let s_off = map.u32_at(20 + base) as usize;
        let s_sz = map.u32_at(24 + base) as usize;
        let s_n = map.u32_at(28 + base) as usize;
        let r_off = map.u32_at(32 + base) as usize;
        let r_sz = map.u32_at(36 + base) as usize;
        let r_n = map.u32_at(40 + base) as usize;

        let sane = (256..=8192).contains(&s_sz)
            && (300..=8192).contains(&r_sz)
            && s_n > 0
            && s_n <= 1024
            && r_n > 0
            && r_n <= 200_000
            && s_off >= 40
            && r_off > s_off;

        if !sane {
            continue;
        }

        // ⚠ 实测 (HWiNFO 8.54)：元素**大小**会被填充，但字段**偏移**始终是经典值。
        //   reading 460B = 经典 316B + szLabelUTF8[128] + szUnitUTF8[16]
        //   sensor  392B = 经典 264B + szNameUTF8[128]
        //   探到 460 就按"r_sz % 8 == 0 → 288"去猜，会错 4 字节，读出来全是垃圾。
        const R_CLASSIC: usize = 316;
        const S_CLASSIC: usize = 264;
        let v_off = 284; // 12 + 128(labelOrig) + 128(labelUser) + 16(unit)

        // 8.5x 的 UTF-8 扩展字段（存在才读，用于干净地显示中文）
        let r_label_utf8 = if r_sz >= R_CLASSIC + 128 {
            Some(R_CLASSIC)
        } else {
            None
        };
        let r_unit_utf8 = if r_sz >= R_CLASSIC + 144 {
            Some(R_CLASSIC + 128)
        } else {
            None
        };
        let name_utf8_off = if s_sz >= S_CLASSIC + 128 {
            Some(S_CLASSIC)
        } else {
            None
        };

        return Some(Hdr {
            ver: map.u32_at(4),
            rev: map.u32_at(8),
            poll: {
                let lo = map.u32_at(12 + base) as u64;
                let hi = map.u32_at(16 + base) as u64;
                ((hi << 32) | lo) as i64
            },
            s_off,
            s_sz,
            s_n,
            r_off,
            r_sz,
            r_n,
            v_off,
            name_user_off: 136,
            name_orig_off: 8,
            name_utf8_off,
            r_type: 0,
            r_sensor_idx: 4,
            r_label_user: 140,
            r_label_orig: 12,
            r_label_utf8,
            r_unit: 268,
            r_unit_utf8,
        });
    }
    None
}

/// 读传感器名表：索引 -> (显示名, 原始英文名)
///
/// 显示名优先用 8.5x 的 UTF-8 名（中文最干净），再退回本地编码的"用户自定义名"，
/// 最后用原始英文名。**原始英文名单独保留**：过滤/分组规则全部基于它，
/// 这样界面切成中文也不会让规则失效。
fn read_sensors(map: &Mapping, h: &Hdr) -> HashMap<usize, (String, String)> {
    let mut out = HashMap::new();
    for i in 0..h.s_n {
        let b = h.s_off + i * h.s_sz;
        let utf8 = h
            .name_utf8_off
            .map(|o| map.cstr_at_smart(b + o, 128))
            .unwrap_or_default();
        let user = map.cstr_at_smart(b + h.name_user_off, 128);
        let orig = map.cstr_at_smart(b + h.name_orig_off, 128);
        let disp = if !utf8.trim().is_empty() {
            utf8
        } else if !user.trim().is_empty() {
            user
        } else {
            orig.clone()
        };
        out.insert(i, (disp, orig));
    }
    out
}

/// 逐核心项（"Core 0 VID"、"CPU Core #3" 之类）——数量太多，默认折叠掉
fn is_per_core(label: &str) -> bool {
    let s = label.trim().to_ascii_lowercase();
    let s = s.strip_prefix("cpu ").unwrap_or(&s);
    let s = s.strip_prefix("core").unwrap_or(s);
    let s = s.trim_start_matches(['#', ' ', ':', '-']);
    s.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
}

/// 噪声项：限值、统计量、错误/链路计数器、视频编解码与计算引擎
///
/// ⚠ 只用于**英文原始标签**。HWiNFO 的中文译名把 "Performance Limit" 译成
/// 「性能受限」、"Thermal Limit" 译成「过热限制」，按中文匹配全都会漏。
fn is_noise(l: &str) -> bool {
    const BAD: [&str; 27] = [
        "limit", "average", "maximum", "minimum", "residency", "distance", "elapsed",
        "total time", "bus width", "bus load", "latency", "throttl", "reason", "error",
        "warn", "replay", "nak", "dllp", "tlp", "lcrc", "measured", "ratio",
        "video decode", "video encode", "video engine", "computing (", "vr usage",
    ];
    BAD.iter().any(|b| l.contains(b))
}

/// 是否值得展示（**全部基于英文原始标签**）
fn keep(sensor_orig: &str, label_orig: &str, rtype: u32, unit: &str) -> bool {
    // 传感器级排除：HWiNFO 自带的帧率提供者（本页帧率另有卡片）、网卡
    let s = sensor_orig.to_ascii_lowercase();
    if s.starts_with("rtss") || s.starts_with("presentmon") || s.contains("network") {
        return false;
    }
    // 布尔标志（Yes/No）与内存时序（单位 T）没有监控价值
    if unit.eq_ignore_ascii_case("Yes/No") || unit == "T" {
        return false;
    }

    let l = label_orig.to_ascii_lowercase();
    if is_noise(&l) {
        return false;
    }
    let per_core = is_per_core(&l);

    match rtype {
        // 逐核心的温度/占用/频率太碎，折叠
        RT_TEMP => !per_core,
        RT_USAGE => !per_core,
        RT_FAN => true,
        // 逐核心功耗不要，只留封装/核心总功率
        RT_POWER => !per_core && !l.contains("input power") && !l.contains("output power"),
        // 频率太多（含各子单元），只保留真正关心的几项
        RT_CLOCK => {
            !per_core
                && (l.contains("gpu clock")
                    || l.contains("memory clock")
                    || l.contains("effective clock")
                    || l.contains("bus clock")
                    || l.contains("infinity fabric")
                    || l.contains("memory controller clock"))
        }
        // 电流项动辄几十条（每个供电相位一条），全不要
        RT_CURRENT => false,
        // 电压只留少数关键轨
        RT_VOLT => {
            !per_core
                && (l.contains("core voltage")
                    || l.contains("soc voltage")
                    || l.contains("battery voltage"))
        }
        // 其他类型里挑真正有用的：内存/显存用量、带宽速率、磁盘寿命、电池、活动核心数
        _ => {
            l.contains("memory")
                || l.contains("page file")
                || l.contains("bandwidth")
                || l.contains("rate")
                || l.contains("life")
                || l.contains("spare")
                || l.contains("capacity")
                || l.contains("charge level")
                || l.contains("wear level")
                || l.contains("active core")
                || l.contains("link speed")
        }
    }
}

/// 归一化到前端已有的分组
fn group_of(sensor_orig: &str, label_orig: &str, _rtype: u32) -> &'static str {
    let s = sensor_orig.to_ascii_lowercase();
    let l = label_orig.to_ascii_lowercase();
    // ⚠ 顺序要紧：GPU 传感器要**先**判，否则 GPU 的 "Memory ..." 项
    //   （显存结温 / 显存使用 / D3D 专用显存）会被下面的 memory 规则抢到「内存」组
    if s.contains("gpu") || s.contains("nvidia") || s.contains("geforce") || s.contains("radeon") {
        return "gpu";
    }
    // 「System」传感器下的内存量归到「内存」，别丢进「主板」
    if l.contains("memory") || l.contains("page file") {
        return "ram";
    }
    if s.contains("cpu") || s.contains("ryzen") || s.contains("core i")
        || s.contains("processor")
    {
        "cpu"
    } else if s.contains("memory") || s.contains("dimm") || s.contains("ram") {
        "ram"
    } else if s.contains("drive") || s.contains("ssd") || s.contains("nvme") || s.contains("hdd") {
        "storage"
    } else if s.contains("motherboard") || s.contains("mainboard") || s.contains("system")
        || s.contains("chipset")
    {
        "board"
    } else {
        "other"
    }
}

/// 分组内排序优先级：温度 > 功耗 > 占用 > 频率 > 风扇 > 其他
/// （HWiNFO 原始项太多，超额时要先砍掉低价值项）
fn rank_of_id(id: u32) -> u8 {
    match id {
        0x0000 => 0, // 温度
        0x0060 => 1, // 功耗
        0x0030 => 2, // 占用率
        0x0020 => 3, // 频率
        0x0010 => 4, // 风扇
        _ => 5,
    }
}

/// 传感器短名：用于区分同名项（iGPU / dGPU、两条内存条）
fn short_sensor(s: &str) -> String {
    let head = s.split(':').next().unwrap_or(s).trim();
    if let Some(i) = head.find(" [#") {
        let base = head[..i].trim();
        let num = head[i + 2..]
            .trim_start_matches('#')
            .split(']')
            .next()
            .unwrap_or("")
            .trim();
        // "DDR5 DIMM [#0]" -> "DIMM #0"，"dGPU [#0]" -> "dGPU #0"
        let base = base.split_whitespace().last().unwrap_or(base);
        if num.is_empty() {
            return base.to_string();
        }
        return format!("{} #{}", base, num);
    }
    head.to_string()
}

/// 伪源 ID：沿用 Afterburner 的编码，让前端既有的按 id 判断逻辑仍然生效
fn pseudo_id(rtype: u32) -> u32 {
    match rtype {
        RT_TEMP => 0x0000,     // 温度
        RT_FAN => 0x0010,      // 风扇
        RT_CLOCK => 0x0020,    // 频率
        RT_USAGE => 0x0030,    // 占用率
        RT_POWER => 0x0060,    // 功耗
        _ => 0x00E0,
    }
}

/// 单个分组最多展示多少项（**只在智能筛选模式生效**；全显示 / 自选都不截断）
const PER_GROUP: usize = 24;
/// 扫描上限（读数表一般 400 上下，留足余量）
const MAX_SCAN: usize = 4096;

/// 一条 HWiNFO 读数（中间形态，三种视图共用一次解析结果）
struct RawRow {
    /// 稳定标识：英文原始标签 + 原始传感器名（选择项持久化用这个）
    key: String,
    rtype: u32,
    /// 显示标签（8.5x 的 UTF-8 名优先）
    disp: String,
    /// 显示用传感器名
    sensor_disp: String,
    unit: String,
    value: f64,
    group: &'static str,
    /// 是否通过智能筛选（keep）
    auto_pass: bool,
}

/// 一次共享内存解析的结果
struct RawSet {
    ok: bool,
    all_count: u32,
    error: Option<String>,
    rows: Vec<RawRow>,
}

/// 三种视图（智能筛选 / 全显示 / 自选）
pub struct Views {
    pub ok: bool,
    pub all_count: u32,
    pub error: Option<String>,
    pub auto: MahmData,
    pub full: MahmData,
    pub custom: MahmData,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// 智能筛选：自动挑出常见且有用的项
    Auto,
    /// 全显示：共享内存里所有读数
    Full,
    /// 自选：用户在页面上勾选的项
    Custom,
}

/// 读完共享内存里的**全部**读数。
///
/// 只丢掉「标签为空」和「数值异常」的项，不做任何取舍 ——
/// 取舍交给上层三种视图（智能筛选 / 全显示 / 自选）各自决定。
fn read_raw() -> RawSet {
    let mut opened = None;
    for n in NAMES {
        if let Some(m) = Mapping::open(n) {
            if m.u32_at(0) == SIG {
                opened = Some(m);
                break;
            }
        }
    }

    let map = match opened {
        Some(m) => m,
        None => {
            return RawSet {
                ok: false,
                all_count: 0,
                error: Some("HWiNFO 未运行，或未开启「Shared Memory Support」".to_string()),
                rows: Vec::new(),
            };
        }
    };

    let h = match parse_header(&map) {
        Some(h) => h,
        None => {
            return RawSet {
                ok: false,
                all_count: 0,
                error: Some("HWiNFO 共享内存头部异常（布局不识别）".to_string()),
                rows: Vec::new(),
            };
        }
    };

    let sensors = read_sensors(&map, &h);

    let mut rows: Vec<RawRow> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for i in 0..h.r_n {
        if rows.len() >= MAX_SCAN {
            break;
        }
        let b = h.r_off + i * h.r_sz;
        let rtype = map.u32_at(b + h.r_type);

        let sensor_idx = map.u32_at(b + h.r_sensor_idx) as usize;
        let (sensor_disp, sensor_orig) = sensors.get(&sensor_idx).cloned().unwrap_or_default();

        // 英文原始标签：过滤 / 分组 / 主键都用它（界面语言变了也不受影响）
        let orig = map.cstr_at_smart(b + h.r_label_orig, 128);
        // 显示标签：优先 8.5x 的 UTF-8 名，其次本地编码的用户名
        let label_user = map.cstr_at_smart(b + h.r_label_user, 128);
        let label_utf8 = h
            .r_label_utf8
            .map(|o| map.cstr_at_smart(b + o, 128))
            .unwrap_or_default();
        let disp = if !label_utf8.trim().is_empty() {
            label_utf8
        } else if !label_user.trim().is_empty() {
            label_user
        } else {
            orig.clone()
        };
        if disp.trim().is_empty() {
            continue;
        }

        // 单位：UTF-8 版优先
        let unit_raw = match h.r_unit_utf8 {
            Some(o) => {
                let u = map.cstr_at_smart(b + o, 16);
                if u.trim().is_empty() {
                    map.cstr_at_smart(b + h.r_unit, 16)
                } else {
                    u
                }
            }
            None => map.cstr_at_smart(b + h.r_unit, 16),
        };
        let unit = norm_unit(&unit_raw);

        let value = map.f64_at(b + h.v_off);
        if !value.is_finite() || value.abs() > 1.0e12 {
            continue;
        }

        // 去重：同一传感器下同名读数只留第一条（HWiNFO 有重复项）
        let key = format!("{}\u{1}{}", orig, sensor_orig);
        if !seen.insert(key.clone()) {
            continue;
        }

        let auto_pass = keep(&sensor_orig, &orig, rtype, &unit);
        rows.push(RawRow {
            key,
            rtype,
            disp,
            sensor_disp,
            unit,
            value,
            group: group_of(&sensor_orig, &orig, rtype),
            auto_pass,
        });
    }

    RawSet {
        ok: true,
        all_count: h.r_n as u32,
        error: None,
        rows,
    }
}

/// 消歧后的展示项
struct Deco {
    key: String,
    id: u32,
    label: String,
    source: String,
    group: &'static str,
    value: f64,
    unit: String,
}

/// 给一组读数加前缀消歧：
///   · 风扇项名字常常就叫 "CPU"/"GPU" → 加「风扇 ·」
///   · 同名同单位出现在多个传感器（dGPU / iGPU、两条内存条）→ 加传感器短名
///   · GPU 组里有多块显卡时整组加前缀（否则「GPU 使用率 0%」和
///     「GPU 核心使用率 39%」并存，分不清是哪块卡）
fn decorate(rows: &[&RawRow]) -> Vec<Deco> {
    let mut dup: HashMap<(String, String), usize> = HashMap::new();
    for r in rows {
        *dup.entry((r.disp.clone(), r.unit.clone())).or_insert(0) += 1;
    }

    let mut out: Vec<Deco> = rows
        .iter()
        .map(|r| {
            let mut label = r.disp.clone();
            if r.rtype == RT_FAN {
                let low = label.to_ascii_lowercase();
                if !low.contains("fan") && !label.contains("风扇") {
                    label = format!("风扇 · {}", label);
                }
            }
            let needs_tag = dup
                .get(&(r.disp.clone(), r.unit.clone()))
                .copied()
                .unwrap_or(0)
                > 1;
            if needs_tag {
                let tag = short_sensor(&r.sensor_disp);
                if !tag.is_empty() && !label.starts_with(&tag) {
                    label = format!("{} · {}", tag, label);
                }
            }
            Deco {
                key: r.key.clone(),
                id: pseudo_id(r.rtype),
                label,
                source: r.sensor_disp.clone(),
                group: r.group,
                value: r.value,
                unit: r.unit.clone(),
            }
        })
        .collect();

    let gpu_tags: std::collections::HashSet<String> = out
        .iter()
        .filter(|i| i.group == "gpu")
        .map(|i| short_sensor(&i.source))
        .collect();
    if gpu_tags.len() > 1 {
        for it in out.iter_mut().filter(|i| i.group == "gpu") {
            let tag = short_sensor(&it.source);
            if !tag.is_empty() && !it.label.starts_with(&tag) {
                it.label = format!("{} · {}", tag, it.label);
            }
        }
    }

    out
}

/// 排序（分组 → 类型优先级 → 名称）+ 可选的分组截断
fn arrange(mut items: Vec<Deco>, cap_per_group: bool) -> Vec<Deco> {
    let order = |g: &str| match g {
        "gpu" => 0,
        "cpu" => 1,
        "ram" => 2,
        "storage" => 3,
        "board" => 4,
        _ => 5,
    };
    items.sort_by(|a, b| {
        order(a.group)
            .cmp(&order(b.group))
            .then_with(|| rank_of_id(a.id).cmp(&rank_of_id(b.id)))
            .then_with(|| a.label.cmp(&b.label))
    });

    if cap_per_group {
        let mut per: HashMap<&'static str, usize> = HashMap::new();
        items.retain(|it| {
            let c = per.entry(it.group).or_insert(0);
            if *c >= PER_GROUP {
                false
            } else {
                *c += 1;
                true
            }
        });
    }
    items
}

fn to_mahm(items: &[Deco], all_count: u32) -> MahmData {
    let out: Vec<SensorItem> = items
        .iter()
        .map(|d| SensorItem {
            id: d.id,
            name: d.label.clone(),
            label: d.label.clone(),
            source: d.source.clone(),
            group: d.group.to_string(),
            value: ((d.value as f32) * 100.0).round() / 100.0,
            unit: d.unit.clone(),
            gpu: 0,
        })
        .collect();
    let n = out.len() as u32;

    MahmData {
        ok: true,
        all_count,
        osd_count: n,
        items: out,
        error: None,
    }
}

/// 「智能筛选」最终留下的项（含分组截断），用作自选模式的默认勾选集。
///
/// ⚠ 必须和 `Mode::Auto` 走同一条排序+截断路径，否则「自选默认」和
///   「智能默认」会数不上号（页面提示 75 项、弹窗却勾了 85 项）。
fn auto_key_set(raw: &RawSet) -> std::collections::HashSet<String> {
    let rows: Vec<&RawRow> = raw.rows.iter().filter(|r| r.auto_pass).collect();
    arrange(decorate(&rows), true)
        .into_iter()
        .map(|d| d.key)
        .collect()
}

/// 按指定模式，从一次解析结果里挑出要展示的项
fn build(set: &RawSet, mode: Mode, selected: &std::collections::HashSet<String>) -> MahmData {
    if !set.ok {
        return err(set.error.as_deref().unwrap_or("HWiNFO 共享内存不可用"));
    }
    let rows: Vec<&RawRow> = match mode {
        Mode::Auto => set.rows.iter().filter(|r| r.auto_pass).collect(),
        Mode::Full => set.rows.iter().collect(),
        Mode::Custom => set
            .rows
            .iter()
            .filter(|r| selected.contains(&r.key))
            .collect(),
    };
    to_mahm(&arrange(decorate(&rows), mode == Mode::Auto), set.all_count)
}

/// 一次解析，产出三种视图（采集线程每秒调一次）
///
/// `sel`：用户在配置里存的自选列表；None 表示未自定义，跟随智能筛选。
pub fn views(sel: Option<&[String]>) -> Views {
    let raw = read_raw();
    if !raw.ok {
        let e = raw
            .error
            .clone()
            .unwrap_or_else(|| "HWiNFO 共享内存不可用".to_string());
        return Views {
            ok: false,
            all_count: 0,
            error: Some(e.clone()),
            auto: err(&e),
            full: err(&e),
            custom: err(&e),
        };
    }

    let empty: std::collections::HashSet<String> = std::collections::HashSet::new();
    let auto = build(&raw, Mode::Auto, &empty);
    let full = build(&raw, Mode::Full, &empty);
    let auto_keys = auto_key_set(&raw);
    let keys: std::collections::HashSet<String> = match sel {
        Some(v) => v.iter().cloned().collect(),
        None => auto_keys.clone(),
    };
    let custom = build(&raw, Mode::Custom, &keys);

    Views {
        ok: true,
        all_count: raw.all_count,
        error: None,
        auto,
        full,
        custom,
    }
}

/// 共享内存当前是否可读
pub fn is_ok() -> bool {
    read_raw().ok
}

/// 供「自选」界面使用的完整读数清单
#[derive(Serialize, Clone, Debug)]
pub struct ListItem {
    pub key: String,
    pub label: String,
    pub group: String,
    pub source: String,
    pub unit: String,
    pub value: f32,
    /// 是否属于智能筛选的默认集合
    pub auto: bool,
}

/// 返回 (读数总数, 全部读数项)。共享内存不可用时返回 None。
pub fn listing() -> Option<(u32, Vec<ListItem>)> {
    let raw = read_raw();
    if !raw.ok {
        return None;
    }
    let auto_keys = auto_key_set(&raw);
    let rows: Vec<&RawRow> = raw.rows.iter().collect();
    let items = decorate(&rows)
        .into_iter()
        .map(|d| ListItem {
            auto: auto_keys.contains(&d.key),
            key: d.key,
            label: d.label,
            group: d.group.to_string(),
            source: d.source,
            unit: d.unit,
            value: ((d.value as f32) * 100.0).round() / 100.0,
        })
        .collect();
    Some((raw.all_count, items))
}

/// 共享内存版本信息（写日志用）
pub fn info() -> String {
    for n in NAMES {
        if let Some(m) = Mapping::open(n) {
            if m.u32_at(0) == SIG {
                if let Some(h) = parse_header(&m) {
                    return format!(
                        "HWiNFO 共享内存 v{}.{} · 传感器 {} 个(项 {}B) · 读数 {} 项(项 {}B, Value@{}{})",
                        h.ver,
                        h.rev,
                        h.s_n,
                        h.s_sz,
                        h.r_n,
                        h.r_sz,
                        h.v_off,
                        if h.r_label_utf8.is_some() { ", 含UTF8" } else { "" }
                    );
                }
            }
        }
    }
    "HWiNFO 共享内存未找到".to_string()
}

// ==========================================================================
//  配置管理：自动开启 Shared Memory Support
// ==========================================================================
//
//  HWiNFO 的开关存在两处，便携版读 INI、安装版读注册表，所以两处都写：
//    · HWiNFO64.INI  [Settings] 段  SensorsSM=1
//    · HKCU\Software\HWiNFO64       SensorsSM (DWORD) = 1
//
//  ⚠ 必须在 HWiNFO 关闭时写：它退出时会把内存里的设置回写到配置文件，
//    运行中修改会被覆盖。

use serde::Serialize;

/// 一键开启的结果，供前端展示
#[derive(Serialize, Clone, Debug, Default)]
pub struct SetupResult {
    pub ok: bool,
    /// 找到了便携版 INI
    pub ini_found: bool,
    pub ini_written: bool,
    pub reg_written: bool,
    /// 操作前 HWiNFO 正在运行
    pub was_running: bool,
    /// 已自动重启
    pub restarted: bool,
    /// 重启后共享内存已就绪
    pub sm_live: bool,
    pub msg: String,
}

/// exe 同目录下的 INI 路径（HWiNFO64.exe -> HWiNFO64.INI）
fn ini_path(exe: &str) -> Option<std::path::PathBuf> {
    let p = std::path::Path::new(exe);
    let dir = p.parent()?;
    let stem = p.file_stem()?.to_string_lossy().to_string();
    Some(dir.join(format!("{}.INI", stem)))
}

fn bracket_name(l: &str) -> Option<&str> {
    let t = l.trim();
    if t.len() >= 2 && t.starts_with('[') && t.ends_with(']') {
        Some(t[1..t.len() - 1].trim())
    } else {
        None
    }
}

fn key_of(l: &str) -> Option<&str> {
    let t = l.trim();
    if t.is_empty() || t.starts_with(';') || t.starts_with('#') {
        return None;
    }
    let (k, _) = t.split_once('=')?;
    let k = k.trim();
    if k.is_empty() {
        None
    } else {
        Some(k)
    }
}

/// 在 INI 的 [Settings] 段写入 SensorsSM=1
fn patch_ini(path: &std::path::Path) -> bool {
    let raw = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let text = String::from_utf8_lossy(&raw).to_string();
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();

    // 定位 [Settings] 段范围
    let mut sec: Option<(usize, usize)> = None;
    for (i, l) in lines.iter().enumerate() {
        if let Some(n) = bracket_name(l) {
            if n.eq_ignore_ascii_case("settings") {
                sec = Some((i, lines.len()));
            } else if let Some((s, _)) = sec {
                if i > s {
                    sec = Some((s, i));
                    break;
                }
            }
        }
    }

    match sec {
        Some((s, e)) => {
            let mut done = false;
            for i in (s + 1)..e {
                if key_of(&lines[i])
                    .map(|k| k.eq_ignore_ascii_case("SensorsSM"))
                    .unwrap_or(false)
                {
                    lines[i] = "SensorsSM=1".to_string();
                    done = true;
                    break;
                }
            }
            if !done {
                lines.insert(e, "SensorsSM=1".to_string());
            }
        }
        None => {
            if lines.last().map(|l| !l.trim().is_empty()).unwrap_or(false) {
                lines.push(String::new());
            }
            lines.push("[Settings]".to_string());
            lines.push("SensorsSM=1".to_string());
        }
    }

    let out = lines.join(eol) + eol;
    std::fs::write(path, out).is_ok()
}

/// 读注册表里的 SensorsSM
fn reg_read() -> Option<u32> {
    use windows_sys::Win32::System::Registry::*;
    let sub: Vec<u16> = "Software\\HWiNFO64\0".encode_utf16().collect();
    let name: Vec<u16> = "SensorsSM\0".encode_utf16().collect();
    let mut hkey: HKEY = std::ptr::null_mut();
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, sub.as_ptr(), 0, KEY_QUERY_VALUE, &mut hkey) != 0 {
            return None;
        }
        let mut ty: u32 = 0;
        let mut data: u32 = 0;
        let mut cb: u32 = std::mem::size_of::<u32>() as u32;
        let rc = RegQueryValueExW(
            hkey,
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut ty,
            &mut data as *mut u32 as *mut u8,
            &mut cb,
        );
        RegCloseKey(hkey);
        if rc == 0 && ty == REG_DWORD {
            Some(data)
        } else {
            None
        }
    }
}

/// 写注册表 SensorsSM=1
fn reg_write() -> bool {
    use windows_sys::Win32::System::Registry::*;
    let sub: Vec<u16> = "Software\\HWiNFO64\0".encode_utf16().collect();
    let name: Vec<u16> = "SensorsSM\0".encode_utf16().collect();
    let mut hkey: HKEY = std::ptr::null_mut();
    let mut disp: u32 = 0;
    unsafe {
        let rc = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            0,
            std::ptr::null_mut(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | KEY_QUERY_VALUE,
            std::ptr::null(),
            &mut hkey,
            &mut disp,
        );
        if rc != 0 {
            return false;
        }
        let val: u32 = 1;
        let bytes = std::slice::from_raw_parts(&val as *const u32 as *const u8, 4);
        let rc2 = RegSetValueExW(hkey, name.as_ptr(), 0, REG_DWORD, bytes.as_ptr(), 4);
        RegCloseKey(hkey);
        rc2 == 0
    }
}

/// 共享内存导出是否已在配置里开启（INI 或注册表任一为 1）
pub fn sm_enabled(exe: &str) -> bool {
    if let Some(p) = ini_path(exe) {
        if let Ok(raw) = std::fs::read(&p) {
            let text = String::from_utf8_lossy(&raw);
            if text
                .lines()
                .any(|l| l.trim().eq_ignore_ascii_case("SensorsSM=1"))
            {
                return true;
            }
        }
    }
    reg_read() == Some(1)
}

/// 枚举进程，找 HWiNFO 的 PID（32/64/ARM64 都认）
fn find_pid() -> Option<u32> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;

    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        let mut found = None;
        if Process32FirstW(snap, &mut e) != 0 {
            loop {
                let len = e
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(e.szExeFile.len());
                let name = String::from_utf16_lossy(&e.szExeFile[..len]).to_ascii_lowercase();
                if name == "hwinfo64.exe" || name == "hwinfo32.exe" || name == "hwinfo_arm64.exe" {
                    found = Some(e.th32ProcessID);
                    break;
                }
                if Process32NextW(snap, &mut e) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
        found
    }
}

pub fn is_running() -> bool {
    find_pid().is_some()
}

/// 结束 HWiNFO 进程
pub fn kill() -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    let pid = match find_pid() {
        Some(p) => p,
        None => return true, // 本来就没在跑
    };
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if h.is_null() {
            return false;
        }
        let rc = TerminateProcess(h, 0);
        CloseHandle(h);
        rc != 0
    }
}

/// 一键开启共享内存：
/// 关掉 HWiNFO → 写 INI + 注册表 → 重新启动 → 等待共享内存出现
///
/// `also_start`：HWiNFO 本来没在跑时，是否也顺便把它拉起来
/// （HWiNFO 要求管理员权限，拉起时会弹一次 UAC）
pub fn setup(exe: &str, also_start: bool) -> SetupResult {
    let mut r = SetupResult::default();

    if exe.is_empty() || !std::path::Path::new(exe).exists() {
        r.msg = "没找到 HWiNFO64.exe，请确认安装位置".to_string();
        return r;
    }

    let ini = ini_path(exe);
    r.ini_found = ini.as_ref().map(|p| p.exists()).unwrap_or(false);

    // ---- 1. 先关掉 HWiNFO（运行中写配置会被它退出时覆盖）----
    r.was_running = is_running();
    if r.was_running {
        kill();
        // 等进程真正退出，否则它退出时会把旧配置写回去
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if !is_running() {
                break;
            }
        }
    }

    // ---- 2. 写配置 ----
    if let Some(p) = &ini {
        if p.exists() {
            r.ini_written = patch_ini(p);
        }
    }
    r.reg_written = reg_write();

    // ---- 3. 启动 HWiNFO ----
    // 它的 manifest 要求管理员权限，必须用 runas 提权启动，
    // 否则直接报 WinError 740（请求的操作需要提升）
    if r.was_running || also_start {
        if crate::util::spawn_exe(exe, true) {
            r.restarted = true;
        }
    }

    // ---- 4. 等共享内存出现（UAC 确认 + 传感器初始化，给足 30 秒）----
    if r.restarted {
        for _ in 0..120 {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if is_ok() {
                r.sm_live = true;
                break;
            }
        }
    }

    r.ok = r.ini_written || r.reg_written;
    r.msg = if !r.ok {
        "配置写入失败（试试用管理员身份运行本程序）".to_string()
    } else if r.sm_live {
        "共享内存已开启并生效".to_string()
    } else if r.restarted {
        "配置已写入，HWiNFO 正在启动（若弹出 UAC 请点「是」）".to_string()
    } else if is_running() {
        "配置已写入，重启 HWiNFO 后生效".to_string()
    } else {
        "配置已写入，下次启动 HWiNFO 自动生效".to_string()
    };
    r
}

