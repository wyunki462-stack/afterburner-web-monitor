//! 通用工具：共享内存映射、日志、网络、窗口提示

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ,
    PAGE_READONLY,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

static LOG_FILE: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

pub fn init_log() {
    let dir = exe_dir();
    let p = dir.join("AfterburnerWebMonitor.log");
    *LOG_FILE.lock().unwrap() = Some(p);
}

pub fn log(msg: &str) {
    let ts = local_time_string();
    let line = format!("{} {}\n", ts, msg);
    if let Some(p) = LOG_FILE.lock().unwrap().as_ref() {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

fn local_time_string() -> String {
    // 简单格式：YYYY-MM-DD HH:MM:SS
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs() as i64;
    // 东八区
    let secs = secs + 8 * 3600;
    let days = secs / 86400;
    let rem = secs % 86400;
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // 从 1970-01-01 起算
    let (y, mo, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, mo, d, h, mi, s)
}

/// Howard Hinnant 的 civil_from_days 算法
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub fn exe_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|x| x.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

// ============ 共享内存映射 ============

pub struct Mapping {
    _handle: HANDLE,
    pub view: *const u8,
    raw_view: windows_sys::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS,
}

// 共享内存视图在进程内跨线程使用是安全的（只读）
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

impl Mapping {
    /// 打开已存在的命名共享内存（只读）
    pub fn open(name: &str) -> Option<Self> {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let handle = OpenFileMappingW(FILE_MAP_READ, FALSE, wide.as_ptr());
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                return None;
            }
            let view = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0);
            if view.Value.is_null() {
                CloseHandle(handle);
                return None;
            }
            Some(Mapping {
                _handle: handle,
                view: view.Value as *const u8,
                raw_view: view,
            })
        }
    }

    /// 创建命名共享内存（用于创建者场景，本程序一般不用）
    #[allow(dead_code)]
    pub fn create(name: &str, size: usize) -> Option<Self> {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let handle = CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                std::ptr::null(),
                PAGE_READONLY,
                0,
                size as u32,
                wide.as_ptr(),
            );
            if handle.is_null() {
                return None;
            }
            let view = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0);
            if view.Value.is_null() {
                CloseHandle(handle);
                return None;
            }
            Some(Mapping {
                _handle: handle,
                view: view.Value as *const u8,
                raw_view: view,
            })
        }
    }

    /// 读 u32（小端）
    #[inline]
    pub fn u32_at(&self, off: usize) -> u32 {
        unsafe { std::ptr::read_unaligned(self.view.add(off) as *const u32) }
    }

    /// 读 f32（小端）
    #[inline]
    pub fn f32_at(&self, off: usize) -> f32 {
        unsafe { std::ptr::read_unaligned(self.view.add(off) as *const f32) }
    }

    /// 读固定长度 C 字符串（按 GBK/本地编码解码）
    pub fn cstr_at(&self, off: usize, max: usize) -> String {
        unsafe {
            let slice = std::slice::from_raw_parts(self.view.add(off), max);
            let end = slice.iter().position(|&b| b == 0).unwrap_or(max);
            decode_ansi(&slice[..end])
        }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        if !self.view.is_null() {
            unsafe {
                UnmapViewOfFile(self.raw_view);
                CloseHandle(self._handle);
            }
        }
    }
}

/// 把 Windows ANSI(GBK) 字节转成 String
fn decode_ansi(bytes: &[u8]) -> String {
    // 纯 ASCII 快速路径
    if bytes.iter().all(|&b| b < 0x80) {
        return String::from_utf8_lossy(bytes).to_string();
    }
    // 使用 Windows MultiByteToWideChar (CP_ACP = 0)
    use windows_sys::Win32::Globalization::MultiByteToWideChar;
    unsafe {
        let n = MultiByteToWideChar(0, 0, bytes.as_ptr(), bytes.len() as i32, std::ptr::null_mut(), 0);
        if n <= 0 {
            return String::from_utf8_lossy(bytes).to_string();
        }
        let mut buf = vec![0u16; n as usize];
        let n2 = MultiByteToWideChar(0, 0, bytes.as_ptr(), bytes.len() as i32, buf.as_mut_ptr(), n);
        if n2 <= 0 {
            return String::from_utf8_lossy(bytes).to_string();
        }
        buf.truncate(n2 as usize);
        String::from_utf16_lossy(&buf)
    }
}

// ============ 网络 ============

/// 检查端口是否被占用（能连上说明被占用）
pub fn port_open(port: u16) -> bool {
    use std::net::{TcpStream, SocketAddr};
    let addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
    std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(400)).is_ok()
        && {
            let _ = TcpStream::connect(addr);
            true
        }
}

/// 获取局域网 IP（通过 UDP 连接到公网地址，不实际发包）
pub fn lan_ip() -> String {
    use std::net::UdpSocket;
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = sock.local_addr() {
                return addr.ip().to_string();
            }
        }
    }
    "127.0.0.1".to_string()
}

// ============ 路径探测 ============

pub fn find_exe(configured: &str, candidates: &[&str]) -> String {
    if !configured.is_empty() && std::path::Path::new(configured).exists() {
        return configured.to_string();
    }
    for c in candidates {
        if std::path::Path::new(c).exists() {
            return c.to_string();
        }
    }
    configured.to_string()
}

/// 同上，但语义更明确（配置为空时从候选里找）
pub fn find_exe_if_empty(configured: &str, candidates: &[&str]) -> String {
    find_exe(configured, candidates)
}

/// 启动外部程序；elevate=true 时用 UAC 提权（ShellExecuteW runas）
pub fn spawn_exe(path: &str, elevate: bool) -> bool {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    if path.is_empty() || !std::path::Path::new(path).exists() {
        return false;
    }

    let file: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let verb: Vec<u16> = if elevate {
        "runas".encode_utf16().chain(std::iter::once(0)).collect()
    } else {
        "open".encode_utf16().chain(std::iter::once(0)).collect()
    };

    unsafe {
        let r = ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
        // 返回值 > 32 表示成功
        (r as isize) > 32
    }
}

// ============ 弹窗 ============

pub fn msgbox(text: &str, title: &str) {
    let t: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let c: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            t.as_ptr(),
            c.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}
