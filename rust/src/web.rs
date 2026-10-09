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
            let obj = serde_json::json!({
                "ts": st.ts,
                "ab": st.mahm,
                "rtss": st.rtss,
                "ab_ok": st.mahm.ok,
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
            let st = state.lock().unwrap();
            let cfg = st.cfg.clone();
            drop(st);
            let obj = settings_json(&cfg);
            json(&mut stream, 200, &obj)?;
        }
        ("POST", "/api/settings") => {
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
            if let Some(b) = v.get("auto_start_ab").and_then(|x| x.as_bool()) {
                st.cfg.auto_start_ab = b;
            }
            if let Some(b) = v.get("auto_start_rtss").and_then(|x| x.as_bool()) {
                st.cfg.auto_start_rtss = b;
            }
            let cfg = st.cfg.clone();
            drop(st);
            config::save(&cfg);
            let obj = serde_json::json!({ "ok": true });
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

fn settings_json(cfg: &config::Config) -> serde_json::Value {
    let ab = util::find_exe_if_empty(&cfg.ab_path, &config::AB_CANDIDATES);
    let rtss = util::find_exe_if_empty(&cfg.rtss_path, &config::RTSS_CANDIDATES);
    serde_json::json!({
        "config": {
            "port": cfg.port,
            "ab_path": ab,
            "rtss_path": rtss,
            "auto_start_ab": cfg.auto_start_ab,
            "auto_start_rtss": cfg.auto_start_rtss,
        },
        "paths_exist": {
            "ab": std::path::Path::new(&ab).exists(),
            "rtss": std::path::Path::new(&rtss).exists(),
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
