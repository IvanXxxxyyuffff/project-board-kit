//! 开机自启：`%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\项目看板.lnk`
//!
//! 只用一个快捷方式（不用注册表 Run 键），删除即关闭自启。
//! .lnk 用系统自带的 WScript.Shell 生成（隐藏窗口、CREATE_NO_WINDOW，不闪黑框），
//! 避免为了 IShellLinkW 的 COM 虚表额外引依赖；返回码 + 文件存在双判定。

use std::path::PathBuf;
use std::process::Command;

use crate::log::Logger;

pub const LNK_NAME: &str = "项目看板.lnk";

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Startup 文件夹（一般 %APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup）
pub fn startup_dir() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(|u| PathBuf::from(u).join("AppData").join("Roaming"))
        })?;
    Some(
        appdata
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Startup"),
    )
}

pub fn lnk_path() -> Option<PathBuf> {
    startup_dir().map(|d| d.join(LNK_NAME))
}

pub fn enabled() -> bool {
    lnk_path().map(|p| p.is_file()).unwrap_or(false)
}

fn ps_quote(s: &str) -> String {
    s.replace('\'', "''")
}

fn powershell() -> PathBuf {
    if let Some(root) = std::env::var_os("SystemRoot") {
        let p = PathBuf::from(root)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        if p.is_file() {
            return p;
        }
    }
    PathBuf::from("powershell.exe")
}

fn run_hidden(script: &str) -> bool {
    let mut cmd = Command::new(powershell());
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-WindowStyle",
        "Hidden",
        "-Command",
        script,
    ]);
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    match cmd.status() {
        Ok(s) => s.success(),
        Err(_) => false,
    }
}

/// 创建/刷新自启快捷方式。返回是否最终存在该 lnk。
pub fn enable(log: &Logger) -> bool {
    let (Some(lnk), Ok(exe)) = (lnk_path(), std::env::current_exe()) else {
        log.line("[startup] enable failed: no startup dir or exe path");
        return false;
    };
    let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let exe_s = exe.to_string_lossy().to_string();
    if let Some(parent) = lnk.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let script = format!(
        "$ErrorActionPreference='Stop';$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}');\
         $s.TargetPath='{}';$s.WorkingDirectory='{}';$s.IconLocation='{},0';$s.Description='{}';$s.Save()",
        ps_quote(&lnk.to_string_lossy()),
        ps_quote(&exe_s),
        ps_quote(&dir.to_string_lossy()),
        ps_quote(&exe_s),
        ps_quote("项目看板 桌面挂件"),
    );
    let ran = run_hidden(&script);
    let ok = lnk.is_file();
    log.line(&format!(
        "[startup] enable ran={} lnk_exists={} lnk={} target={}",
        ran,
        ok,
        lnk.display(),
        exe_s
    ));
    ok
}

pub fn disable(log: &Logger) -> bool {
    let Some(lnk) = lnk_path() else {
        return false;
    };
    let removed = match std::fs::remove_file(&lnk) {
        Ok(()) => true,
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    };
    log.line(&format!(
        "[startup] disable removed={} exists_after={} lnk={}",
        removed,
        lnk.is_file(),
        lnk.display()
    ));
    removed && !lnk.is_file()
}

/// 切换自启状态，返回切换后的状态（true = 已启用）
pub fn toggle(log: &Logger) -> bool {
    if enabled() {
        disable(log);
        false
    } else {
        enable(log);
        enabled()
    }
}

/// 供菜单/日志展示
pub fn describe() -> String {
    match lnk_path() {
        Some(p) => format!("{} exists={}", p.display(), p.is_file()),
        None => "no startup dir".to_string(),
    }
}
