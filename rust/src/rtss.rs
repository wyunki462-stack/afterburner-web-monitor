//! RTSS 共享内存读取（帧率数据）

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
const E_FRAMETIME: usize = E_FRAMES + 4; // 280
const E_STATFLAGS: usize = E_FRAMETIME + 4; // 284
const E_STATCOUNT: usize = E_STATFLAGS + 16; // 300
const E_STATFPS_MIN: usize = E_STATCOUNT + 4; // 304
const E_STATFPS_AVG: usize = E_STATFPS_MIN + 4; // 308
const E_STATFPS_MAX: usize = E_STATFPS_AVG + 4; // 312

/// 实测验证的 1% low 偏移（不要手算）
const E_FPS_1PCT_LOW: usize = 9172;
const E_FPS_01PCT_LOW: usize = 9176;

#[derive(Serialize, Clone, Debug, Default)]
pub struct AppFps {
    pub pid: u32,
    pub app: String,
    pub fps: f32,
    pub frametime: Option<f32>,
    pub fps_min: Option<f32>,
    pub fps_avg: Option<f32>,
    pub fps_max: Option<f32>,
    pub low_1pct: Option<f32>,
    pub low_01pct: Option<f32>,
}

#[derive(Serialize, Clone, Debug)]
pub struct RtssData {
    pub running: bool,
    pub apps: Vec<AppFps>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Default for RtssData {
    fn default() -> Self {
        Self {
            running: false,
            apps: Vec::new(),
            error: None,
        }
    }
}

impl RtssData {
    /// 主帧率（取第一个有帧率数据的应用）
    pub fn primary_fps(&self) -> Option<f32> {
        self.apps
            .iter()
            .find(|a| a.fps > 0.0)
            .map(|a| a.fps)
    }
}

pub fn read() -> RtssData {
    let map = match Mapping::open(RTSS_NAME) {
        Some(m) => m,
        None => {
            return RtssData {
                running: false,
                apps: Vec::new(),
                error: Some("RTSS 未运行".into()),
            }
        }
    };

    let sig = map.u32_at(OFF_SIG);
    if sig == RTSS_SIG_DEAD {
        return RtssData {
            running: false,
            apps: Vec::new(),
            error: Some("RTSS 未运行".into()),
        };
    }
    if sig != RTSS_SIG {
        return RtssData {
            running: false,
            apps: Vec::new(),
            error: Some(format!("RTSS 签名异常: 0x{:X}", sig)),
        };
    }

    let entry_size = map.u32_at(OFF_APP_ENTRY_SIZE) as usize;
    let arr_off = map.u32_at(OFF_APP_ARR_OFFSET) as usize;
    let arr_size = map.u32_at(OFF_APP_ARR_SIZE) as usize;

    if entry_size == 0 || arr_size == 0 {
        return RtssData {
            running: true,
            apps: Vec::new(),
            error: None,
        };
    }

    let count = arr_size / entry_size;
    let mut apps = Vec::new();

    for i in 0..count {
        let base = arr_off + i * entry_size;
        let pid = map.u32_at(base + E_PID);
        if pid == 0 {
            continue;
        }

        let name = map.cstr_at(base + E_NAME, NAME_LEN);
        let t0 = map.u32_at(base + E_TIME0);
        let t1 = map.u32_at(base + E_TIME1);
        let frames = map.u32_at(base + E_FRAMES);
        let frametime = map.u32_at(base + E_FRAMETIME);

        let dt = if t1 > t0 {
            (t1 - t0) as f32 / 1000.0
        } else {
            0.0
        };
        let fps = if dt > 0.0 {
            frames as f32 / dt
        } else {
            0.0
        };

        // 统计数据（仅当有统计标志时才有意义）
        let statflags = map.u32_at(base + E_STATFLAGS);
        let has_stat = statflags != 0;

        let (fps_min, fps_avg, fps_max) = if has_stat {
            let cnt = map.u32_at(base + E_STATCOUNT) as f32;
            let mn = map.f32_at(base + E_STATFPS_MIN);
            let mx = map.f32_at(base + E_STATFPS_MAX);
            let sum = map.f32_at(base + E_STATFPS_AVG);
            let avg = if cnt > 0.0 { sum / cnt } else { 0.0 };
            (
                if mn > 0.0 { Some(mn) } else { None },
                if avg > 0.0 { Some(avg) } else { None },
                if mx > 0.0 { Some(mx) } else { None },
            )
        } else {
            (None, None, None)
        };

        let low1 = map.f32_at(base + E_FPS_1PCT_LOW);
        let low01 = map.f32_at(base + E_FPS_01PCT_LOW);

        let clean = |v: f32| -> Option<f32> {
            if v.is_finite() && v > 0.0 && v < 1e6 {
                Some((v * 10.0).round() / 10.0)
            } else {
                None
            }
        };

        apps.push(AppFps {
            pid,
            app: name,
            fps: (fps * 10.0).round() / 10.0,
            frametime: if frametime > 0 {
                Some((frametime as f32 / 1000.0 * 100.0).round() / 100.0)
            } else {
                None
            },
            fps_min,
            fps_avg,
            fps_max,
            low_1pct: low1_clean(low1, clean),
            low_01pct: low1_clean(low01, clean),
        });
    }

    RtssData {
        running: true,
        apps,
        error: None,
    }
}

fn low1_clean(v: f32, f: impl Fn(f32) -> Option<f32>) -> Option<f32> {
    f(v)
}

/// RTSS 是否在运行
pub fn alive() -> bool {
    read().running
}
