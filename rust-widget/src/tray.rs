//! 系统托盘：Shell_NotifyIconW + NOTIFYICONDATAW。
//!
//! - 图标：优先 exe 内嵌资源（build.rs → windres → assets/app.rc 的 `1 ICON`），
//!   资源缺失时退回 exe 同目录的「项目看板.ico」（LR_LOADFROMFILE），再退 app.ico。
//! - 回调：uCallbackMessage = WM_APP + 1（main.rs 的 WM_MENU），lParam 低字是鼠标消息。
//! - 用默认的 version 3 语义：lParam 直接是 WM_LBUTTONUP / WM_RBUTTONUP，
//!   这样验收脚本 PostMessage(hwnd, WM_MENU, 0, 0) 也能照常弹出菜单。

use std::ffi::c_void;
use std::path::Path;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{GetLastError, HWND, RECT};
use windows_sys::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconGetRect, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NOTIFYICONDATAW, NOTIFYICONIDENTIFIER,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    LoadImageW, HICON, IMAGE_ICON, LR_DEFAULTCOLOR, LR_LOADFROMFILE,
};

use crate::log::Logger;

/// WM_APP + 1：托盘回调消息（main.rs 里同一常量定义，验收脚本也用它）
pub const TRAY_MSG: u32 = 0x8000 + 1;
pub const TRAY_ID: u32 = 1;
pub const TIP: &str = "项目看板";
/// app.rc 里的图标资源 id
const ICON_RES_ID: usize = 1;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn set_tip(dst: &mut [u16; 128], s: &str) {
    let w: Vec<u16> = s.encode_utf16().take(127).collect();
    dst[..w.len()].copy_from_slice(&w);
    dst[w.len()] = 0;
}

/// 加载托盘图标：内嵌资源 → 同目录 ico。
pub unsafe fn load_icon(exe_dir: Option<&Path>) -> HICON {
    let hinst = GetModuleHandleW(null_mut());
    if !hinst.is_null() {
        let h = LoadImageW(
            hinst,
            ICON_RES_ID as *const u16,
            IMAGE_ICON,
            32,
            32,
            LR_DEFAULTCOLOR,
        );
        if !h.is_null() {
            return h;
        }
    }
    if let Some(dir) = exe_dir {
        for name in ["项目看板.ico", "app.ico"] {
            let p = dir.join(name);
            if p.is_file() {
                let w = wide(&p.to_string_lossy());
                let h = LoadImageW(
                    null_mut(),
                    w.as_ptr(),
                    IMAGE_ICON,
                    32,
                    32,
                    LR_LOADFROMFILE | LR_DEFAULTCOLOR,
                );
                if !h.is_null() {
                    return h;
                }
            }
        }
    }
    null_mut()
}

fn base(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_ID;
    nid.uCallbackMessage = TRAY_MSG;
    nid
}

/// 托盘图标当前坐标（存在才返回）。既用于自检，也方便确认「图标真的在」。
pub unsafe fn rect(hwnd: HWND) -> Option<(i32, i32, i32, i32)> {
    let mut id: NOTIFYICONIDENTIFIER = std::mem::zeroed();
    id.cbSize = std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32;
    id.hWnd = hwnd;
    id.uID = TRAY_ID;
    let mut r: RECT = std::mem::zeroed();
    if Shell_NotifyIconGetRect(&id, &mut r) == 0 {
        Some((r.left, r.top, r.right, r.bottom))
    } else {
        None
    }
}

/// 添加托盘图标。返回 (ok, icon_loaded)。
pub unsafe fn install(hwnd: HWND, icon: HICON, log: &Logger) -> bool {
    // 托盘 API 在部分系统（Win11 26300 实测）要求调用线程已初始化 COM：
    // WinForms/STA 场景天然成立，裸 Win32 进程不初始化时会 NIM_ADD 返回 ERROR_ACCESS_DENIED(5)。
    // 已初始化时 CoInitializeEx 返回 S_FALSE(1)，可安全重复调用。
    let hr = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
    log.line(&format!("[tray] CoInitializeEx hr=0x{:08X}", hr as u32));
    let mut nid = base(hwnd);
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.hIcon = icon;
    set_tip(&mut nid.szTip, TIP);
    // 资源管理器首次可能还没准备好（ERROR_TIMEOUT=1460），重试三次
    let mut ok = false;
    for attempt in 0..3 {
        ok = Shell_NotifyIconW(NIM_ADD, &nid) != 0;
        if ok {
            break;
        }
        let err = GetLastError();
        log.line(&format!(
            "[tray] add attempt={} failed err={} cbSize={} hwnd=0x{:X}",
            attempt + 1,
            err,
            nid.cbSize,
            hwnd as usize
        ));
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    let r = if ok { rect(hwnd) } else { None };
    log.line(&format!(
        "[tray] add ok={} icon={} tip=\"{}\" msg=0x{:X} rect={}",
        ok,
        if icon.is_null() { "none" } else { "loaded" },
        TIP,
        TRAY_MSG,
        match r {
            Some((l, t, rr, b)) => format!("{},{}..{},{}", l, t, rr, b),
            None => "none".to_string(),
        }
    ));
    ok
}

pub unsafe fn remove(hwnd: HWND, log: &Logger) -> bool {
    let nid = base(hwnd);
    let ok = Shell_NotifyIconW(NIM_DELETE, &nid) != 0;
    log.line(&format!("[tray] delete ok={}", ok));
    ok
}

/// 释放 HICON（DestroyIcon 在 WindowsAndMessaging 里，用函数指针避免额外 import 冲突）
pub unsafe fn destroy_icon(icon: HICON) {
    if icon.is_null() {
        return;
    }
    #[link(name = "user32")]
    extern "system" {
        fn DestroyIcon(h: *mut c_void) -> i32;
    }
    let _ = DestroyIcon(icon);
}
