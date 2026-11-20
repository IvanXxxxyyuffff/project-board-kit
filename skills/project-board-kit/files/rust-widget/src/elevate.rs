//! 自提升（self-elevation）。
//!
//! 背景：本机 explorer.exe 以 High 完整性（token IL 8192）运行，而本程序从普通
//! 进程启动时是 Medium（4096）。托盘 API 由 shell 一侧校验调用方完整性，
//! 于是 `Shell_NotifyIconW(NIM_ADD)` 返回 FALSE + ERROR_ACCESS_DENIED(5)。
//! 实测：同一 hwnd、同一 NOTIFYICONDATAW，由 High 进程添加即 ok=True。
//!
//! 策略：托盘添加失败时才尝试一次自提升（`ShellExecuteW(runas)`），并带上
//! `--elevated` 标记防止循环。本机 ConsentPromptBehaviorAdmin=0（不提示直接提升），
//! 所以不会弹 UAC；普通 UAC 机器上用户拒绝时只是退回「无托盘」运行。
//!
//! 关闭自提升：设置环境变量 `RW_NO_ELEVATE=1`。

use std::ffi::c_void;

use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::log::Logger;

/// 子进程标记：已经尝试过自提升，不再递归。
pub const FLAG: &str = "--elevated";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 当前进程是不是「自提升后的重试」。
pub fn is_retry() -> bool {
    std::env::args().any(|a| a == FLAG)
}

/// 环境变量里明确关掉自提升（验收脚本/调试用）。
fn disabled() -> bool {
    matches!(std::env::var("RW_NO_ELEVATE").as_deref(), Ok("1"))
}

/// 需要并且可以做一次自提升重试。
/// 默认关闭：`ShellExecuteW("runas")` 在本机对无清单 exe 会弹出「打开文件 - 安全警告」
/// 并阻塞调用线程，因此要显式设置 `RW_AUTO_ELEVATE=1` 才走这条路。
/// 常规部署改用内嵌清单 requireAdministrator（build.rs 编入 app.manifest）。
pub fn should_retry() -> bool {
    !is_retry() && !disabled() && matches!(std::env::var("RW_AUTO_ELEVATE").as_deref(), Ok("1"))
}

/// 以管理员身份重启自己，参数原样带上（外加 `--elevated`）。
/// 返回 true 表示已成功拉起子进程（调用方应尽快退出）。
pub fn relaunch(why: &str, log: &Logger) -> bool {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            log.line(&format!("[elevate] current_exe failed err={} -> skip", e));
            return false;
        }
    };
    let mut parts: Vec<String> = Vec::new();
    for a in std::env::args().skip(1) {
        if a.contains(' ') || a.contains('\t') {
            parts.push(format!("\"{}\"", a));
        } else {
            parts.push(a);
        }
    }
    parts.push(FLAG.to_string());
    let params = parts.join(" ");
    let exe_w = wide(&exe.to_string_lossy());
    let op_w = wide("runas");
    let params_w = wide(&params);
    let h = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op_w.as_ptr(),
            exe_w.as_ptr(),
            params_w.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    let code = h as isize;
    let ok = code > 32;
    log.line(&format!(
        "[elevate] why={} runas ok={} code={} exe={} params={}",
        why, ok, code, exe.display(), params
    ));
    ok
}

// 保留 c_void 引用，避免 windows-sys 版本间 HINSTANCE 类型变化时的编译噪音
#[allow(dead_code)]
fn _assert_ptr(_: *mut c_void) {}
