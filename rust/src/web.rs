//! 极简 HTTP 服务（零依赖，手写 HTTP/1.1）
//!
//! 只支持本项目需要的几个路由，避免引入任何 web 框架。

use crate::config;
use crate::util;
use crate::AppState;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

/// 内嵌前端页面
const PAGE: &str = include_str!("page.html");

pub fn serve(port: u16, state: Arc<Mutex<AppState>>) {
    let listener = match TcpListener::bind(("0.0.0.0", port)) {
        Ok(l) => l,
        Err(e) => {
            util::log(&format!("监听端口 {} 失败: {}", port, e));
            return;
        }
    };
    util::log(&format!("Web 服务已启动: http://0.0.0.0:{}", port));

    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let st = Arc::clone(&state);
                std::thread::spawn(move || {
                    let _ = handle(s, st);
                });
            }
            Err(_) => continue,
        }
    }
}

fn handle(mut stream: TcpStream, state: Arc<Mutex<AppState>>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(5)))?;

    let mut reader = BufReader::new(stream.try_clone()?);

    // 读请求行
    let mut req_line = String::new();
    if reader.read_line(&mut req_line)? == 0 {
        return Ok(());
    }
    let mut parts = req_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path_q = parts.next().unwrap_or("/").to_string();
    let path = path_q.split('?').next().unwrap_or("/").to_string();

    // 读头部
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let t = line.trim_end();
        if t.is_empty() {
            break;
        }
        let lower = t.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            content_length = v.trim().parse().unwrap_or(0);
        }
    }

    // 路由
    match (method.as_str(), path.as_str()) {
        ("GET", "/") | ("GET", "/index.html") => {
            respond(&mut stream, 200, "text/html; charset=utf-8", PAGE.as_bytes())?;
        }
        ("GET", "/api/data") => {
            let st = state.lock().unwrap();
            let ab_ok = st.mahm.ok;
            let hw_ok = st.hwinfo.ok;
            let sel = norm_source(&st.cfg.source);
            let active = active_source(sel, ab_ok, hw_ok);
            // 解析出当前生效源对应的那一份数据
            let sensors = match active {
                "hwfull" => &st.hwinfo.full,
                "hwcustom" => &st.hwinfo.custom,
                _ => &st.mahm,
            };
            let obj = serde_json::json!({
                "ts": st.ts,
                "source": sel,
                "active": active,
                "ab_ok": ab_ok,
                "hwinfo_ok": hw_ok,
                "ab": st.mahm,
                "sensors": sensors,
                "rtss": st.rtss,
                "hw_stats": {
                    "all": st.hwinfo.all_count,
                    "auto": st.hwinfo.auto.osd_count,
                    "full": st.hwinfo.full.osd_count,
                    "custom": st.hwinfo.custom.osd_count,
                    "explicit": st.cfg.hwinfo_selected.is_some(),
                    "error": st.hwinfo.error,
                },
            });
            json(&mut stream, 200, &obj)?;
        }
        ("GET", "/api/fps_history") => {
            let st = state.lock().unwrap();
            let now = util::now();
            let pts: Vec<_> = st
                .fps_history
                .iter()
                .map(|(t, f)| {
                    serde_json::json!({
                        "t": ((now - t) * 10.0).round() / 10.0,
                        "fps": f,
                    })
                })
                .collect();
            let obj = serde_json::json!({
                "points": pts,
                "len": crate::FPS_HISTORY_LEN,
            });
            json(&mut stream, 200, &obj)?;
        }
        ("GET", "/api/settings") => {
            let (cfg, hw_auto, hw_all) = {
                let st = state.lock().unwrap();
                (
                    st.cfg.clone(),
                    st.hwinfo.auto.osd_count,
                    st.hwinfo.all_count,
                )
            };
            let obj = settings_json(&cfg, hw_auto, hw_all);
            json(&mut stream, 200, &obj)?;
        }
        ("POST", "/api/settings") | ("POST", "/api/source") => {
            let mut body = vec![0u8; content_length.min(1 << 20)];
            if !body.is_empty() {
                reader.read_exact(&mut body)?;
            }
            let v: serde_json::Value =
                serde_json::from_slice(&body).unwrap_or(serde_json::json!({}));
            let mut st = state.lock().unwrap();
            if let Some(port) = v.get("port").and_then(|x| x.as_u64()) {
                st.cfg.port = port as u16;
            }
            // 数据源：顶层 source 或 config.source（ab / hwfull / hwcustom）
            let src = v
                .get("source")
                .and_then(|x| x.as_str())
                .or_else(|| v.get("config").and_then(|c| c.get("source")).and_then(|x| x.as_str()));
            if let Some(s) = src {
                st.cfg.source = norm_source(s).to_string();
            }
            // 开机自启开关（autostart 嵌套或顶层两种写法都接受）
            let a = v.get("autostart").unwrap_or(&v);
            if let Some(b) = a.get("app").and_then(|x| x.as_bool()) {
                st.cfg.auto_start_app = b;
            }
            if let Some(b) = a.get("ab").and_then(|x| x.as_bool()) {
                st.cfg.auto_start_ab = b;
            }
            if let Some(b) = a.get("rtss").and_then(|x| x.as_bool()) {
                st.cfg.auto_start_rtss = b;
            }
            if let Some(b) = a.get("hwinfo").and_then(|x| x.as_bool()) {
                st.cfg.auto_start_hwinfo = b;
            }
            // 兼容旧的顶层写法
            if let Some(b) = v.get("auto_start_ab").and_then(|x| x.as_bool()) {
                st.cfg.auto_start_ab = b;
            }
            if let Some(b) = v.get("auto_start_rtss").and_then(|x| x.as_bool()) {
                st.cfg.auto_start_rtss = b;
            }
            if let Some(b) = v.get("auto_start_hwinfo").and_then(|x| x.as_bool()) {
                st.cfg.auto_start_hwinfo = b;
            }
            let cfg = st.cfg.clone();
            drop(st);
            config::save(&cfg);
            let obj = serde_json::json!({ "ok": true, "source": cfg.source });
            json(&mut stream, 200, &obj)?;
        }
        ("GET", "/api/hwinfo/list") => {
            // 「自选」界面用：全部读数 + 每项是否属于智能默认集合
            let sel = {
                let st = state.lock().unwrap();
                st.cfg.hwinfo_selected.clone()
            };
            match crate::hwinfo::listing() {
                Some((all_count, items)) => {
                    let obj = serde_json::json!({
                        "ok": true,
                        "all_count": all_count,
                        "selected": sel,
                        "items": items,
                    });
                    json(&mut stream, 200, &obj)?;
                }
                None => {
                    let obj = serde_json::json!({
                        "ok": false,
                        "all_count": 0,
                        "selected": serde_json::Value::Null,
                        "items": [],
                        "msg": "HWiNFO 共享内存不可用",
                    });
                    json(&mut stream, 200, &obj)?;
                }
            }
        }
        ("POST", "/api/hwinfo/selected") => {
            // 保存「自选」勾选结果；{"reset":true} 表示恢复智能默认
            let mut body = vec![0u8; content_length.min(1 << 21)];
            if !body.is_empty() {
                reader.read_exact(&mut body)?;
            }
            let v: serde_json::Value =
                serde_json::from_slice(&body).unwrap_or(serde_json::json!({}));
            let mut st = state.lock().unwrap();
            let mut count = 0usize;
            let mut explicit = false;
            if v.get("reset").and_then(|x| x.as_bool()).unwrap_or(false) {
                st.cfg.hwinfo_selected = None;
                count = st.hwinfo.auto.osd_count as usize;
            } else if let Some(arr) = v.get("keys").and_then(|x| x.as_array()) {
                let keys: Vec<String> = arr
                    .iter()
                    .filter_map(|x| x.as_str())
                    .map(|s| s.to_string())
                    .collect();
                count = keys.len();
                explicit = true;
                st.cfg.hwinfo_selected = Some(keys);
            }
            let cfg = st.cfg.clone();
            drop(st);
            config::save(&cfg);
            let obj = serde_json::json!({ "ok": true, "count": count, "explicit": explicit });
            json(&mut stream, 200, &obj)?;
        }
        ("POST", "/api/hwinfo/setup") => {
            // 一键开启 HWiNFO 的共享内存导出（会重启 HWiNFO，耗时几秒）
            let hw = {
                let st = state.lock().unwrap();
                util::find_exe_if_empty(&st.cfg.hwinfo_path, &config::HWINFO_CANDIDATES)
            };
            let r = crate::hwinfo::setup(&hw, true);
            let obj = serde_json::to_value(&r).unwrap_or(serde_json::json!({"ok": false}));
            json(&mut stream, 200, &obj)?;
        }
        ("POST", p) if p.starts_with("/api/start/") => {
            let what = p.rsplit('/').next().unwrap_or("");
            let ok = start_target(what, &state);
            let obj = serde_json::json!({ "ok": ok });
            json(&mut stream, 200, &obj)?;
        }
        ("GET", "/favicon.ico") => {
            respond(&mut stream, 204, "image/x-icon", b"")?;
        }
        _ => {
            let obj = serde_json::json!({"ok": false, "msg": "not found"});
            json(&mut stream, 404, &obj)?;
        }
    }
    Ok(())
}

/// 归一化数据源取值（兼容旧配置里的 "hwinfo"）
pub fn norm_source(s: &str) -> &'static str {
    match s {
        "hwfull" | "hwinfo_full" | "hwinfofull" | "full" => "hwfull",
        "hwcustom" | "hwinfo_custom" | "hwinfocustom" | "hwinfo" | "custom" => "hwcustom",
        _ => "ab",
    }
}

/// 计算实际生效的数据源。
///
/// 用户选的源可用就用它；不可用而另一个可用时自动顶替（避免页面空白）。
fn active_source(sel: &'static str, ab_ok: bool, hw_ok: bool) -> &'static str {
    if sel == "ab" {
        if ab_ok {
            "ab"
        } else if hw_ok {
            // 小飞机没跑但 HWiNFO 在跑 —— 退回自选（默认即智能筛选）
            "hwcustom"
        } else {
            "ab"
        }
    } else if hw_ok {
        sel
    } else if ab_ok {
        "ab"
    } else {
        sel
    }
}

fn settings_json(cfg: &config::Config, hw_auto: u32, hw_all: u32) -> serde_json::Value {
    let ab = util::find_exe_if_empty(&cfg.ab_path, &config::AB_CANDIDATES);
    let rtss = util::find_exe_if_empty(&cfg.rtss_path, &config::RTSS_CANDIDATES);
    let hw = util::find_exe_if_empty(&cfg.hwinfo_path, &config::HWINFO_CANDIDATES);
    serde_json::json!({
        "config": {
            "port": cfg.port,
            "ab_path": ab,
            "rtss_path": rtss,
            "hwinfo_path": hw,
            "source": norm_source(&cfg.source),
            "hwinfo_selected": cfg.hwinfo_selected,
            "auto_start_ab": cfg.auto_start_ab,
            "auto_start_rtss": cfg.auto_start_rtss,
            "auto_start_hwinfo": cfg.auto_start_hwinfo,
        },
        "source": norm_source(&cfg.source),
        "hwinfo_selected": cfg.hwinfo_selected,
        "hw_stats": { "all": hw_all, "auto": hw_auto },
        "autostart": {
            "app": cfg.auto_start_app,
            "ab": cfg.auto_start_ab,
            "rtss": cfg.auto_start_rtss,
            "hwinfo": cfg.auto_start_hwinfo,
        },
        "hwinfo": {
            "path": hw,
            "exists": std::path::Path::new(&hw).exists(),
            "sm_enabled": crate::hwinfo::sm_enabled(&hw),
            "running": crate::hwinfo::is_running(),
        },
        "paths_exist": {
            "ab": std::path::Path::new(&ab).exists(),
            "rtss": std::path::Path::new(&rtss).exists(),
            "hwinfo": std::path::Path::new(&hw).exists(),
        }
    })
}

fn start_target(what: &str, state: &Arc<Mutex<AppState>>) -> bool {
    let st = state.lock().unwrap();
    let cfg = st.cfg.clone();
    drop(st);
    match what {
        "ab" => {
            let p = util::find_exe_if_empty(&cfg.ab_path, &config::AB_CANDIDATES);
            util::spawn_exe(&p, false)
        }
        "rtss" => {
            let p = util::find_exe_if_empty(&cfg.rtss_path, &config::RTSS_CANDIDATES);
            util::spawn_exe(&p, true)
        }
        "hwinfo" => {
            let p = util::find_exe_if_empty(&cfg.hwinfo_path, &config::HWINFO_CANDIDATES);
            util::spawn_exe(&p, true)
        }
        _ => false,
    }
}

fn json(stream: &mut TcpStream, code: u16, v: &serde_json::Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(v).unwrap_or_else(|_| b"{}".to_vec());
    respond(stream, code, "application/json; charset=utf-8", &body)
}

fn respond(
    stream: &mut TcpStream,
    code: u16,
    ctype: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let reason = match code {
        200 => "OK",
        204 => "No Content",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let head = format!(
        "HTTP/1.1 {} {}\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Connection: close\r\n\r\n",
        code,
        reason,
        ctype,
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    if !body.is_empty() {
        stream.write_all(body)?;
    }
    stream.flush()
}
