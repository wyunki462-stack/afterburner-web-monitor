//! 系统托盘图标：隐藏窗口 + 消息循环，保持进程常驻
//!
//! 用 Shell_NotifyIconW 创建托盘图标，右键菜单可「打开网页 / 退出」。
//! 这是进程保活的关键 —— 没有它，main 函数结束后进程就退出了。

use std::sync::atomic::{AtomicUsize, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW, PostQuitMessage, RegisterClassW,
    SetForegroundWindow, TrackPopupMenu, TranslateMessage, CW_USEDEFAULT, HMENU, MF_SEPARATOR,
    MF_STRING, MSG, TPM_BOTTOMALIGN, TPM_LEFTALIGN, WM_APP, WM_COMMAND, WM_DESTROY, WM_LBUTTONUP,
    WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
};

use crate::util;

/// 托盘图标 ID
const TRAY_ID: u32 = 1;
/// 托盘回调消息
const WM_TRAY: u32 = WM_APP + 1;
/// 菜单项 ID
const IDM_OPEN: usize = 1001;
const IDM_EXIT: usize = 1002;

/// 跨消息循环传参：url 指针（在 main 里保持存活）
static URL_PTR: AtomicUsize = AtomicUsize::new(0);

/// 窗口过程：处理托盘图标的鼠标事件和菜单命令
unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TRAY => {
            // 左键单击或右键单击都弹出菜单
            let ev = lparam as u32;
            if ev == WM_LBUTTONUP || ev == WM_RBUTTONUP {
                show_menu(hwnd);
            }
            0
        }
        WM_COMMAND => {
            match wparam & 0xFFFF {
                IDM_OPEN => open_url(),
                IDM_EXIT => {
                    unsafe { DestroyWindow(hwnd) };
                }
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            // 移除托盘图标并退出消息循环
            remove_tray(hwnd);
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// 弹出右键菜单
fn show_menu(hwnd: HWND) {
    unsafe {
        let menu: HMENU = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        let open: Vec<u16> = "打开网页\0".encode_utf16().collect();
        let exit: Vec<u16> = "退出\0".encode_utf16().collect();
        AppendMenuW(menu, MF_STRING, IDM_OPEN, open.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, IDM_EXIT, exit.as_ptr());

        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        // 必须 SetForegroundWindow，否则菜单点外面不会消失
        SetForegroundWindow(hwnd);
        TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_BOTTOMALIGN,
            pt.x,
            pt.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        DestroyMenu(menu);
    }
}

/// 用系统默认浏览器打开网页地址
fn open_url() {
    let ptr = URL_PTR.load(Ordering::Relaxed) as *const u16;
    if ptr.is_null() {
        return;
    }
    let mut len = 0usize;
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let url: Vec<u16> = std::slice::from_raw_parts(ptr, len).to_vec();
        let op: Vec<u16> = "open\0".encode_utf16().collect();
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1, // SW_SHOWNORMAL
        );
    }
}

/// 移除托盘图标
fn remove_tray(hwnd: HWND) {
    unsafe {
        let mut nid = new_nid(hwnd);
        Shell_NotifyIconW(NIM_DELETE, &mut nid);
    }
}

/// 构造 NOTIFYICONDATAW
fn new_nid(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_ID;
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    // 优先用嵌进 exe 的自定义图标（资源 ID = 1，见 assets/app.rc）；
    // 拿不到时回退到系统默认应用图标 IDI_APPLICATION = MAKEINTRESOURCE(32512)
    nid.hIcon = unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let custom = LoadIconW(hinst, 1usize as *const u16);
        if custom.is_null() {
            util::log("托盘图标：未取到内嵌图标，回退到系统默认图标");
            LoadIconW(std::ptr::null_mut(), 32512usize as *const u16)
        } else {
            util::log("托盘图标：已使用内嵌的自定义图标");
            custom
        }
    };
    let tip = "AfterburnerWebMonitor - 硬件监控网页";
    let tip_utf16: Vec<u16> = tip.encode_utf16().take(127).collect();
    nid.szTip[..tip_utf16.len()].copy_from_slice(&tip_utf16);
    nid
}

/// 创建托盘图标并进入消息循环（阻塞，直到用户选择退出）
///
/// `url` 需要在整个消息循环期间保持存活，调用方负责。
pub fn run(url: &str) {
    // 保存 url 指针供窗口过程使用
    URL_PTR.store(url.as_ptr() as usize, Ordering::Relaxed);

    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());

        // 注册窗口类
        let class_name: Vec<u16> = "AfterburnerWebMonitorTray\0".encode_utf16().collect();
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(wndproc);
        wc.hInstance = hinst;
        wc.lpszClassName = class_name.as_ptr();
        RegisterClassW(&wc);

        // 创建隐藏窗口（接收托盘消息）
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            WS_OVERLAPPED,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null_mut(),
        );

        if hwnd.is_null() {
            util::log("创建托盘窗口失败，降级为阻塞等待");
            // 降级：直接永久阻塞，保证 Web 服务不中断
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3600));
            }
        }

        // 添加托盘图标
        let mut nid = new_nid(hwnd);
        if Shell_NotifyIconW(NIM_ADD, &mut nid) == 0 {
            util::log("添加托盘图标失败，降级为阻塞等待");
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3600));
            }
        }
        util::log("托盘图标已就绪");

        // 消息循环：保持进程存活
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        util::log("消息循环结束");
    }
}
