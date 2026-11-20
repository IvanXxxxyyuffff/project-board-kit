// 日志：统一追加到 <ROOT>\research\rust-widget.log（可由 --log 改写）。
// 每行前缀本地时间，UTF-8 无 BOM；同时回显 stdout（headless 调试用）。
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::System::Time::{
    GetTimeZoneInformation, TIME_ZONE_ID_INVALID, TIME_ZONE_INFORMATION,
};

#[derive(Clone)]
pub struct Logger {
    path: PathBuf,
}

impl Logger {
    pub fn new(path: PathBuf) -> Logger {
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        Logger { path }
    }

    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn line(&self, msg: &str) {
        let s = format!("{} {}\n", now_str(), msg);
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            let _ = f.write_all(s.as_bytes());
        }
        print!("{s}");
        let _ = std::io::stdout().flush();
    }
}

/// 本地时间 `YYYY-MM-DD HH:MM:SS`，不引入 chrono。
pub fn now_str() -> String {
    unsafe {
        let mut st: SYSTEMTIME = std::mem::zeroed();
        GetLocalTime(&mut st);
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
        )
    }
}

/// 本地日期 `YYYY-MM-DD`（倒计时一律用它，避免数据快照过期导致「今天」错位）。
pub fn now_date() -> String {
    unsafe {
        let mut st: SYSTEMTIME = std::mem::zeroed();
        GetLocalTime(&mut st);
        format!("{:04}-{:02}-{:02}", st.wYear, st.wMonth, st.wDay)
    }
}

/// 本地时区相对 UTC 的偏移（分钟），如东八区 = +480。取不到系统时区时按 0 处理。
#[allow(dead_code)]
fn tz_offset_minutes() -> i32 {
    unsafe {
        let mut tz: TIME_ZONE_INFORMATION = std::mem::zeroed();
        let id = GetTimeZoneInformation(&mut tz);
        if id == TIME_ZONE_ID_INVALID {
            return 0;
        }
        // Bias 是「本地 → UTC」的分钟数（东八区为 -480），夏令时另加 DaylightBias。
        let sub = if id == 2 {
            tz.DaylightBias
        } else {
            tz.StandardBias
        };
        -(tz.Bias + sub)
    }
}

/// ISO 8601 带本地时区偏移：`2026-11-10T14:40:12+08:00`。归档时间戳用它。
#[allow(dead_code)]
pub fn now_iso_local() -> String {
    let (y, mo, d, h, mi, s) = unsafe {
        let mut st: SYSTEMTIME = std::mem::zeroed();
        GetLocalTime(&mut st);
        (st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond)
    };
    let off = tz_offset_minutes();
    let sign = if off < 0 { '-' } else { '+' };
    let a = off.abs();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
        y,
        mo,
        d,
        h,
        mi,
        s,
        sign,
        a / 60,
        a % 60
    )
}
