// 使用 Windows 子系统，避免弹出控制台黑窗口（release 构建生效）
// debug 构建保留控制台，方便看 println 调试输出
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! AfterburnerWebMonitor v2.3 — Rust 重写版
//!
//! 从 MSI Afterburner 的 OSD 配置项读取硬件监控数据，用网页展示，
//! 手机连同一局域网即可查看。
//!
//! 相比 v1.0（Python）：内存占用从 ~50MB 降到 ~5MB。

mod shared_mem;
mod mahm;
mod hwinfo;
mod rtss;
mod web;
mod config;
mod util;
mod tray;

use std::sync::{Arc, Mutex};
use std::time::Duration;

const APP_NAME: &str = "AfterburnerWebMonitor";
const APP_VERSION: &str = "2.6.0";
const DEFAULT_PORT: u16 = 8777;
const FPS_HISTORY_LEN: usize = 60;

/// 全局共享状态
pub struct AppState {
    pub mahm: shared_mem::MahmData,
    /// HWiNFO 三种视图（智能筛选 / 全显示 / 自选）
    pub hwinfo: hwinfo::Views,
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
    let hw_path = util::find_exe(&cfg.hwinfo_path, &config::HWINFO_CANDIDATES);
    util::log(&format!("Afterburner: {}", ab_path));
    util::log(&format!("RTSS: {}", rtss_path));
    util::log(&format!("HWiNFO: {}", hw_path));

    // 自动开启 HWiNFO 的共享内存导出（HWiNFO 装了才管）
    if !hw_path.is_empty() && std::path::Path::new(&hw_path).exists() {
        if hwinfo::sm_enabled(&hw_path) {
            util::log("HWiNFO 共享内存：已开启");
        } else {
            util::log("HWiNFO 共享内存：未开启，自动写入配置");
            let r = hwinfo::setup(&hw_path, false);
            util::log(&format!("自动配置完成：{}", r.msg));
        }
    }

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
        hwinfo: hwinfo::views(None),
        rtss: rtss::RtssData::default(),
        fps_history: Vec::with_capacity(FPS_HISTORY_LEN),
        ts: 0.0,
        cfg,
    }));

    // 采集线程
    let collector_state = Arc::clone(&state);
    let collector_hw = hw_path.clone();
    std::thread::spawn(move || collector_loop(collector_state, collector_hw));

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
fn collector_loop(state: Arc<Mutex<AppState>>, hw_exe: String) {
    let mut first = true;
    // HWiNFO 共享内存连续读不到的秒数
    let mut hw_miss = 0u32;
    // 是否曾经成功读到过 HWiNFO 数据。
    // 只有「读到过、后来断了」才判定为异常（免费版 12 小时到期），
    // 否则 HWiNFO 冷启动期间会被误判成故障而被反复重启。
    let mut ever_ok = false;
    // 上次自动修复的时间戳（5 分钟内不重复修）
    let mut last_repair = 0.0f64;

    loop {
        // 自选项可能被网页改过，每轮取一次最新配置
        let sel = { state.lock().unwrap().cfg.hwinfo_selected.clone() };

        let mahm = mahm::read();
        let hw = hwinfo::views(sel.as_deref());
        let mut rtss = rtss::read();
        // 用 Afterburner 的 Framerate 作为锚点，确定「哪个进程是游戏」
        rtss::finalize(&mut rtss, &mahm);

        if first {
            util::log(&hwinfo::info());
            first = false;
        }

        // ---- 自愈：曾经读到过 HWiNFO 数据，现在读不到了 ----
        // 免费版连续运行约 12 小时后会停掉共享内存，重启 HWiNFO 即可恢复
        if hw.ok {
            ever_ok = true;
            hw_miss = 0;
        } else {
            hw_miss += 1;
            if hw_miss >= 20 {
                hw_miss = 0;
                let now = util::now();
                if ever_ok
                    && !hw_exe.is_empty()
                    && now - last_repair > 300.0
                    && hwinfo::is_running()
                    && hwinfo::sm_enabled(&hw_exe)
                {
                    util::log("HWiNFO 共享内存中断（免费版到期？），自动重启 HWiNFO");
                    let r = hwinfo::setup(&hw_exe, true);
                    util::log(&format!("自愈结果：{}", r.msg));
                    last_repair = now;
                    ever_ok = false;
                } else if !hwinfo::is_running() {
                    // 进程没在跑，重新计时
                    ever_ok = false;
                }
            }
        }

        let now = util::now();
        {
            let mut st = state.lock().unwrap();
            st.mahm = mahm;
            st.hwinfo = hw;
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
