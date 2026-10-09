//! RTSS 共享内存读取（帧率数据）
//!
//! 注意：RTSS 的 `dwAppArrSize` 实测存的是「条目数量」(MAX_APPS = 256)，
//! 而不是 SDK 注释里写的字节数。早期版本按字节数换算会得到 0 个槽位，
//! 导致帧率永远读不到。这里做了自适应兼容。

use crate::shared_mem::{MahmData, SRC_FRAMERATE, SRC_FPS_01LOW, SRC_FPS_1LOW, SRC_FPS_AVG};
use crate::util::Mapping;
use serde::Serialize;

const RTSS_NAME: &str = "RTSSSharedMemoryV2";
const RTSS_SIG: u32 = 0x52545353; // 'RTSS'
const RTSS_SIG_DEAD: u32 = 0x0000DEAD; // 未运行

const OFF_SIG: usize = 0;
const OFF_APP_ENTRY_SIZE: usize = 8;
const OFF_APP_ARR_OFFSET: usize = 12;
const OFF_APP_ARR_SIZE: usize = 16;

const E_PID: usize = 0;
const E_NAME: usize = 4;
const NAME_LEN: usize = 260;
const E_FLAGS: usize = E_NAME + NAME_LEN; // 264
const E_TIME0: usize = E_FLAGS + 4; // 268
const E_TIME1: usize = E_TIME0 + 4; // 272
const E_FRAMES: usize = E_TIME1 + 4; // 276

/// 单个被 RTSS 统计的应用
#[derive(Serialize, Clone, Debug, Default)]
pub struct AppFps {
    pub pid: u32,
    pub app: String,
    pub fps: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frametime: Option<f32>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct RtssData {
    /// RTSS 是否在运行（共享内存签名有效）
    pub running: bool,
    /// 正在渲染的应用，按帧率降序
    pub apps: Vec<AppFps>,

    // ---- 扁平字段：前端帧率卡片直接读这几个 ----
    /// 主帧率（与 Afterburner OSD 上显示的一致）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps: Option<f32>,
    /// 主帧率对应的应用名
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frametime: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps_avg: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps_1pct_low: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps_01pct_low: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RtssData {
    /// 主帧率（供 60 秒历史曲线使用）
    pub fn primary_fps(&self) -> Option<f32> {
        self.fps
    }
}

pub fn read() -> RtssData {
    let mut out = RtssData::default();

    let map = match Mapping::open(RTSS_NAME) {
        Some(m) => m,
        None => {
            out.error = Some("RTSS 未运行".into());
            return out;
        }
    };

    let sig = map.u32_at(OFF_SIG);
    if sig == RTSS_SIG_DEAD || sig != RTSS_SIG {
        out.error = Some(if sig == RTSS_SIG_DEAD {
            "RTSS 未运行".into()
        } else {
            format!("RTSS 签名异常: 0x{:X}", sig)
        });
        return out;
    }

    out.running = true;

    let entry_size = map.u32_at(OFF_APP_ENTRY_SIZE) as usize;
    let arr_off = map.u32_at(OFF_APP_ARR_OFFSET) as usize;
    let arr_size = map.u32_at(OFF_APP_ARR_SIZE) as usize;

    if entry_size == 0 || arr_size == 0 {
        return out;
    }

    // 自适应：小数值按「条目数量」解释，大数值按「字节数」解释
    let count = if arr_size <= 4096 {
        arr_size
    } else {
        arr_size / entry_size
    };

    let mut apps = Vec::new();
    for i in 0..count {
        let base = arr_off + i * entry_size;
        let pid = map.u32_at(base + E_PID);
        if pid == 0 {
            continue;
        }

        let t0 = map.u32_at(base + E_TIME0);
        let t1 = map.u32_at(base + E_TIME1);
        let frames = map.u32_at(base + E_FRAMES);

        // 只保留真正在渲染的条目（时间窗口有效且出了帧）
        if t1 <= t0 || frames == 0 {
            continue;
        }

        // dwTime0/dwTime1 单位 = 毫秒；已验证 (t1-t0)/1000 得秒
        let dt_ms = (t1 - t0) as f32;
        let fps = frames as f32 * 1000.0 / dt_ms;
        if !fps.is_finite() || fps <= 0.1 {
            continue;
        }

        apps.push(AppFps {
            pid,
            app: map.cstr_at(base + E_NAME, NAME_LEN),
            fps: (fps * 10.0).round() / 10.0,
            frametime: Some(((1000.0 / fps) * 100.0).round() / 100.0),
        });
    }

    // 帧率降序，主应用取第一个
    apps.sort_by(|a, b| b.fps.partial_cmp(&a.fps).unwrap_or(std::cmp::Ordering::Equal));
    out.apps = apps;

    out
}

/// 用 Afterburner(MAHM) 的数据补全主帧率与应用名。
///
/// Afterburner 自己判断「当前哪个是游戏」，所以以它的 Framerate 为锚点，
/// 在 RTSS 的应用表里找帧率最接近的那个进程作为应用名。
pub fn finalize(rt: &mut RtssData, mahm: &MahmData) {
    let mget = |sid: u32| -> Option<f32> {
        mahm.items
            .iter()
            .find(|i| i.id == sid)
            .map(|i| i.value)
            .filter(|v| v.is_finite() && *v > 0.0)
    };

    let ab_fps = mget(SRC_FRAMERATE);

    // 主应用
    let best = match ab_fps {
        Some(a) => rt
            .apps
            .iter()
            .filter(|c| c.fps >= 5.0)
            .min_by(|x, y| {
                (x.fps - a)
                    .abs()
                    .partial_cmp(&(y.fps - a).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
        // 没有 Afterburner 锚点时，退化为帧率最高的应用（过滤低频重绘进程）
        None => rt.apps.iter().find(|c| c.fps >= 20.0),
    };

    rt.app = best.map(|a| short_name(&a.app));
    rt.fps = ab_fps.or_else(|| best.map(|a| a.fps));

    rt.frametime = rt
        .fps
        .filter(|f| *f > 0.0)
        .map(|f| ((1000.0 / f) * 100.0).round() / 100.0);

    rt.fps_avg = mget(SRC_FPS_AVG);
    rt.fps_1pct_low = mget(SRC_FPS_1LOW);
    rt.fps_01pct_low = mget(SRC_FPS_01LOW);
}

/// 取路径里的文件名
fn short_name(p: &str) -> String {
    p.rsplit(['\\', '/']).next().unwrap_or(p).to_string()
}
