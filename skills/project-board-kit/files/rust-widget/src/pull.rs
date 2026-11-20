//! 内置采集调度（t7）：启动即抓取 + 日间 60 分钟 / 夜间 240 分钟自动刷新。
//!
//! 设计要点
//! - 子进程用 `CreateProcessW` 直接拉起：`CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP`，
//!   窗口不会闪、Ctrl+C 也不会打到挂件自己的控制台组；
//! - 命令：`pwsh -NoProfile -ExecutionPolicy Bypass -File <每日项目简报.ps1> -NoOpen -NoWidgetRestart`
//!   （`-NoWidgetRestart` 让管道别去重启看板——看板自己在跑，靠 mtime 热重载）；
//! - 间隔每次现读 `<data 目录>/widget_config.json`：
//!   `pullMinutes`（同时覆盖日夜）/ `dayMinutes` / `nightMinutes`，缺省白天 60、夜间 240；
//! - 夜间窗口 22:00–08:00；
//! - 上一轮没结束就不起下一轮：内存里持有进程句柄 + 磁盘 `<data 目录>/widget_pull.pid`
//!   （句柄丢了的场景——比如两个实例——用 PID 存活探测兜住）；
//! - 与 09:10 的计划任务撞车（±3 分钟）时跳过本轮，顺着推到下一个间隔；
//! - 所有调度事件追加到 `<data 目录>/widget_pull.log`，同时进主日志
//!   （`rust-widget.log`），离线化验只看一处就够。
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW, TerminateProcess,
    WaitForSingleObject,
    PROCESS_INFORMATION, STARTUPINFOW,
};

use crate::config::Config;
use crate::log::{now_str, Logger};

/// 白天默认间隔（分钟）
pub const DAY_MINUTES: u32 = 60;
/// 夜间默认间隔（分钟）
pub const NIGHT_MINUTES: u32 = 240;
/// 夜间起点（含）
pub const NIGHT_START: u32 = 22;
/// 夜间终点（不含）
pub const NIGHT_END: u32 = 8;
/// 计划任务时间 09:10
pub const COLLIDE_H: u32 = 9;
pub const COLLIDE_M: u32 = 10;
/// 撞车窗口（±分钟）
pub const COLLIDE_WINDOW: u32 = 3;
/// 单轮最长运行时间：超过就判定卡死并强制结束（45 分钟）
pub const MAX_RUN: Duration = Duration::from_secs(45 * 60);
/// t8 轻量未读通道：默认 1 分钟，允许 1/2/3/5；0 = 关闭
pub const UNREAD_MINUTES_DEFAULT: u32 = 1;
pub const UNREAD_ALLOWED: [u32; 4] = [1, 2, 3, 5];
/// 轻量短连接单轮最长：JS 侧 8s 硬超时，这里 30s 兜底 watchdog
pub const UNREAD_MAX_RUN: Duration = Duration::from_secs(30);

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const STARTF_USESHOWWINDOW: u32 = 0x0001;
const SW_HIDE: u16 = 0;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const WAIT_OBJECT_0: u32 = 0;
const ERROR_ALREADY_EXISTS: u32 = 183;

/// 托盘菜单「立即刷新」/验收脚本置位：下一次 tick 立刻抓一轮
static FORCE_NOW: AtomicI32 = AtomicI32::new(0);

/// 页脚状态快照（render 在 CTX 锁内调用，只读这个独立小锁，不会和 CTX 交叉）
static FOOTER: OnceLock<Mutex<(String, bool)>> = OnceLock::new();

fn footer_slot() -> &'static Mutex<(String, bool)> {
    FOOTER.get_or_init(|| Mutex::new((String::new(), false)))
}

/// 页脚文案 + 是否正在抓取（None = 调度未启用/未初始化，不占位）
pub fn footer_text() -> Option<(String, bool)> {
    let g = footer_slot().lock().ok()?;
    if g.0.is_empty() {
        None
    } else {
        Some((g.0.clone(), g.1))
    }
}

fn set_footer(text: String, running: bool) {
    if let Ok(mut g) = footer_slot().lock() {
        g.0 = text;
        g.1 = running;
    }
}

/// widget_config.json（只认这几个键，别的键原样忽略）
#[derive(Debug, Default, serde::Deserialize)]
struct PullCfg {
    #[serde(default, rename = "pullMinutes")]
    pull_minutes: Option<u32>,
    #[serde(default, rename = "dayMinutes")]
    day_minutes: Option<u32>,
    #[serde(default, rename = "nightMinutes")]
    night_minutes: Option<u32>,
    /// t8 轻量未读短连接间隔（分钟），0 = 关闭
    #[serde(default, rename = "unreadMinutes")]
    unread_minutes: Option<u32>,
}

/// 当前生效的日夜间隔
#[derive(Debug, Clone, Copy)]
pub struct Intervals {
    pub day: u32,
    pub night: u32,
    /// 来源说明，写进日志便于化验
    pub source: &'static str,
}

fn read_pull_cfg(path: &Path) -> Option<PullCfg> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice::<PullCfg>(&bytes).ok()
}

/// 计算间隔：CLI 覆盖 > dayMinutes/nightMinutes > pullMinutes > 默认(60/240)
pub fn intervals(cfg_path: &Path, cli_override: Option<u32>) -> Intervals {
    if let Some(v) = cli_override.filter(|v| *v > 0) {
        return Intervals {
            day: v,
            night: v,
            source: "cli(--pull-minutes)",
        };
    }
    let file = read_pull_cfg(cfg_path);
    let pull = file.as_ref().and_then(|c| c.pull_minutes).filter(|v| *v > 0);
    let day_cfg = file.as_ref().and_then(|c| c.day_minutes).filter(|v| *v > 0);
    let night_cfg = file.as_ref().and_then(|c| c.night_minutes).filter(|v| *v > 0);
    // CLI 覆盖 > dayMinutes/nightMinutes > pullMinutes > 默认(60/240)：
    // day、night 各自先看专属字段，缺了才回退 pullMinutes，都没有才用默认值。
    let day = day_cfg.or(pull).unwrap_or(DAY_MINUTES);
    let night = night_cfg.or(pull).unwrap_or(NIGHT_MINUTES);
    // source 按 day/night 实际生效的字段组合判定，供 [pull]/[pull-plan] 日志排查间隔来源；
    // 不再一律写 config(pullMinutes)（旧写法只配 day/night 时会误报）。
    let source = match (day_cfg.is_some(), night_cfg.is_some(), pull.is_some()) {
        (true, true, _) => "config(day/nightMinutes)",
        (true, false, true) => "config(dayMinutes+pullMinutes)",
        (true, false, false) => "config(dayMinutes)+default(night)",
        (false, true, true) => "config(nightMinutes+pullMinutes)",
        (false, true, false) => "default(day)+config(nightMinutes)",
        (false, false, true) => "config(pullMinutes)",
        (false, false, false) => "default",
    };
    Intervals { day, night, source }
}

/// 某个整点属于夜间吗（22:00–08:00）
pub fn is_night(hour: u32) -> bool {
    !(NIGHT_END..NIGHT_START).contains(&hour)
}

/// 该时刻应使用的间隔
pub fn interval_for(hour: u32, iv: Intervals) -> u32 {
    if is_night(hour) {
        iv.night
    } else {
        iv.day
    }
}

/// 是否落在 09:10 计划任务的 ±3 分钟窗口里
pub fn collides_0910(hour: u32, minute: u32) -> bool {
    let now = hour * 60 + minute;
    let target = COLLIDE_H * 60 + COLLIDE_M;
    now.abs_diff(target) <= COLLIDE_WINDOW
}

fn local_hm() -> (u32, u32, u32) {
    // 验收钩子：RW_PULL_FAKE_HHMM="09:10" 只影响调度判定（不动系统时钟）
    if let Ok(v) = std::env::var("RW_PULL_FAKE_HHMM") {
        let mut it = v.split(':');
        let h = it.next().and_then(|x| x.trim().parse().ok()).unwrap_or(0);
        let m = it.next().and_then(|x| x.trim().parse().ok()).unwrap_or(0);
        return (h, m, 0);
    }
    // (hour, minute, second)
    let t = now_str(); // "YYYY-MM-DD HH:MM:SS"
    let hm = t.split(' ').nth(1).unwrap_or("00:00:00");
    let mut it = hm.split(':');
    let h = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let m = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let s = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    (h, m, s)
}

fn hhmm(h: u32, m: u32) -> String {
    format!("{:02}:{:02}", h % 24, m % 60)
}

/// 从现在起 `mins` 分钟后的墙钟文案（跨天带「明天」）
fn wall_after(mins: u32) -> String {
    let (h, m, _) = local_hm();
    let total = h * 60 + m + mins;
    let day = total / (24 * 60);
    let hh = (total % (24 * 60)) / 60;
    let mm = total % 60;
    if day == 0 {
        hhmm(hh, mm)
    } else if day == 1 {
        format!("明天 {}", hhmm(hh, mm))
    } else {
        format!("{}天后 {}", day, hhmm(hh, mm))
    }
}

fn append_line(path: &Path, text: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{} {}", now_str(), text);
    }
}

/// 进程可执行文件名（小写）。pid 被系统复用时会拿到别的程序，用名字过滤避免误判「仍在抓取」。
fn process_name(pid: u32) -> String {
    if pid == 0 {
        return String::new();
    }
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if ok == 0 {
            return String::new();
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        full.rsplit(['\\', '/'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
    }
}

/// 只有「还在跑的 pwsh 抓取进程」才算忙：pid 存在但已是别的程序（pid 复用）一律视为空闲。
fn pid_alive(pid: u32) -> bool {
    let name = process_name(pid);
    if name.is_empty() {
        return false;
    }
    name == "pwsh.exe" || name == "powershell.exe"
}

/// 找一个 pwsh：Program Files → WindowsApps → PATH（CreateProcess 自己搜）
fn find_pwsh() -> String {
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Some(pf) = std::env::var_os("ProgramFiles") {
        cands.push(PathBuf::from(pf).join("PowerShell").join("7").join("pwsh.exe"));
    }
    if let Some(la) = std::env::var_os("LOCALAPPDATA") {
        cands.push(
            PathBuf::from(la)
                .join("Microsoft")
                .join("WindowsApps")
                .join("pwsh.exe"),
        );
    }
    cands.push(PathBuf::from(r"<ROOT>\PowerShell\7\pwsh.exe"));
    for c in cands {
        if c.is_file() {
            return c.display().to_string();
        }
    }
    "pwsh.exe".to_string()
}

struct Run {
    hproc: isize,
    pid: u32,
    started: Instant,
    started_wall: String,
    reason: String,
}

struct Last {
    started_wall: String,
    done_wall: String,
    ok: bool,
    secs: u64,
}

/// 一轮 tick 的结果，窗口层据此决定要不要立刻重画/重载数据
#[derive(Debug, Default, Clone, Copy)]
pub struct Tick {
    pub started: bool,
    pub finished: bool,
    /// t8 轻量短连接收工：main.rs 据此立刻热载 signals，不等 30s 轮询
    pub unread_finished: bool,
}

pub struct Puller {
    pub enabled: bool,
    /// research 目录（日志/PID/配置都落这里）
    pub dir: PathBuf,
    pub log_file: PathBuf,
    pub pid_file: PathBuf,
    pub cfg_file: PathBuf,
    pub script: PathBuf,
    pub pwsh: String,
    cli_minutes: Option<u32>,
    /// 首轮 = 启动即抓取
    first_pending: bool,
    next_due: Option<Instant>,
    next_wall: String,
    last: Option<Last>,
    running: Option<Run>,
    /// 到点但上一轮还在跑：只记一次，别每秒刷屏
    due_noted: bool,
    intervals_now: Intervals,
    /// t8 轻量未读轮（独立 1 分钟 timer，重管道优先、绝不并发绑 WS）
    unread: UnreadPuller,
}

impl Puller {
    pub fn new(cfg: &Config, log: &Logger) -> Puller {
        let dir = cfg
            .data
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let script = cfg.pull_script.clone().unwrap_or_else(|| {
            dir.parent()
                .map(|p| p.join("每日项目简报.ps1"))
                .unwrap_or_else(|| PathBuf::from("每日项目简报.ps1"))
        });
        let p = Puller {
            enabled: cfg.pull,
            dir: dir.clone(),
            log_file: dir.join("widget_pull.log"),
            pid_file: dir.join("widget_pull.pid"),
            cfg_file: dir.join("widget_config.json"),
            script,
            pwsh: find_pwsh(),
            cli_minutes: cfg.pull_minutes,
            first_pending: cfg.pull,
            next_due: None,
            next_wall: String::new(),
            last: None,
            running: None,
            due_noted: false,
            intervals_now: Intervals {
                day: DAY_MINUTES,
                night: NIGHT_MINUTES,
                source: "init",
            },
            unread: UnreadPuller::new(&dir, &dir.join("widget_config.json"), log),
        };
        let msg = format!(
            "[pull] init enabled={} dir={} script={} script_exists={} pwsh={} log={} pid={} cfg={}",
            p.enabled,
            p.dir.display(),
            p.script.display(),
            p.script.is_file(),
            p.pwsh,
            p.log_file.display(),
            p.pid_file.display(),
            p.cfg_file.display()
        );
        log.line(&msg);
        append_line(&p.log_file, &msg);
        if p.enabled {
            // 启动即抓取：第一次 tick 就开跑
            set_footer("启动抓取中…".into(), true);
        }
        p
    }

    /// 托盘菜单「立即刷新」/ --pull-now
    pub fn trigger_now(&self, log: &Logger) {
        FORCE_NOW.store(1, Ordering::Relaxed);
        log.line("[pull] trigger manual");
        append_line(&self.log_file, "[pull] trigger manual");
    }

    fn schedule(&mut self, log: &Logger, why: &str) {
        let mins = self.plan_next();
        let (h, m, _) = local_hm();
        let iv = self.intervals_now;
        let msg = format!(
            "[pull] schedule why={} now={} interval={}min (day={} night={} source={}) next={}",
            why,
            hhmm(h, m),
            mins,
            iv.day,
            iv.night,
            iv.source,
            self.next_wall
        );
        log.line(&msg);
        append_line(&self.log_file, &msg);
        self.update_footer();
        self.log_footer(log);
    }

    /// 排下一轮（现读配置）：返回间隔分钟数
    fn plan_next(&mut self) -> u32 {
        let iv = intervals(&self.cfg_file, self.cli_minutes);
        self.intervals_now = iv;
        let (h, _, _) = local_hm();
        let mins = interval_for(h, iv);
        self.next_due = Some(Instant::now() + Duration::from_secs(mins as u64 * 60));
        self.next_wall = wall_after(mins);
        self.due_noted = false;
        mins
    }

    fn log_footer(&self, log: &Logger) {
        if let Some((text, running)) = footer_text() {
            log.line(&format!("[pull] footer=\"{}\" running={}", text, running));
        }
    }

    fn update_footer(&self) {
        if !self.enabled {
            set_footer("采集未启用 · 可手动刷新".into(), false);
            return;
        }
        if self.running.is_some() {
            set_footer("抓取中…".into(), true);
            return;
        }
        let text = match &self.last {
            Some(l) => format!(
                "上次抓取 {} · 下次 {}",
                l.done_wall,
                if self.next_wall.is_empty() {
                    "—".to_string()
                } else {
                    self.next_wall.clone()
                }
            ),
            None => format!(
                "下次 {}",
                if self.next_wall.is_empty() {
                    "启动抓取中…".to_string()
                } else {
                    self.next_wall.clone()
                }
            ),
        };
        set_footer(text, false);
    }

    /// 起一轮：撞车/忙则只记日志并顺延
    fn start(&mut self, log: &Logger, reason: &str) {
        let script_ok = self.script.is_file();
        if !script_ok {
            let msg = format!("[pull] skip reason=no-script path={}", self.script.display());
            log.line(&msg);
            append_line(&self.log_file, &msg);
            self.schedule(log, "no-script");
            return;
        }
        // 别的实例正在抓？
        if let Ok(txt) = std::fs::read_to_string(&self.pid_file) {
            if let Some(pid) = txt
                .split_whitespace()
                .find_map(|t| t.strip_prefix("pid=").and_then(|v| v.parse::<u32>().ok()))
            {
                // pid 可能被复用，或该进程早已退出：超过 15 分钟的 pid 文件一律视为过期
                let fresh = std::fs::metadata(&self.pid_file)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .map(|d| d.as_secs() < 900)
                    .unwrap_or(false);
                if fresh && pid_alive(pid) {
                    let msg = format!("[pull] skip reason=busy prev_pid={} (pid file kept)", pid);
                    log.line(&msg);
                    append_line(&self.log_file, &msg);
                    self.schedule(log, "busy");
                    return;
                }
            }
            let _ = std::fs::remove_file(&self.pid_file);
        }
        // 与 09:10 计划任务撞车（只对自动轮生效，手动刷新照跑）
        let (h, m, _) = local_hm();
        if reason == "schedule" && collides_0910(h, m) {
            let msg = format!(
                "[pull] skip reason=0910-collision now={} window=±{}min",
                hhmm(h, m),
                COLLIDE_WINDOW
            );
            log.line(&msg);
            append_line(&self.log_file, &msg);
            self.schedule(log, "collision");
            return;
        }

        let args = format!(
            "-NoProfile -ExecutionPolicy Bypass -File \"{}\" -PullOnly -NoOpen -NoWidgetRestart",
            self.script.display()
        );
        let cmdline = format!("\"{}\" {}", self.pwsh, args);
        let mut cmd_w = crate::wide(&cmdline);
        let cwd = self
            .script
            .parent()
            .map(|p| crate::wide(&p.display().to_string()))
            .unwrap_or_else(|| crate::wide("."));
        unsafe {
            let mut si: STARTUPINFOW = std::mem::zeroed();
            si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            si.dwFlags = STARTF_USESHOWWINDOW;
            si.wShowWindow = SW_HIDE;
            let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
            let ok = CreateProcessW(
                std::ptr::null(),
                cmd_w.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP,
                std::ptr::null(),
                cwd.as_ptr(),
                &si,
                &mut pi,
            );
            if ok == 0 {
                let err = GetLastError();
                let msg = format!(
                    "[pull] spawn failed err={} cmd={}",
                    err, cmdline
                );
                log.line(&msg);
                append_line(&self.log_file, &msg);
                self.schedule(log, "spawn-failed");
                return;
            }
            CloseHandle(pi.hThread);
            let pid = pi.dwProcessId;
            let started_wall = hhmm(h, m);
            let _ = std::fs::write(
                &self.pid_file,
                format!(
                    "pid={} started={} reason={} script={}\n",
                    pid,
                    started_wall,
                    reason,
                    self.script.display()
                ),
            );
            self.running = Some(Run {
                hproc: pi.hProcess as isize,
                pid,
                started: Instant::now(),
                started_wall: started_wall.clone(),
                reason: reason.to_string(),
            });
            let msg = format!(
                "[pull] start reason={} pid={} at={} flags=CREATE_NO_WINDOW|CREATE_NEW_PROCESS_GROUP cmd={}",
                reason, pid, started_wall, cmdline
            );
            log.line(&msg);
            append_line(&self.log_file, &msg);
        }
        // 开跑的同时就把下一轮排上：跑超时也不会有「空档漏拍」
        let _ = self.plan_next();
        self.update_footer();
        self.log_footer(log);
    }

    /// 重管道状态机（每秒一次）；对外入口是文件末尾的 `Puller::tick` wrapper
    pub fn tick_heavy(&mut self, log: &Logger) -> Tick {
        let mut t = Tick::default();
        if !self.enabled {
            return t;
        }
        // 1) 收尸
        if let Some(run) = &self.running {
            let w = unsafe { WaitForSingleObject(run.hproc as *mut core::ffi::c_void, 0) };
            if w == WAIT_OBJECT_0 {
                let mut code: u32 = 0;
                unsafe {
                    GetExitCodeProcess(run.hproc as *mut core::ffi::c_void, &mut code);
                    CloseHandle(run.hproc as *mut core::ffi::c_void);
                }
                let Some(run) = self.running.take() else {
                    return t;
                };
                let secs = run.started.elapsed().as_secs();
                let _ = std::fs::remove_file(&self.pid_file);
                let done_wall = {
                    let (h, m, _) = local_hm();
                    hhmm(h, m)
                };
                self.last = Some(Last {
                    done_wall: done_wall.clone(),
                    ok: code == 0,
                    secs,
                    started_wall: run.started_wall.clone(),
                });
                let msg = format!(
                    "[pull] done pid={} code={} ok={} secs={} started={} done={} reason={}",
                    run.pid,
                    code,
                    code == 0,
                    secs,
                    run.started_wall,
                    done_wall,
                    run.reason
                );
                log.line(&msg);
                append_line(&self.log_file, &msg);
                if let Some(l) = &self.last {
                    log.line(&format!(
                        "[pull] last ok={} secs={} started={} done={}",
                        l.ok, l.secs, l.started_wall, l.done_wall
                    ));
                }
                self.schedule(log, "after-done");
                t.finished = true;
                return t;
            } else if run.started.elapsed() > MAX_RUN {
                let pid = run.pid;
                unsafe {
                    TerminateProcess(run.hproc as *mut core::ffi::c_void, 1);
                }
                let msg = format!("[pull] watchdog kill pid={} elapsed>{:?}", pid, MAX_RUN);
                log.line(&msg);
                append_line(&self.log_file, &msg);
            }
        }

        // 2) 手动触发
        if FORCE_NOW.swap(0, Ordering::Relaxed) != 0 && self.running.is_none() {
            self.first_pending = false;
            self.start(log, "manual");
            t.started = true;
            return t;
        }

        // 3) 启动即抓取
        if self.first_pending {
            self.first_pending = false;
            self.start(log, "startup");
            t.started = true;
            if self.running.is_none() {
                // 没起来（无脚本/被占）：照样排下一轮
            }
            return t;
        }

        // 4) 到点
        if self.running.is_none() {
            if let Some(due) = self.next_due {
                if Instant::now() >= due {
                    self.start(log, "schedule");
                    t.started = true;
                }
            }
        } else if !self.due_noted {
            // 到点但上一轮没结束：不起新的一轮（票数也留在原位），只记一条
            if let Some(due) = self.next_due {
                if Instant::now() >= due {
                    self.due_noted = true;
                    let pid = self.running.as_ref().map(|r| r.pid).unwrap_or(0);
                    let msg = format!(
                        "[pull] wait reason=running prev_pid={} (上一轮未完成，不起下一轮)",
                        pid
                    );
                    log.line(&msg);
                    append_line(&self.log_file, &msg);
                }
            }
        }
        t
    }
}

/// `--pull-plan`：不建窗口，把当前配置下的调度表打出来（验收用）
pub fn plan(cfg: &Config, cli_minutes: Option<u32>) -> String {
    let dir = cfg
        .data
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let cfg_file = dir.join("widget_config.json");
    let script = cfg.pull_script.clone().unwrap_or_else(|| {
        dir.parent()
            .map(|p| p.join("每日项目简报.ps1"))
            .unwrap_or_else(|| PathBuf::from("每日项目简报.ps1"))
    });
    let iv = intervals(&cfg_file, cli_minutes);
    let raw = std::fs::read_to_string(&cfg_file).unwrap_or_default();
    let mut out = String::new();
    out.push_str(&format!(
        "[pull-plan] enabled={} config={} raw={:?} day={} night={} source={} script={} script_exists={}\n",
        cfg.pull,
        cfg_file.display(),
        raw.trim(),
        iv.day,
        iv.night,
        iv.source,
        script.display(),
        script.is_file()
    ));
    out.push_str(&format!(
        "[pull-plan] night_window={:02}:00-{:02}:00 collide={:02}:{:02}±{}min\n",
        NIGHT_START, NIGHT_END, COLLIDE_H, COLLIDE_M, COLLIDE_WINDOW
    ));
    for h in 0..24u32 {
        out.push_str(&format!(
            "[pull-plan] hour={:02} night={} interval={}min collide={}\n",
            h,
            is_night(h),
            interval_for(h, iv),
            collides_0910(h, COLLIDE_M)
        ));
    }
    out
}

/// 单实例：Local 命名互斥量；已在跑返回 true（调用方决定退出）
pub fn single_instance_taken(name: &str) -> bool {
    let w = crate::wide(name);
    unsafe {
        let h = windows_sys::Win32::System::Threading::CreateMutexW(std::ptr::null(), 1, w.as_ptr());
        if h.is_null() {
            return false;
        }
        // 句柄故意不关：进程活多久互斥量就在多久
        GetLastError() == ERROR_ALREADY_EXISTS
    }
}
 
// ============================================================================
// t8 · 轻量未读通道：独立 1 分钟 timer，短连接 bind→getConversation→close。
// 与 30/60 重管道完全解耦：独立 pid 文件、独立超时、同分钟重管道优先让位。
// 绝不调用 每日项目简报.ps1，绝不并发绑 WS。
// ============================================================================

/// 当前 UTC 分钟桶：用于「同一分钟重管道刚起过 → 轻量轮跳过」判定
fn minute_bucket() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 60)
        .unwrap_or(0)
}

/// 重管道优先：正在跑，或同一分钟刚起过，都让轻量轮跳过（绝不并发绑 WS）。
fn should_skip_unread(heavy_running: bool, heavy_minute: Option<u64>, bucket: u64) -> bool {
    heavy_running || heavy_minute == Some(bucket)
}

/// 读 widget_config.json 的 unreadMinutes：缺失/非法回退默认 1，0 = 关闭
fn read_unread_minutes(path: &Path) -> (u32, &'static str) {
    let c = match read_pull_cfg(path) {
        Some(c) => c,
        None => return (UNREAD_MINUTES_DEFAULT, "default(file-missing)"),
    };
    match c.unread_minutes {
        None => (UNREAD_MINUTES_DEFAULT, "default(key-missing)"),
        Some(0) => (0, "config(0=disabled)"),
        Some(v) if UNREAD_ALLOWED.contains(&v) => (v, "config"),
        Some(_) => (UNREAD_MINUTES_DEFAULT, "default(invalid-value)"),
    }
}

fn find_node() -> String {
    if let Ok(v) = std::env::var("NODE_BIN") {
        if !v.trim().is_empty() {
            return v;
        }
    }
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Some(pf) = std::env::var_os("ProgramFiles") {
        cands.push(PathBuf::from(pf).join("nodejs").join("node.exe"));
    }
    if let Some(la) = std::env::var_os("LOCALAPPDATA") {
        cands.push(PathBuf::from(la).join("Programs").join("nodejs").join("node.exe"));
    }
    // 再按 PATH 逐段找（自启 lnk 方式启动时未必带 NODE_BIN）
    if let Some(pv) = std::env::var_os("PATH") {
        for seg in std::env::split_paths(&pv) {
            cands.push(seg.join("node.exe"));
        }
    }
    for c in cands {
        if c.is_file() {
            return c.display().to_string();
        }
    }
    "node.exe".to_string()
}

struct UnreadRun {
    hproc: isize,
    pid: u32,
    started: Instant,
}

/// t8 轻量未读轮调度器（挂在 Puller 内，由 Puller::tick 每秒推进）
struct UnreadPuller {
    dir: PathBuf,
    cfg_file: PathBuf,
    script: PathBuf,
    node: String,
    pid_file: PathBuf,
    log_file: PathBuf,
    minutes: u32,
    next_due: Option<Instant>,
    running: Option<UnreadRun>,
    /// 重管道最近一次启动所在的分钟桶
    heavy_minute: Option<u64>,
    /// 撞车跳过只记一次，避免每 15s 刷屏
    skipped_noted: bool,
}

impl UnreadPuller {
    fn new(dir: &Path, cfg_file: &Path, log: &Logger) -> UnreadPuller {
        let script = dir.join("raw").join("emobile").join("em_unread_live.js");
        let pid_file = dir.join("unread_pull.pid");
        let log_file = dir.join("widget_pull.log");
        let node = find_node();
        let (minutes, src) = read_unread_minutes(cfg_file);
        let mut u = UnreadPuller {
            dir: dir.to_path_buf(),
            cfg_file: cfg_file.to_path_buf(),
            script,
            node,
            pid_file,
            log_file,
            minutes,
            next_due: None,
            running: None,
            heavy_minute: None,
            skipped_noted: false,
        };
        let msg = format!(
            "[unread] init enabled={} interval={}min source={} script={} script_exists={} node={} pid={}",
            minutes > 0,
            minutes,
            src,
            u.script.display(),
            u.script.is_file(),
            u.node,
            u.pid_file.display()
        );
        log.line(&msg);
        append_line(&u.log_file, &msg);
        u.schedule(log, "init");
        u
    }

    /// 重管道启动/手动触发时记一分钟桶：同分钟轻量轮让位，避免并发绑 WS
    pub fn note_heavy_start(&mut self) {
        self.heavy_minute = Some(minute_bucket());
    }

    fn schedule(&mut self, log: &Logger, why: &str) {
        let (mins, src) = read_unread_minutes(&self.cfg_file);
        self.minutes = mins;
        if mins == 0 {
            self.next_due = None;
            let msg = format!("[unread] schedule why={} disabled (unreadMinutes=0)", why);
            log.line(&msg);
            append_line(&self.log_file, &msg);
            return;
        }
        self.next_due = Some(Instant::now() + Duration::from_secs(mins as u64 * 60));
        let msg = format!(
            "[unread] schedule why={} interval={}min source={} next={}",
            why,
            mins,
            src,
            wall_after(mins)
        );
        log.line(&msg);
        append_line(&self.log_file, &msg);
    }

    /// 每秒推进。heavy_running 来自 Puller.running；结束时置 t.unread_finished
    fn tick_unread(&mut self, log: &Logger, heavy_running: bool, t: &mut Tick) {
        // 1) 收尸
        if let Some(run) = &self.running {
            let w = unsafe { WaitForSingleObject(run.hproc as *mut core::ffi::c_void, 0) };
            if w == WAIT_OBJECT_0 {
                let mut code: u32 = 0;
                unsafe {
                    GetExitCodeProcess(run.hproc as *mut core::ffi::c_void, &mut code);
                    CloseHandle(run.hproc as *mut core::ffi::c_void);
                }
                let secs = run.started.elapsed().as_secs();
                let pid = run.pid;
                self.running = None;
                let _ = std::fs::remove_file(&self.pid_file);
                let msg = format!("[unread] done pid={} code={} ok={} secs={}", pid, code, code == 0, secs);
                log.line(&msg);
                append_line(&self.log_file, &msg);
                // main.rs 只按 widget_data.json mtime 决定要不要热载 signals
                t.unread_finished = true;
                self.schedule(log, "after-done");
                return;
            } else if run.started.elapsed() > UNREAD_MAX_RUN {
                let pid = run.pid;
                unsafe {
                    TerminateProcess(run.hproc as *mut core::ffi::c_void, 1);
                }
                let msg = format!("[unread] watchdog kill pid={} elapsed>{:?}", pid, UNREAD_MAX_RUN);
                log.line(&msg);
                append_line(&self.log_file, &msg);
            }
        }
        if self.running.is_some() {
            return; // 自身上一轮没结束，绝不起第二个 WS
        }
        let Some(due) = self.next_due else {
            return; // unreadMinutes=0：轻量轮关闭
        };
        if Instant::now() < due {
            return;
        }
        // 2) 重管道优先：正在跑，或同一分钟刚起过 → 跳过并短顺延重试
        let bucket = minute_bucket();
        if should_skip_unread(heavy_running, self.heavy_minute, bucket) {
            if !self.skipped_noted {
                let reason = if heavy_running { "heavy-running" } else { "heavy-same-minute" };
                let msg = format!(
                    "[unread] skip reason={} bucket={} retry_in=15s (重管道优先，不并发绑 WS)",
                    reason, bucket
                );
                log.line(&msg);
                append_line(&self.log_file, &msg);
                self.skipped_noted = true;
            }
            self.next_due = Some(Instant::now() + Duration::from_secs(15));
            return;
        }
        self.skipped_noted = false;
        self.start(log);
    }

    fn start(&mut self, log: &Logger) {
        if !self.script.is_file() {
            let msg = format!("[unread] skip reason=no-script path={}", self.script.display());
            log.line(&msg);
            append_line(&self.log_file, &msg);
            self.schedule(log, "no-script");
            return;
        }
        // 单实例：别的挂件实例正在跑轻量轮就不绑第二个 WS
        if let Ok(txt) = std::fs::read_to_string(&self.pid_file) {
            if let Some(pid) = txt
                .split_whitespace()
                .find_map(|t| t.strip_prefix("pid=").and_then(|v| v.parse::<u32>().ok()))
            {
                let fresh = std::fs::metadata(&self.pid_file)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .map(|d| d.as_secs() < 60)
                    .unwrap_or(false);
                let pname = process_name(pid);
                if fresh && !pname.is_empty() && pname.contains("node") {
                    let msg = format!("[unread] skip reason=busy prev_pid={}", pid);
                    log.line(&msg);
                    append_line(&self.log_file, &msg);
                    self.schedule(log, "busy");
                    return;
                }
            }
            let _ = std::fs::remove_file(&self.pid_file);
        }
        let cwd = self
            .script
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.dir.clone());
        let cmdline = format!("\"{}\" \"{}\" --once", self.node, self.script.display());
        let mut cmd_w = crate::wide(&cmdline);
        let cwd_w = crate::wide(&cwd.display().to_string());
        unsafe {
            let mut si: STARTUPINFOW = std::mem::zeroed();
            si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            si.dwFlags = STARTF_USESHOWWINDOW;
            si.wShowWindow = SW_HIDE;
            let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
            let ok = CreateProcessW(
                std::ptr::null(),
                cmd_w.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP,
                std::ptr::null(),
                cwd_w.as_ptr(),
                &si,
                &mut pi,
            );
            if ok == 0 {
                let err = GetLastError();
                let msg = format!("[unread] spawn failed err={} cmd={}", err, cmdline);
                log.line(&msg);
                append_line(&self.log_file, &msg);
                self.schedule(log, "spawn-failed");
                return;
            }
            CloseHandle(pi.hThread);
            let pid = pi.dwProcessId;
            let (h, m, _) = local_hm();
            let _ = std::fs::write(
                &self.pid_file,
                format!(
                    "pid={} started={} reason=unread-live script={}",
                    pid,
                    hhmm(h, m),
                    self.script.display()
                ),
            );
            self.running = Some(UnreadRun {
                hproc: pi.hProcess as isize,
                pid,
                started: Instant::now(),
            });
            let msg = format!(
                "[unread] start pid={} at={} flags=CREATE_NO_WINDOW|CREATE_NEW_PROCESS_GROUP cmd={}",
                pid,
                hhmm(h, m),
                cmdline
            );
            log.line(&msg);
            append_line(&self.log_file, &msg);
        }
        // 起跑同时排下一轮，卡死也不会漏拍
        self.next_due = Some(Instant::now() + Duration::from_secs(self.minutes.max(1) as u64 * 60));
    }
}

impl Puller {
    /// 每秒一次：先推重管道，再推 t8 轻量未读轮（重管道优先、同分钟不并发）
    pub fn tick(&mut self, log: &Logger) -> Tick {
        let mut t = self.tick_heavy(log);
        if !self.enabled {
            return t; // --no-pull：重管道与轻量轮一起关，绝不绑 WS
        }
        if t.started {
            // 重管道刚起：记分钟桶，同分钟轻量轮让位，绝不并发绑 WS
            self.unread.note_heavy_start();
        }
        let heavy_running = self.running.is_some();
        self.unread.tick_unread(log, heavy_running, &mut t);
        t
    }
}

#[cfg(test)]
mod t8_unread_tests {
    use super::*;

    #[test]
    fn heavy_running_wins_over_unread() {
        assert!(should_skip_unread(true, None, 100));
    }

    #[test]
    fn same_minute_heavy_start_skips_unread() {
        assert!(should_skip_unread(false, Some(100), 100));
    }

    #[test]
    fn different_minute_allows_unread() {
        assert!(!should_skip_unread(false, Some(99), 100));
        assert!(!should_skip_unread(false, None, 100));
    }
}
