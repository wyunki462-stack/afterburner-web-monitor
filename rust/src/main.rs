//! AfterburnerWebMonitor v2.0 — Rust 重写版
//!
//! 从 MSI Afterburner 的 OSD 配置项读取硬件监控数据，用网页展示，
//! 手机连同一局域网即可查看。
//!
//! 相比 v1.0（Python）：内存占用从 ~50MB 降到 ~5MB。

mod shared_mem;
mod mahm;
mod rtss;
mod web;
mod config;
mod util;
mod tray;

use std::sync::{Arc, Mutex};
use std::time::Duration;

const APP_NAME: &str = "AfterburnerWebMonitor";
const APP_VERSION: &str = "2.2.0";
const DEFAULT_PORT: u16 = 8777;
const FPS_HISTORY_LEN: usize = 60;

/// 全局共享状态
pub struct AppState {
    pub mahm: shared_mem::MahmData,
    pub rtss: rtss::RtssData,
    pub fps_history: Vec<(f64, f32)>, // (时间戳, fps)
    pub ts: f64,
    pub cfg: config::Config,
}

fn main() {
    let cfg = config::load();

    // 日志文件
    util::init_log();

    util::log(&format!("=== {} v{} 启动 ===", APP_NAME, APP_VERSION));

    // 探测路径
    let ab_path = util::find_exe(&cfg.ab_path, &config::AB_CANDIDATES);
    let rtss_path = util::find_exe(&cfg.rtss_path, &config::RTSS_CANDIDATES);
    util::log(&format!("Afterburner: {}", ab_path));
    util::log(&format!("RTSS: {}", rtss_path));

    let port = cfg.port;

    // 端口占用检查
    if util::port_open(port) {
        util::log(&format!("端口 {} 已被占用，退出", port));
        util::msgbox(
            &format!("端口 {} 已被占用。\n\n可能服务已在运行。", port),
            APP_NAME,
        );
        return;
    }

    // 共享状态
    let state = Arc::new(Mutex::new(AppState {
        mahm: shared_mem::MahmData::default(),
        rtss: rtss::RtssData::default(),
        fps_history: Vec::with_capacity(FPS_HISTORY_LEN),
        ts: 0.0,
        cfg,
    }));

    // 采集线程
    let collector_state = Arc::clone(&state);
    std::thread::spawn(move || collector_loop(collector_state));

    // Web 服务线程
    let web_state = Arc::clone(&state);
    std::thread::spawn(move || web::serve(port, web_state));

    std::thread::sleep(Duration::from_millis(800));

    // 局域网地址
    let ip = util::lan_ip();
    let url = format!("http://{}:{}", ip, port);
    util::log(&format!("手机访问地址: {}", url));

    // 弹窗提示（--no-dialog / --silent 可跳过，适合开机自启）
    let silent = std::env::args().any(|a| a == "--no-dialog" || a == "--silent");
    if !silent {
        let tip = format!(
            "{} 已启动\n\n手机浏览器访问：\n{}\n\n\
             数据来自 Afterburner 的 OSD 勾选项，\n\
             在 Afterburner 里改勾选，网页自动跟随。\n\n\
             点击「确定」后程序在系统托盘运行，\n\
             右键托盘图标可退出。",
            APP_NAME, url
        );
        util::log("弹窗提示");
        util::msgbox(&tip, APP_NAME);
    } else {
        util::log("静默模式，跳过弹窗");
    }

    // 进入托盘消息循环，保持进程常驻（关键：否则 main 结束进程就退出了）
    util::log("进入托盘消息循环");
    tray::run(&url);

    util::log("=== 程序退出 ===");
}

/// 采集循环：每秒读一次传感器与帧率
fn collector_loop(state: Arc<Mutex<AppState>>) {
    loop {
        let mahm = mahm::read();
        let rtss = rtss::read();

        let now = util::now();
        {
            let mut st = state.lock().unwrap();
            st.mahm = mahm;
            st.rtss = rtss.clone();
            st.ts = now;

            // 帧率历史
            if let Some(fps) = rtss.primary_fps() {
                st.fps_history.push((now, fps));
                while st.fps_history.len() > FPS_HISTORY_LEN {
                    st.fps_history.remove(0);
                }
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}
