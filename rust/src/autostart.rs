//! Windows 开机启动项管理
//!
//! 位置：`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
//!
//! 四个开关各自对应一条**独立**的启动项（值名见下方常量）：
//! 打开时写入、关闭时删除。配置（`config.json`）是「意愿」，
//! 本模块负责把意愿落到注册表。
//!
//! ⚠️ RTSS 与 HWiNFO64 的 manifest 都要求管理员权限，
//! 从启动项拉起时 Windows 会弹 UAC —— 这是它们自身的设计，绕不过去。

use crate::config;
use crate::util;
use windows_sys::Win32::System::Registry::*;

const RUN_SUB: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

/// 启动项的注册表值名，同时也是 `Item::id` 之外的人类可读标识
pub const NAME_APP: &str = "AfterburnerWebMonitor";
pub const NAME_AB: &str = "AfterburnerWebMonitor_AB";
pub const NAME_RTSS: &str = "AfterburnerWebMonitor_RTSS";
pub const NAME_HWINFO: &str = "AfterburnerWebMonitor_HWiNFO";

/// 旧版本（LHMViewer 时期，v1.0 Python 版）留下的启动项名。
///
/// 实测这台机器上就有 `LHMViewer_AB` / `LHMViewer_RTSS` 两条 ——
/// Python 版真的写过启动项，Rust 重写时把这段实现弄丢了（所以开关一直是摆设）。
/// v2.6.1 起用新名字接管，并把旧条目清掉，否则开机时 Afterburner/RTSS 会被拉起两次。
const LEGACY: [(&str, &str); 2] = [
    ("LHMViewer_AB", NAME_AB),
    ("LHMViewer_RTSS", NAME_RTSS),
];

/// 本程序自己开机启动时用的参数（跳过那个提示弹窗）
const APP_ARGS: &str = "--no-dialog";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0u16)).collect()
}

/// 读取启动项的命令行；不存在返回 None
pub fn get(name: &str) -> Option<String> {
    let sub = wide(RUN_SUB);
    let nm = wide(name);
    let mut hkey: HKEY = std::ptr::null_mut();
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, sub.as_ptr(), 0, KEY_QUERY_VALUE, &mut hkey) != 0 {
            return None;
        }
        let mut ty: u32 = 0;
        let mut cb: u32 = 0;
        // 第一遍：只问长度
        let rc = RegQueryValueExW(
            hkey,
            nm.as_ptr(),
            std::ptr::null_mut(),
            &mut ty,
            std::ptr::null_mut(),
            &mut cb,
        );
        if rc != 0 || cb == 0 {
            RegCloseKey(hkey);
            return None;
        }
        let mut buf = vec![0u8; cb as usize];
        let rc2 = RegQueryValueExW(
            hkey,
            nm.as_ptr(),
            std::ptr::null_mut(),
            &mut ty,
            buf.as_mut_ptr(),
            &mut cb,
        );
        RegCloseKey(hkey);
        if rc2 != 0 || (ty != REG_SZ && ty != REG_EXPAND_SZ) {
            return None;
        }
        let u16s: Vec<u16> = buf[..cb as usize]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let s = String::from_utf16_lossy(&u16s)
            .trim_end_matches('\0')
            .to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }
}

/// 写入（或覆盖）启动项
pub fn set(name: &str, cmd: &str) -> bool {
    let sub = wide(RUN_SUB);
    let nm = wide(name);
    let data: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0u16)).collect();
    let bytes = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2) };
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
        let rc2 = RegSetValueExW(
            hkey,
            nm.as_ptr(),
            0,
            REG_SZ,
            bytes.as_ptr(),
            bytes.len() as u32,
        );
        RegCloseKey(hkey);
        rc2 == 0
    }
}

/// 删除启动项；本来就没有也算成功
pub fn remove(name: &str) -> bool {
    const ERROR_FILE_NOT_FOUND: u32 = 2;
    let sub = wide(RUN_SUB);
    let nm = wide(name);
    let mut hkey: HKEY = std::ptr::null_mut();
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, sub.as_ptr(), 0, KEY_SET_VALUE, &mut hkey) != 0 {
            return true; // Run 键都不存在 → 肯定没有启动项
        }
        let rc = RegDeleteValueW(hkey, nm.as_ptr());
        RegCloseKey(hkey);
        rc == 0 || rc == ERROR_FILE_NOT_FOUND
    }
}

/// 把 exe 路径拼成命令行；含空格时加引号
pub fn cmdline(exe: &str, args: Option<&str>) -> String {
    let mut s = if exe.contains(' ') {
        format!("\"{}\"", exe)
    } else {
        exe.to_string()
    };
    if let Some(a) = args {
        if !a.is_empty() {
            s.push(' ');
            s.push_str(a);
        }
    }
    s
}

/// 一条启动项的状态
#[derive(Clone, Debug)]
pub struct Item {
    /// "app" / "ab" / "rtss" / "hwinfo"
    pub id: &'static str,
    /// 注册表值名
    pub name: &'static str,
    /// 显示名
    pub label: &'static str,
    /// 程序路径（空 = 没探测到）
    pub exe: String,
    /// 程序是否真的存在
    pub exists: bool,
    /// 配置里是否勾选（意愿）
    pub want: bool,
    /// 注册表里当前是否已有这条启动项（实际）
    pub in_run: bool,
}

fn mk(id: &'static str, name: &'static str, label: &'static str, exe: &str, want: bool) -> Item {
    let exists = !exe.is_empty() && std::path::Path::new(exe).exists();
    Item {
        id,
        name,
        label,
        exe: exe.to_string(),
        exists,
        want,
        in_run: get(name).is_some(),
    }
}

/// 按当前配置算出四条启动项的**实际**状态（只读，不碰注册表）
pub fn items(cfg: &config::Config) -> Vec<Item> {
    let app_exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let ab = util::find_exe_if_empty(&cfg.ab_path, &config::AB_CANDIDATES);
    let rtss = util::find_exe_if_empty(&cfg.rtss_path, &config::RTSS_CANDIDATES);
    let hw = util::find_exe_if_empty(&cfg.hwinfo_path, &config::HWINFO_CANDIDATES);

    vec![
        mk("app", NAME_APP, "AfterburnerWebMonitor", &app_exe, cfg.auto_start_app),
        mk("ab", NAME_AB, "MSI Afterburner", &ab, cfg.auto_start_ab),
        mk("rtss", NAME_RTSS, "RivaTuner Statistics Server", &rtss, cfg.auto_start_rtss),
        mk("hwinfo", NAME_HWINFO, "HWiNFO64", &hw, cfg.auto_start_hwinfo),
    ]
}

/// 清理旧版本留下的启动项（值名已迁移到 `AfterburnerWebMonitor_*`）
fn cleanup_legacy() {
    for (old, new) in LEGACY {
        if let Some(val) = get(old) {
            remove(old);
            util::log(&format!(
                "开机自启：清理旧条目 [{}] = {}（已由 [{}] 接管）",
                old, val, new
            ));
        }
    }
}

/// 按配置把四条启动项同步到注册表，返回同步后的实际状态。
///
/// - 勾选 且 程序存在 → 写入启动项
/// - 勾选 但 程序没找到 → **保持原样**（不清掉用户可能手动配好的项）
/// - 未勾选 → 删除启动项
pub fn sync(cfg: &config::Config) -> Vec<Item> {
    for it in items(cfg) {
        if it.want {
            if !it.exists {
                continue;
            }
            let args = if it.id == "app" { Some(APP_ARGS) } else { None };
            let ok = set(it.name, &cmdline(&it.exe, args));
            util::log(&format!(
                "开机自启 [{}] {} -> {}",
                it.label,
                if ok { "已写入" } else { "写入失败" },
                cmdline(&it.exe, args)
            ));
        } else {
            let had = it.in_run;
            remove(it.name);
            if had {
                util::log(&format!("开机自启 [{}] 已移除", it.label));
            }
        }
    }
    cleanup_legacy();
    items(cfg)
}
