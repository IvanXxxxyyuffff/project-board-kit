// rust-widget — 「项目看板」桌面挂件（t3：双标签页 + 月历 + 待办）
// 980×566 无边框、无激活、不进 Alt-Tab、纯 alpha 贴图（无模糊 / 无 Acrylic / 无点击穿透）。
// 渲染：WS_EX_LAYERED + UpdateLayeredWindow（预乘 ARGB，软件光栅化圆角）。
// 命中：整数矩形 + z 序功能异常（hit.rs），不再用 WM_NCHITTEST 返 HTCAPTION。
// 窗口层三档：embed（GWL_HWNDPARENT=Progman + HWND_BOTTOM）/ normal / topmost。
#![windows_subsystem = "windows"]

mod canvas;
mod config;
mod elevate;
mod data;
mod hit;
mod layout;
mod log;
mod meet;
mod popup;
mod net;
mod wxhandle;
mod lunar;
#[allow(dead_code)]
mod png;
mod pull;
mod state;
mod text;
mod thumb;
mod cmd;
mod tray;
mod startup;
mod view;

use std::ffi::c_void;
use std::io::Write;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicI32, AtomicIsize, Ordering};
use std::sync::{Mutex, OnceLock};

// 备注框：拖选状态 / 选区起点（放在静态里，避免给 UiLocal 三处构造器都加字段）
thread_local! {
    static NOTE_DRAG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static NOTE_ANCHOR: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
static ITEM_DRAG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
static ITEM_ANCHOR: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
use std::time::{Duration, Instant, SystemTime};

use windows_sys::Win32::Foundation::{
    GetLastError, SetLastError, COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateDIBSection, DeleteDC,
    DeleteObject, EndPaint, GetDC, GetDIBits, ReleaseDC, ScreenToClient, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BLENDFUNCTION, HGDIOBJ, PAINTSTRUCT, AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB,
    DIB_RGB_COLORS, SRCCOPY,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus as SetFocusW,
};
use windows_sys::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OPENFILENAMEW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST,
};
use windows_sys::Win32::UI::Input::Ime::{
    ImmGetCompositionStringW, ImmGetContext, ImmReleaseContext,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, FindWindowW, GetCursorPos, GetShellWindow, GetWindowLongPtrW, GetWindowLongW,
    SetWindowLongW, GWL_EXSTYLE, GWL_HWNDPARENT, GetDlgItem, GetMessageW, GetSystemMetrics,
    GetClientRect, GetWindowRect, GetWindowTextW, HICON, IsWindowVisible, LoadCursorW, PostQuitMessage,
    RegisterClassW,
    SetCursor, SetForegroundWindow, SetParent, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    SystemParametersInfoW, TranslateMessage, UpdateLayeredWindow, GWLP_HWNDPARENT,
    HTCLIENT, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT,
    MINMAXINFO, WM_EXITSIZEMOVE, WM_GETMINMAXINFO, WM_SIZE, HWND_BOTTOM, HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST, IDC_ARROW, IDC_IBEAM, IDC_SIZEALL, MSG, SM_CXSCREEN,
    SM_CYSCREEN, SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE,
    SW_SHOW, SW_SHOWNORMAL, SW_SHOWNOACTIVATE,
    ULW_ALPHA, WM_CHAR, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_NCHITTEST, WM_PAINT, WM_SETCURSOR, WM_TIMER, WNDCLASSW, WS_CHILD, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::UI::Shell::ShellExecuteW;

use crate::config::Config;
use crate::hit::{Hit, Kind, Hits};
use crate::log::Logger;
use crate::view::{DeptRow, Tab, View};
use cmd::*;


const TIMER_ID: usize = 1;
const TICK_MS: u32 = 1000;
const MEM_EVERY_TICKS: u32 = 30;
const CLASS_NAME: &str = "RustWidgetShell";
const EDIT_CLASS: &str = "RustWidgetEditor";
const TITLE: &str = "项目看板";

const WM_MENU: u32 = 0x8000 + 1; // WM_APP + 1：托盘回调（验收脚本 PostMessage 用这个）
const MENU_TOGGLE: usize = 1;
const MENU_RELOAD: usize = 2;
const MENU_OPEN: usize = 3;
const MENU_STARTUP: usize = 4;
const MENU_QUIT: usize = 5;
// 菜单项标志（AppendMenuW 的 uFlags）

/// 托盘 HICON（WM_DESTROY 时释放）与安装状态
static TRAY_ICON: AtomicIsize = AtomicIsize::new(0);
static TRAY_ON: AtomicI32 = AtomicI32::new(0);

const DEL_CONFIRM: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Embed,
    Normal,
    Topmost,
}

impl Mode {
    fn from_key(s: &str) -> Mode {
        match s {
            "topmost" => Mode::Topmost,
            "normal" | "float" => Mode::Normal,
            _ => Mode::Embed,
        }
    }
    fn key(self) -> &'static str {
        match self {
            Mode::Embed => "embed",
            Mode::Normal => "float",
            Mode::Topmost => "topmost",
        }
    }
}

/// UI 侧局部状态（tab / 可见月份 / 选中日 / 锁定 / 删除二次确认）。
struct UiLocal {
    tab: String,
    y: i32,
    mo: u32,
    sel_day: i32,
    scroll: usize,
    locked: bool,
    /// (按钮 id, 截止时刻)  —— 行删除二次确认
    pending: Option<(String, Instant)>,
    /// 标签二内归档视图
    arc_mode: bool,
    arc_scroll: usize,
    /// (按钮 id, 截止时刻) —— 归档彻底删除二次确认
    arc_pending: Option<(String, Instant)>,
    /// 新建待办：行内输入行状态（＋ 打开、回车提交、Esc 取消）
    newtodo: view::NewTodo,
    /// 新建待办模态窗（＋ 现在走这个；行内输入行保留但不再触发）
    modal: view::ModalForm,
    set: view::SetForm,
    /// 鼠标客户区坐标；(-1,-1) 表示不在窗口内
    mouse: (i32, i32),
    /// 上次悬停重绘时刻（毫秒，节流）
    hover_ms: u64,
    /// 行拖拽排序：(组 0=自建 1=部门, 行下标, 按下时的 y)
    row_drag: Option<(u8, usize, i32)>,
    drag_to: Option<usize>,
    drag_u: Option<usize>,
    /// E-Mobile 状态灯说明浮层
    em_tip: bool,
    /// IME 上屏时记录的待去重字符（WM_IME_COMPOSITION 写入，WM_IME_CHAR/WM_CHAR 命中即吞）
    ime_pending: Vec<char>,
    /// 上一个真正入库的字符与时刻，用于 60ms 同字符去重
    last_ch: char,
    last_at: std::time::Instant,
    /// 设置面板：正在拖动不透明度滑块
    set_dragging: bool,
}

struct Present {
    screen: isize,
    dc: isize,
    #[allow(dead_code)]
    bmp: isize,
    bits: isize,
    w: i32,
    h: i32,
}

unsafe impl Send for Present {}

impl Present {
    fn new(w: i32, h: i32) -> Present {
        unsafe {
            let screen = GetDC(null_mut());
            let dc = CreateCompatibleDC(screen);
            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            };
            let mut bits: *mut c_void = null_mut();
            let bmp = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if !bmp.is_null() {
                SelectObject(dc, bmp);
            }
            Present {
                screen: screen as isize,
                dc: dc as isize,
                bmp: bmp as isize,
                bits: bits as isize,
                w,
                h,
            }
        }
    }

    /// 帧缓冲 → DIB → UpdateLayeredWindow（DIB 只分配一次）。
    unsafe fn present(&mut self, hwnd: HWND, cv: &canvas::Canvas, alpha: f32) {
        if self.bits == 0 || cv.w <= 0 || cv.h <= 0 {
            return;
        }
        // 画布永远是逻辑尺寸（980×566）。窗口被拖大/拖小时不需要重建位图：
        // UpdateLayeredWindow 的目标尺寸取客户区，系统自己拉伸这张 1:1 帧，
        // 这样拖拽缩放不会在 UI 线程里重采样几十万像素（之前就是这么卡死的）。
        if cv.w == self.w && cv.h == self.h {
            let dst = self.bits as *mut u32;
            let n = (self.w * self.h) as usize;
            let k = (alpha.clamp(0.0, 1.0) * 255.0).round() as u32;
            if k >= 255 {
                std::ptr::copy_nonoverlapping(cv.px.as_ptr(), dst, n);
            } else {
                for i in 0..n {
                    let p = cv.px[i];
                    let a = ((p >> 24) & 255) * k / 255;
                    let r = ((p >> 16) & 255) * k / 255;
                    let g = ((p >> 8) & 255) * k / 255;
                    let b = (p & 255) * k / 255;
                    *dst.add(i) = (a << 24) | (r << 16) | (g << 8) | b;
                }
            }
        }
        let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        let _ = GetClientRect(hwnd, &mut rc);
        let (dw, dh) = if rc.right > 0 && rc.bottom > 0 {
            (rc.right, rc.bottom)
        } else {
            (self.w, self.h)
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let pt = POINT { x: 0, y: 0 };
        let size = SIZE { cx: dw, cy: dh };
        let _ = UpdateLayeredWindow(
            hwnd,
            self.screen as *mut c_void,
            std::ptr::null(),
            &size,
            self.dc as *mut c_void,
            &pt,
            0 as COLORREF,
            &blend,
            ULW_ALPHA,
        );
    }
}

struct Ctx {
    cfg: Config,
    data: data::WidgetData,
    st: state::AppState,
    log: Logger,
    last_mtime: Option<SystemTime>,
    last_state_mtime: Option<SystemTime>,
    ticks: u32,
    ui: UiLocal,
    mode: Mode,
    canvas: canvas::Canvas,
    /// 静止层命中区（panel/card/header chips），每次 Full 从它复制
    base_hits: Hits,
    /// 静止层（面板/头部/卡片框）：脏区重画时从这里 blit，避免半透明底二次混合
    base: canvas::Canvas,
    /// base 是否可用：只有数据/字号/字体变化时才重建（面板 SDF 约 20ms，不能每帧画）
    base_valid: bool,
    te: text::TextEngine,
    hits: Hits,
    pres: Present,
    hwnd: isize,
    /// 拖动偏移：鼠标按下时的窗口左上角
    drag: Option<(i32, i32)>,
    /// 首帧时间戳，用于 A01 启动耗时日志
    t_start: Instant,
    /// 内置采集调度（t7）：启动即抓取 + 日夜间隔自动刷新
    pull: pull::Puller,
}

static CTX: OnceLock<Mutex<Ctx>> = OnceLock::new();

/// 给不持锁的路径（菜单、光标的原子读）
static MODE_ATOMIC: AtomicI32 = AtomicI32::new(0);
static LOCKED_ATOMIC: AtomicI32 = AtomicI32::new(0);
static DRAGGING: AtomicI32 = AtomicI32::new(0);
static EDIT_OWNER: AtomicIsize = AtomicIsize::new(0);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn main() {
    // --netcheck <url>：只验证内置 HTTP 客户端（TLS/内网连通），不读写任何业务文件
    let argv: Vec<String> = std::env::args().collect();
    if let Some(i) = argv.iter().position(|a| a == "--wxhandle") {
        let f = argv.get(i + 1).cloned().unwrap_or_default();
        let o = argv.get(i + 2).cloned().unwrap_or_else(|| "wxhandle_out.bin".into());
        wxhandle::selfcheck(&f, &o);
        std::process::exit(0);
    }
    if let Some(i) = argv.iter().position(|a| a == "--netcheck") {
        let url = argv.get(i + 1).cloned().unwrap_or_else(|| "https://doc.weixin.qq.com/".into());
        net::selfcheck(&url);
        return;
    }
    let cfg = Config::from_args();
    config::set_data_file(&cfg.data);
    if cfg.show_version {
        println!("项目看板 v{} (rich-text build)", config::APP_VERSION);
        return;
    }

    // 单实例保护：除 --multi / --selftest / --dump-hit 外，已有实例在跑就静默退出。
    // 先试 Global（跨会话唯一：计划任务、WMI、用户会话共享一个名字），
    // 没有 SeCreateGlobalPrivilege 时退回 Local（同会话唯一），两级都不会漏。
    if !cfg.multi && !cfg.selftest && cfg.dump_hit.is_none() {
        use windows_sys::Win32::System::Threading::CreateMutexW;
        let mk = |ns: &str| -> (isize, u32) {
            let mut name: Vec<u16> = format!("{}\\FRD_BOARD_WIDGET_SINGLETON", ns).encode_utf16().collect();
            name.push(0);
            let h = unsafe { CreateMutexW(std::ptr::null(), 1, name.as_ptr()) } as isize;
            let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
            (h, err)
        };
        let (h_g, e_g) = mk("Global");
        let already = if h_g != 0 { e_g == 183u32 } else { mk("Local").1 == 183u32 };
        if already {
            eprintln!("[start] another instance is running, exit");
            return;
        }
    }
    let lg = Logger::new(cfg.log_path());
    lg.line(&format!(
        "[start] rust-widget {} pid={} {}",
        config::APP_VERSION,
        std::process::id(),
        cfg.summary()
    ));

    if cfg.help {
        lg.line(config::USAGE);
        return;
    }

    if cfg.selftest {
        // 自检只在临时副本上跑：绝不改写用户真实的 widget_state.json
        // 沙箱文件放 state 同目录（%TEMP% 在某些受控环境不可写），跑完即删
        let mut tmp: std::path::PathBuf = cfg
            .state
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(std::env::temp_dir);
        tmp.push(format!("_selftest_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        lg.line(&format!("[selftest] sandbox copy={}", tmp.display()));
        let ok = state::selftest(&tmp, &cfg.yynote, &lg);
        let _ = std::fs::remove_file(&tmp);
        lg.line(&format!("[selftest] exit code={}", if ok { 0 } else { 1 }));
        std::process::exit(if ok { 0 } else { 1 });
    }

    // 自启开关（与托盘菜单共用 startup::*，执行完直接退出，不建窗口）
    if let Some(action) = cfg.autostart.clone() {
        let code = match action.as_str() {
            "on" => {
                let ok = startup::enable(&lg);
                if ok {
                    0
                } else {
                    1
                }
            }
            "off" => {
                let ok = startup::disable(&lg);
                if ok {
                    0
                } else {
                    1
                }
            }
            "toggle" => {
                startup::toggle(&lg);
                0
            }
            _ => {
                lg.line(&format!(
                    "[startup] status enabled={} {}",
                    startup::enabled(),
                    startup::describe()
                ));
                0
            }
        };
        std::process::exit(code);
    }

    if let Some(px) = cfg.font_size {
        let pct = (px.clamp(10, 18) * 100 + 6) / 13;
        layout::set_font_pct(pct);
        lg.line(&format!("[ui] font_size={} px pct={}%", px, pct));
    }

    let (d, data_ok) = data::load(&cfg.data, &lg);
    lg.line(&format!(
        "[verify] parse_ok={} todo={} p0={} nodes={} signals={} perDay={}",
        data_ok,
        d.todo.len(),
        d.p0.len(),
        d.nodes.len(),
        d.signals.len(),
        d.cal.per_day.len()
    ));
    let mut st = state::load(&cfg.state, &cfg.yynote, &lg);
    let swept = st.sweep_done();
    if swept > 0 {
        lg.line(&format!("[state] sweep_done {} -> {}", swept, st.summary()));
        state::save(&cfg.state, &st, &lg);
    }

    if cfg.dump_hit.is_some() || cfg.snap.is_some() {
        dump_artifacts(&cfg, &d, &st, &lg);
        if cfg.headless {
            lg.line("[dump] done (headless) exit 0");
            return;
        }
    }

    if cfg.headless {
        headless(cfg, d, st, lg);
        return;
    }

    // --pull-plan：只打调度表（日夜间隔 / 09:10 撞车窗口），不建窗口
    if cfg.pull_plan {
        let text = pull::plan(&cfg, cfg.pull_minutes);
        for line in text.lines() {
            lg.line(line);
        }
        return;
    }

    // 单实例（t7）：GUI 只允许一份；--multi 让验收并行起多份
    if !cfg.multi {
        if pull::single_instance_taken("Local\\项目看板WidgetSingle") {
            lg.line("[single] another instance is running -> exit 0");
            return;
        }
        lg.line("[single] mutex acquired name=Local\\项目看板WidgetSingle");
    } else {
        lg.line("[single] skipped (--multi)");
    }

    run_gui(cfg, d, st, lg);
}

// ───────────────────────── 离线产物：命中区 JSON / PNG ─────────────────────────

fn build_view<'a>(d: &'a data::WidgetData, st: &state::AppState, ui: &UiLocal) -> View<'a> {
    let mut v = View::new(d);
    v.tab = Tab::from_key(&ui.tab);
    MEET.with(|m| {
        if let Some(md) = m.borrow().as_ref() {
            v.meet_today = meet_future_only(md.today.clone());
            v.meet_week = meet_future_only(md.week.clone());
            v.meet_month = meet_future_only(md.month.clone());
            v.meet_span = md.span;
            v.meet_at = md.at.clone();
        }
    });
    v.remind = REMIND.lock().ok().and_then(|g| (*g).clone());
    v.y = ui.y;
    v.mo = ui.mo;
    v.sel_day = ui.sel_day;
    v.scroll = ui.scroll;
    // 点日历某天 → 左卡切换成当天节点清单
    if ui.sel_day > 0 && d.cal.mo > 0 {
        let target = format!("{:04}-{:02}-{:02}", d.cal.y, d.cal.mo, ui.sel_day);
        for nd in d.nodes.iter().filter(|x| x.d == target) {
            v.day_list.push(view::DeptRow {
                text: format!("{} · {}", nd.model, nd.name),
                due: String::new(),
                done: false,
                prio: nd.prio.clone(),
                img: String::new(),
                note: String::new(),
            });
        }
        v.day_label = format!("{:02}月{:02}日 节点 {}", d.cal.mo, ui.sel_day, v.day_list.len());
    }
    v.locked = ui.locked;
    v.drag = match (ui.drag_u, ui.row_drag) {
        (Some(u), Some((_, _, y0))) => Some((u, ui.mouse.1 - y0, ui.drag_to.unwrap_or(u))),
        _ => None,
    };
    v.done = st.done.clone();
    for it in &st.department {
        v.dept.push(DeptRow {
            text: it.text.clone(),
            due: it.due.clone().unwrap_or_default(),
            done: it.done,
            prio: it.prio.clone(),
            img: String::new(),
            note: it.note.clone(),
        });
    }
    for it in &st.custom {
        v.custom.push(DeptRow {
            text: it.text.clone(),
            due: it.due.clone(),
            done: it.done,
            prio: it.prio.clone(),
            img: it.img.clone(),
            note: it.note.clone(),
        });
    }
    v.pending_del = ui.pending.as_ref().map(|(id, _)| id.clone());
    v.arc_mode = ui.arc_mode;
    v.arc_scroll = ui.arc_scroll;
    v.arc_pending = ui.arc_pending.as_ref().map(|(id, _)| id.clone());
    v.archive = st
        .archive_list()
        .iter()
        .map(|a| view::ArcRow {
            id: a.id.clone(),
            text: a.text.clone(),
            due: a.due.clone().unwrap_or_default(),
            done: a.done,
            at: short_md(&a.archived_at),
        })
        .collect();
    v.newtodo = ui.newtodo.clone();
    v.modal = ui.modal.clone();
    v.set = ui.set.clone();
    v.mouse = ui.mouse;
    let e = &d.health.emobile;
    v.emstat = emstat_of(
        e.run_ok,
        e.auth_ok,
        e.scram_ok,
        !e.auth_at.is_empty(),
        e.msgs,
        e.age_min,
        e.interval_min,
        !d.health.alerts.is_empty(),
    );
    v.em_tip = ui.em_tip;
    v
}

/// 状态灯：1 绿 / 2 黄 / 3 红。
/// 绿 = 管道成功、令牌可用、SCRAM 握手成功，且数据在「周期 + 15 分钟」内；
/// 黄 = 抓取成功但已超一个周期，或有告警；
/// 红 = 上一轮失败 / 令牌不可用 / 握手失败 / 超过两个周期没有成功抓取。
/// 老数据没有 authOk/scramOk/authAt 字段时，按「runOk 且有数据」兼容为通过，避免升级即误报红。
fn emstat_of(
    run_ok: bool,
    auth_ok: bool,
    scram_ok: bool,
    has_auth_at: bool,
    msgs: i64,
    age_min: i64,
    interval_min: i64,
    has_alerts: bool,
) -> u8 {
    let iv = if interval_min > 0 { interval_min } else { 30 };
    let stale = age_min < 0 || age_min > iv * 2;
    let has_auth_fields = auth_ok || scram_ok || has_auth_at;
    let authed = if has_auth_fields {
        auth_ok && scram_ok
    } else {
        run_ok && msgs > 0
    };
    if !run_ok || !authed || stale {
        3
    } else if has_alerts || age_min > iv + 15 {
        2
    } else {
        1
    }
}

#[cfg(test)]
mod emstat_tests {
    use super::emstat_of;

    #[test]
    fn green_when_all_good() {
        assert_eq!(emstat_of(true, true, true, true, 2264, 3, 30, false), 1);
    }

    #[test]
    fn red_when_token_missing() {
        assert_eq!(emstat_of(true, false, true, true, 2264, 3, 30, false), 3);
    }

    #[test]
    fn red_when_scram_failed() {
        assert_eq!(emstat_of(true, true, false, true, 2264, 3, 30, false), 3);
    }

    #[test]
    fn red_when_stale_beyond_two_intervals() {
        assert_eq!(emstat_of(true, true, true, true, 2264, 61, 30, false), 3);
    }

    #[test]
    fn amber_when_alerts_or_slightly_late() {
        assert_eq!(emstat_of(true, true, true, true, 2264, 3, 30, true), 2);
        assert_eq!(emstat_of(true, true, true, true, 2264, 46, 30, false), 2);
    }

    #[test]
    fn legacy_without_auth_fields_is_compatible() {
        assert_eq!(emstat_of(true, false, false, false, 2264, 3, 30, false), 1);
        assert_eq!(emstat_of(true, false, false, false, 0, 3, 30, false), 3);
    }
}

/// "2026-11-10 14:01:33" -> "11-10"
fn short_md(ts: &str) -> String {
    if ts.len() >= 10 {
        ts[5..10].to_string()
    } else {
        ts.to_string()
    }
}

fn tab_json(tab: &str, hits: &Hits) -> String {
    format!(
        "{{\"tab\":\"{}\",\"w\":{},\"h\":{},\"regions\":{}}}",
        tab,
        WIN_W,
        WIN_H,
        hits.json()
    )
}

/// 命中区 + 可选 PNG：不需要窗口就能跑（verifier / CI 友好）。
fn dump_artifacts(cfg: &Config, d: &data::WidgetData, st: &state::AppState, lg: &Logger) {
    let mut te = text::TextEngine::new();
    layout::set_opaque(cfg.opaque);
    let mut cv = canvas::Canvas::new(WIN_W, WIN_H);
    let mut per_tab: Vec<(String, String)> = Vec::new();

    // 三份：proj / dept / archive（归档视图是标签二内的子视图，单独一份给 verifier）
    let dumps: [(Tab, bool); 3] = [(Tab::Proj, false), (Tab::Dept, false), (Tab::Dept, true)];
    for (tab, arc) in dumps {
        let mut ui = UiLocal {
            tab: tab.key().to_string(),
            y: if d.cal.y != 0 { d.cal.y } else { 2026 },
            mo: if d.cal.mo > 0 {
                d.cal.mo as u32
            } else {
                1
            },
            sel_day: 0,
            scroll: 0,
            locked: false,
            pending: None,
            arc_mode: arc,
            arc_scroll: 0,
            arc_pending: None,
            newtodo: view::NewTodo::default(),
            modal: view::ModalForm::default(),
            set: view::SetForm::default(),
            mouse: (-1, -1),
            set_dragging: false,
            hover_ms: 0,
            row_drag: None,
            drag_to: None,
            drag_u: None,
            em_tip: false,
            ime_pending: Vec::new(),
            last_ch: '\0',
            last_at: std::time::Instant::now(),
        };
        ui.tab = tab.key().to_string();
        // 离线演示态（出证据图用，不影响真实交互）
        if !arc && tab == Tab::Dept {
            match cfg.demo.as_str() {
                "confirm" => {
                    ui.pending = Some(("dept:0:del".to_string(), Instant::now() + DEL_CONFIRM));
                }
                "newtodo" => {
                    ui.newtodo.open(&d.today);
                    ui.newtodo.text = "跟客户确认 ITEM_X 投模时间".into();
                }
                "newtodo-date" => {
                    ui.newtodo.open(&d.today);
                    ui.newtodo.text = "跟客户确认 ITEM_X 投模时间".into();
                    ui.newtodo.menu_open = true;
                    ui.newtodo.menu_sel = 1;
                }
                "settings" => {
                    layout::TINT.store(
                        ((view::TINTS[1].1 .0 as u32) << 16)
                            | ((view::TINTS[1].1 .1 as u32) << 8)
                            | view::TINTS[1].1 .2 as u32,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                    ui.set.open = true;
                    ui.set.opacity = 0.70;
                    ui.set.tint = 1;
                    ui.set.size = 2;
                }
                "modal" | "modalmenu" => {
                    ui.modal.open(&d.today);
                    ui.modal.text = "跟客户确认 ITEM_X 投模时间".into();
                    ui.modal.prio = "P1".into();
                    ui.modal.note = "## 本周重点\n- 带上改模报价\n- 确认 35ML 双仓排产\n普通备注一行".into();
                    ui.modal.set_due(2, &d.today);
                    if cfg.demo == "modalmenu" {
                        ui.modal.due_menu = true;
                    }
                }
                _ => {}
            }
        }
        let label = if arc { "archive" } else { tab.key() };
        let v = build_view(d, st, &ui);
        let mut hits = Hits::new();
        cv.clear();
        layout::render(&mut cv, &mut te, &v, &mut hits);
        lg.line(&format!("[dump] tab={} {}", label, hits.summary()));
        if let Some(path) = &cfg.snap {
            // 两个标签各一张：x.png -> x.proj.png / x.dept.png
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "snap".into());
            let p = path.with_file_name(format!("{}.{}.png", stem, label));
            write_png(&p, &cv);
            lg.line(&format!("[dump] snap tab={} -> {}", label, p.display()));
        }
        per_tab.push((label.to_string(), tab_json(label, &hits)));
    }

    let Some(base) = cfg.dump_hit.as_ref() else {
        lg.line("[dump] 未提供 --dump-hit 输出路径，跳过命中区导出");
        return;
    };
    write_json(base, &format!("{{\"tabs\":[{}]}}", join_json(&per_tab)));
    let stem = base
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "hit_regions".to_string());
    let dir = base.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    for (tab, json) in &per_tab {
        let p = dir.join(format!("{}_{}.json", stem, tab));
        write_json(&p, json);
        lg.line(&format!("[dump] wrote {} ({} bytes)", p.display(), json.len()));
    }
    lg.line(&format!(
        "[dump] wrote {} (combined, {} tabs)",
        base.display(),
        per_tab.len()
    ));
}

fn join_json(items: &[(String, String)]) -> String {
    let mut s = String::new();
    for (i, (_, j)) in items.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(j);
    }
    s
}

fn write_json(path: &std::path::Path, body: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(path, body) {
        eprintln!("write {} failed: {}", path.display(), e);
    }
}

fn write_png(path: &std::path::Path, cv: &canvas::Canvas) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut rgba = vec![0u8; (cv.w * cv.h * 4) as usize];
    for i in 0..cv.px.len() {
        let p = cv.px[i];
        let a = (p >> 24) & 0xFF;
        let (r, g, b) = ((p >> 16) & 0xFF, (p >> 8) & 0xFF, p & 0xFF);
        let f = |c: u32| -> u8 {
            if a == 0 {
                0
            } else {
                (c * 255 + a / 2).checked_div(a).unwrap_or(0).min(255) as u8
            }
        };
        rgba[i * 4] = f(r);
        rgba[i * 4 + 1] = f(g);
        rgba[i * 4 + 2] = f(b);
        rgba[i * 4 + 3] = a as u8;
    }
    let _ = png::write_png(&path.to_string_lossy(), cv.w as u32, cv.h as u32, &rgba);
}

// ───────────────────────── headless ─────────────────────────


/// 画布级确定性自检：Full -> flip(+1) Right -> flip(-1) Right，比较像素哈希。
/// 与窗口/桌面无关，用来区分「渲染不确定」和「截屏受桌面干扰」。
fn selftest_flip(cfg: &Config, d: &data::WidgetData, st: &state::AppState) {
    let mut te = text::TextEngine::new();
    layout::set_opaque(cfg.opaque);
    let mut cv = canvas::Canvas::new(WIN_W, WIN_H);
    let mut base = canvas::Canvas::new(WIN_W, WIN_H);
    let mut hits = hit::Hits::new();
    let mut ui = UiLocal {
        tab: st.ui.tab.clone(),
        y: if d.cal.y != 0 { d.cal.y } else { 2026 },
        mo: if d.cal.mo > 0 { d.cal.mo as u32 } else { 1 },
        sel_day: 0,
        scroll: 0,
        locked: false,
        pending: None,
        arc_mode: false,
        arc_scroll: 0,
        arc_pending: None,
            newtodo: view::NewTodo::default(),
            modal: view::ModalForm::default(),
            set: view::SetForm::default(),
            mouse: (-1, -1),
            set_dragging: false,
            hover_ms: 0,
            row_drag: None,
            drag_to: None,
            drag_u: None,
            em_tip: false,
            ime_pending: Vec::new(),
            last_ch: '\0',
            last_at: std::time::Instant::now(),
    };
    let v = build_view(d, st, &ui);
    layout::render_chrome(&mut base, &mut te, &v, &mut hits);
    cv.copy_rect_from(&base, 0, 0, WIN_W, WIN_H);
    layout::render_tabs(&mut cv, &mut te, &v, &mut hits);
    layout::render_left(&mut cv, &mut te, &v, &mut hits);
    layout::render_right(&mut cv, &mut te, &v, &mut hits);
    let h0 = canvas_hash(&cv);
    let flip_then = |ui: &mut UiLocal, cv: &mut canvas::Canvas, te: &mut text::TextEngine, hits: &mut hit::Hits, base: &canvas::Canvas, delta: i32| -> u64 {
        ui.mo = if delta > 0 { ui.mo % 12 + 1 } else { if ui.mo == 1 { 12 } else { ui.mo - 1 } };
        let v = build_view(d, st, ui);
        let (x, y, w, h) = layout::right_rect();
        cv.copy_rect_from(base, x, y, w, h);
        hits.v.retain(|it| !(it.x >= x && it.y >= y && it.x + it.w <= x + w && it.y + it.h <= y + h));
        layout::render_right(cv, te, &v, hits);
        canvas_hash(cv)
    };
    let h1 = flip_then(&mut ui, &mut cv, &mut te, &mut hits, &base, 1);
    let h2 = flip_then(&mut ui, &mut cv, &mut te, &mut hits, &base, -1);
    let ok = h0 != h1 && h0 == h2;
    println!("SELFTEST_FLIP {} start={:012X} after1={:012X} end={:012X} {}",
        if ok { "PASS" } else { "FAIL" }, h0, h1, h2,
        if ok { "" } else { "(after1!=start && end==start)" });
}

fn canvas_hash(cv: &canvas::Canvas) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for px in cv.px.iter() {
        h ^= *px as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn headless(cfg: Config, d: data::WidgetData, st: state::AppState, lg: Logger) {
    layout::set_opaque(cfg.opaque);
    let reload = cfg.reload_secs as u64;
    let total = cfg.seconds.unwrap_or(reload + 5);
    let started = Instant::now();
    let mut tick = 0u64;
    let mut last_mtime = data::mtime(&cfg.data);
    let mut data = d;
    lg.line(&format!("[headless] initial {}", data.summary()));
    lg.line(&format!("[headless] run_seconds={} reload_secs={}", total, reload));
    if std::env::var_os("RW_FLIP_TEST").is_some() {
        selftest_flip(&cfg, &data, &st);
    }

    while started.elapsed() < Duration::from_secs(total) {
        std::thread::sleep(Duration::from_millis(250));
        tick += 1;
        if !tick.is_multiple_of(reload * 4) {
            continue;
        }
        let cur = data::mtime(&cfg.data);
        if cur != last_mtime {
            last_mtime = cur;
            let (next, ok) = data::load(&cfg.data, &lg);
            data = next;
            lg.line(&format!("[data] reload ok={} {}", ok, data.summary()));
        } else {
            lg.line("[data] unchanged");
        }
        lg.line(&format!(
            "[headless] alive pid={} state_done={}",
            std::process::id(),
            st.done_count()
        ));
    }
    lg.line("[headless] exit 0");
}

// ───────────────────────── 窗口 ─────────────────────────

fn run_gui(cfg: Config, d: data::WidgetData, st: state::AppState, lg: Logger) {
    layout::set_opaque(cfg.opaque);
    load_meet(&cfg, &lg);
    let mode = if !cfg.mode.is_empty() {
        Mode::from_key(&cfg.mode)
    } else {
        Mode::from_key(&st.ui.win_mode)
    };
    let ui = UiLocal {
        tab: if st.ui.tab == "dept" {
            "dept".into()
        } else if st.ui.tab == "meet" {
            "meet".into()
        } else {
            "proj".into()
        },
        y: if d.cal.y != 0 { d.cal.y } else { 2026 },
        mo: if d.cal.mo > 0 { d.cal.mo as u32 } else { 1 },
        sel_day: 0,
        scroll: 0,
        locked: false,
        pending: None,
        arc_mode: false,
        arc_scroll: 0,
        arc_pending: None,
            newtodo: view::NewTodo::default(),
            modal: view::ModalForm::default(),
            set: view::SetForm::default(),
            mouse: (-1, -1),
            set_dragging: false,
            hover_ms: 0,
            row_drag: None,
            drag_to: None,
            drag_u: None,
            em_tip: false,
            ime_pending: Vec::new(),
            last_ch: '\0',
            last_at: std::time::Instant::now(),
    };
    let last_state = data::mtime(&cfg.state);
    let t_start = Instant::now();
    let puller = pull::Puller::new(&cfg, &lg);

    let init_w = if st.ui.win_w > 0 { st.ui.win_w.max(WIN_W) } else { WIN_W };
    let init_h = if st.ui.win_h > 0 { st.ui.win_h.max(WIN_H) } else { WIN_H };
    layout::set_box(init_w, init_h);
    let (sc_x, sc_y, off_x, off_y) = (1.0f32, 1.0f32, 0.0f32, 0.0f32);
    let mut canvas = {
        let mut cv = canvas::Canvas::new(init_w, init_h);
        cv.sx = sc_x;
        cv.sy = sc_y;
        cv.ox = off_x;
        cv.oy = off_y;
        cv
    };
    let mut te = text::TextEngine::new();
    layout::set_opaque(cfg.opaque);
    let hits = Hits::new();
    let init_tint = parse_hex_rgb(&st.ui.tint).unwrap_or((18, 20, 26));
    let init_locked = st.ui.locked;
    {
        // 首帧：整屏绘制（背景 + 左卡 + 右卡）
        let v = build_view(&d, &st, &ui);
        let mut h = hits;
        layout::render(&mut canvas, &mut te, &v, &mut h);
        drop(v);
        let ctx = Ctx {
            cfg,
            data: d,
            st,
            log: lg,
            last_mtime: None,
            last_state_mtime: last_state,
            ticks: 0,
            ui,
            mode,
            canvas,
            base: {
                let mut b = canvas::Canvas::new(init_w, init_h);
                b.sx = sc_x;
                b.sy = sc_y;
                b.ox = off_x;
                b.oy = off_y;
                b
            },
            base_hits: Hits { v: Vec::new() },
            base_valid: false,
            te,
            hits: h,
            pres: Present::new(init_w, init_h),
            hwnd: 0,
            drag: None,
            t_start,
            pull: puller,
        };
        let _ = CTX.set(Mutex::new(ctx));
    }
    MODE_ATOMIC.store(match mode {
        Mode::Embed => 0,
        Mode::Normal => 1,
        Mode::Topmost => 2,
    }, Ordering::Relaxed);

    layout::TINT.store(
        ((init_tint.0 as u32) << 16) | ((init_tint.1 as u32) << 8) | init_tint.2 as u32,
        Ordering::Relaxed,
    );
    layout::LOCKED.store(init_locked, Ordering::Relaxed);
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = GetLastError();
        let hmodule = GetModuleHandleW(null_mut());
        if hmodule.is_null() {
            return;
        }
        let class_name = wide(CLASS_NAME);
        let title = wide(TITLE);
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(wndproc);
        wc.hInstance = hmodule;
        wc.lpszClassName = class_name.as_ptr();
        if RegisterClassW(&wc) == 0 {
            return;
        }
        // 输入框弹窗（新增待办）
        let edit_class = wide(EDIT_CLASS);
        let mut ec: WNDCLASSW = std::mem::zeroed();
        ec.lpfnWndProc = Some(editor_wndproc);
        ec.hInstance = hmodule;
        ec.lpszClassName = edit_class.as_ptr();
        let _ = RegisterClassW(&ec);

        let ex_style = WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
        let (x, y) = initial_position();
        let hwnd = CreateWindowExW(
            ex_style,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            x,
            y,
            init_w,
            init_h,
            null_mut(),
            null_mut(),
            hmodule,
            std::ptr::null(),
        );
MAIN_HWND.store(hwnd as isize, Ordering::Relaxed);
        if hwnd.is_null() {
            with_ctx(|c| c.log.line(&format!(
                "[gui] CreateWindowExW failed err={} ex=0x{:x} size={}x{} class_registered={}",
                GetLastError(),
                ex_style,
                init_w,
                init_h,
                true
            )));
            return;
        }
        with_ctx(|c| c.hwnd = hwnd as isize);
        present_frame(hwnd);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        apply_mode(hwnd, mode);
        // 托盘：内嵌图标资源 → exe 同目录 ico。失败不影响主窗口。
        let icon = tray::load_icon(crate::config::exe_dir().as_deref());
        let mut installed = false;
        with_ctx(|c| installed = tray::install(hwnd, icon, &c.log));
        if installed {
            TRAY_ON.store(1, Ordering::Relaxed);
            TRAY_ICON.store(icon as isize, Ordering::Relaxed);
        } else {
            tray::destroy_icon(icon);
            // 托盘加不上：本机 shell 是 High 完整性、本进程 Medium 时会 err=5。
            // 只在失败路径做一次自提升重试（--elevated 防循环），子进程接管显示后本进程退出。
            if elevate::should_retry() {
                let mut lg_slot: Option<log::Logger> = None;
                with_ctx(|c| lg_slot = Some(c.log.clone()));
                let Some(lg) = lg_slot else {
                    return;   // 拿不到 logger 就别崩在托盘失败路径上
                };
                if elevate::relaunch("tray-add-failed", &lg) {
                    return;
                }
            }
        }
        // 预热菜单窗口：右击托盘时才建窗口一旦失败就只剩一行日志，先在启动期试一次。
        {
        }
        with_ctx(|c| c.log.line(&format!("[startup] autostart {}", startup::describe())));
        log_mem("first-frame");
        let ms = with_ctx_return(|c| c.t_start.elapsed().as_millis() as u64);
        with_ctx(|c| {
            c.log.line(&format!(
                "[gui] shown hwnd=0x{:X} pos={},{} size={}x{} mode={} first_frame_ms={}",
                hwnd as usize,
                x,
                y,
                WIN_W,
                WIN_H,
                c.mode.key(),
                ms
            ));
            c.log.line(&format!("[hit] {}", c.hits.summary()));
        });
        SetTimer(hwnd, TIMER_ID, TICK_MS, None);
    // 备注框光标闪烁（500ms 翻转；弹窗没开时 WM_TIMER 直接返回）
    SetTimer(hwnd, 0x77, 500, None);
        // A07 的进程内等价物：真实屏幕像素（含窗口层/合成），不是画布哈希。
        if std::env::var_os("RW_SCREEN_TEST").is_some() {
            selftest_screen(hwnd);
        }

        let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// 主窗口句柄（9.11：主窗口句柄在创建时写入 MAIN_HWND，供定时器/点击回调复用）
static MAIN_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
fn win_hwnd() -> HWND {
    MAIN_HWND.load(Ordering::Relaxed) as HWND
}

static REMIND: std::sync::Mutex<Option<crate::view::Remind>> = std::sync::Mutex::new(None);
// (key, 触发秒)；10 分钟内有效，超时清掉，防长期运行无界增长
static REMINDED: std::sync::Mutex<Vec<(String, i64)>> = std::sync::Mutex::new(Vec::new());
const REMINDED_TTL_SEC: i64 = 600;
static POISON_HITS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static REMIND_AT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
const WM_MEET_REDRAW: u32 = 0x8002;
thread_local! {
    /// 会议安排缓存（None = 还没读到 / 文件不存在）
    static MEET: std::cell::RefCell<Option<meet::Meet>> = const { std::cell::RefCell::new(None) };
}

/// 只保留未开始的会议，并按 (日期, 开始时间) 升序
fn meet_future_only(rows: Vec<view::MeetRow>) -> Vec<view::MeetRow> {
    let ns = log::now_str();
    let now_key = format!("{} {}", &ns[..10], &ns[11..16]);
    let mut out: Vec<view::MeetRow> = rows
        .into_iter()
        .filter(|r| {
            if r.date.len() < 10 || r.start.len() < 5 {
                return true;
            }
            format!("{} {}", &r.date[..10], &r.start[..5]) > now_key
        })
        .collect();
    out.sort_by(|a, b| (a.date.clone(), a.start.clone()).cmp(&(b.date.clone(), b.start.clone())));
    out
}

/// 读 `<data 同目录>/meeting_data.json`；失败只记日志，不影响挂件
fn load_meet(cfg: &Config, lg: &Logger) {
    let p = meet::path_for(&cfg.data);
    match meet::load(&p) {
        Some(md) => {
            lg.line(&format!(
                "[meet] loaded at={} today={} week={} month={}",
                md.at,
                md.today.len(),
                md.week.len(),
                md.month.len()
            ));
            MEET.with(|m| {
                let span = m.borrow().as_ref().map(|x| x.span).unwrap_or(0);
                let mut md2 = md;
                md2.span = span;
                *m.borrow_mut() = Some(md2);
            });
        }
        None => lg.line(&format!("[meet] skip path={}", p.display())),
    }
}


/// 开会前 5 分钟：弹出独立置顶提醒窗（点「知道了」才关）。每秒 tick 调用一次。
fn check_remind(hwnd: HWND) {
    let ns = log::now_str();
    let date = ns[0..10].to_string();
    let mins = ns[11..13].parse::<i32>().unwrap_or(0) * 60 + ns[14..16].parse::<i32>().unwrap_or(0);
    // 弹窗关掉后自动清掉提醒态（取消 90 秒自动收起）
    if REMIND.lock().map(|g| g.is_some()).unwrap_or(false) && !popup::alive() {
        if let Ok(mut g) = REMIND.lock() { *g = None; }
        unsafe {
            windows_sys::Win32::Graphics::Gdi::InvalidateRect(hwnd, std::ptr::null(), 0);
        }
    }
    if REMIND.lock().map(|g| g.is_some()).unwrap_or(false) {
        return;
    }

    let mut fired: Option<view::Remind> = None;
    MEET.with(|m| {
        if let Some(md) = m.borrow().as_ref() {
            for r in md.today.iter() {
                if r.date != date || r.start.len() < 5 {
                    continue;
                }
                let sm = r.start[0..2].parse::<i32>().unwrap_or(-1) * 60
                    + r.start[3..5].parse::<i32>().unwrap_or(0);
                let left = sm - mins;
                if (0..=5).contains(&left) && left > 0 {
                    let key = format!("{}|{}|{}", r.date, r.start, r.name);
                    let now_s = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    let dup = {
                        let mut d = REMINDED.lock().unwrap_or_else(|e| e.into_inner());
                        d.retain(|(_, t)| now_s - *t < REMINDED_TTL_SEC);
                        d.iter().any(|(k, _)| k == &key)
                    };
                    if !dup {
                        REMINDED
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push((key, now_s));
                        fired = Some(view::Remind {
                            time: r.start.clone(),
                            name: r.name.clone(),
                            place: r.place.clone(),
                            people: r.people.clone(),
                            left,
                        });
                    }
                }
            }
        }
    });

    if let Some(rem) = fired {
        if let Ok(mut g) = REMIND.lock() { *g = Some(rem.clone()); }
        REMIND_AT.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            Ordering::Relaxed,
        );
        popup::show(popup::Body {
            time: rem.time.clone(),
            name: rem.name.clone(),
            place: rem.place.clone(),
            people: rem.people.clone(),
            left: rem.left,
        });
        // 只让弹窗前台：主挂件保持普通层级，不动 ShowWindow/TOPMOST/Foreground
        with_ctx(|c| {
            c.log.line(&format!(
                "[remind] fire {} min-left {} name={}",
                rem.left, rem.time, rem.name
            ))
        });
        redraw(Draw::Full);
    }
}

/// 取状态锁：中毒时继续用内部状态（而不是静默不执行），首次发生记一条 stderr。
fn lock_ctx<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(e) => {
            let n = POISON_HITS.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            if n == 1 {
                eprintln!("[ctx] 状态锁曾中毒，继续使用内部状态（累计 {n} 次）");
            }
            e.into_inner()
        }
    }
}

fn with_ctx<F: FnOnce(&mut Ctx)>(f: F) {
    if let Some(m) = CTX.get() {
        let mut c = lock_ctx(m);
        f(&mut c);
    }
}

fn with_ctx_return<T, F: FnOnce(&mut Ctx) -> T>(f: F) -> T
where
    T: Default,
{
    if let Some(m) = CTX.get() {
        let mut c = lock_ctx(m);
        return f(&mut c);
    }
    T::default()
}

fn work_area() -> (i32, i32, i32, i32) {
    unsafe {
        let mut r: RECT = std::mem::zeroed();
        let ok = SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut r as *mut RECT as *mut c_void, 0);
        if ok != 0 && r.right > r.left && r.bottom > r.top {
            (r.left, r.top, r.right, r.bottom)
        } else {
            (
                0,
                0,
                GetSystemMetrics(SM_CXSCREEN),
                GetSystemMetrics(SM_CYSCREEN),
            )
        }
    }
}

/// 找到承载壁纸的 WorkerW（0x052C 让 Progman 生成真正的壁纸层 WorkerW，再取面积最大的那个）。
fn initial_position() -> (i32, i32) {
    let (left, top, right, bottom) = work_area();
    // 位置优先级：上次保存的位置（ui.x/ui.y，非 0 才算）> 命令行/配置（cfg.x/cfg.y）> 默认左下角
    let (ux, uy, cx, cy) = with_ctx_return(|c| (c.st.ui.x, c.st.ui.y, c.cfg.x, c.cfg.y));
    let x = ux.or(cx).unwrap_or(left + 24);
    let y = uy.or(cy).unwrap_or(bottom - WIN_H);
    (
        // 不允许跑到屏幕外：左下角对齐时 x 可能为负，钳到 0 起。
        x.clamp(left, (right - 120).max(left)),
        y.clamp(top, (bottom - 120).max(top)),
    )
}

/// 抓窗口矩形范围内的真实屏幕像素（含桌面合成层），返回 FNV-1a 哈希。
/// 与画布哈希不同：它把窗口层的分层合成、置顶顺序、遮罩都算进去，
/// 是 A07（外部 CopyFromScreen 往返哈希）的进程内等价物。
unsafe fn screen_hash(hwnd: HWND) -> u64 {
    let mut r: RECT = std::mem::zeroed();
    if GetWindowRect(hwnd, &mut r) == 0 {
        return 0;
    }
    let w = r.right - r.left;
    let h = r.bottom - r.top;
    if w <= 0 || h <= 0 {
        return 0;
    }
    let sdc = GetDC(std::ptr::null_mut());
    if sdc.is_null() {
        return 0;
    }
    let mdc = CreateCompatibleDC(sdc);
    let bmp = CreateCompatibleBitmap(sdc, w, h);
    let old = SelectObject(mdc, bmp as HGDIOBJ);
    let ok = BitBlt(mdc, 0, 0, w, h, sdc, r.left, r.top, SRCCOPY);
    let mut bi: BITMAPINFO = std::mem::zeroed();
    bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bi.bmiHeader.biWidth = w;
    bi.bmiHeader.biHeight = -h;
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = BI_RGB;
    let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
    let lines = if ok != 0 {
        GetDIBits(
            mdc,
            bmp,
            0,
            h as u32,
            buf.as_mut_ptr() as *mut core::ffi::c_void,
            &mut bi,
            DIB_RGB_COLORS,
        )
    } else {
        0
    };
    SelectObject(mdc, old);
    let _ = DeleteObject(bmp as HGDIOBJ);
    let _ = DeleteDC(mdc);
    ReleaseDC(std::ptr::null_mut(), sdc);
    if lines == 0 {
        return 0;
    }
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in buf.iter() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// `RW_SCREEN_TEST=1`：置顶 → 截屏 h0 → 翻月 → 截屏 a1 → 翻回 → 截屏 h2，
/// 打印 `SCREEN_TEST {PASS|FAIL}`。PASS 判据与 A07 一致：`a1 != h0 && h2 == h0`。
unsafe fn selftest_screen(hwnd: HWND) {
    SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
    );
    std::thread::sleep(Duration::from_millis(500));
    let h0 = screen_hash(hwnd);
    with_ctx(|c| flip(c, 1));
    redraw(Draw::Right);
    std::thread::sleep(Duration::from_millis(400));
    let a1 = screen_hash(hwnd);
    with_ctx(|c| flip(c, -1));
    redraw(Draw::Right);
    std::thread::sleep(Duration::from_millis(400));
    let h2 = screen_hash(hwnd);
    let pass = a1 != h0 && h2 == h0;
    // 第二轮：完整复刻 A07 的 20 次交替翻月（10 对），验证窗口层往返无累积漂移。
    for _ in 0..10 {
        with_ctx(|c| flip(c, 1));
        redraw(Draw::Right);
        with_ctx(|c| flip(c, -1));
        redraw(Draw::Right);
    }
    std::thread::sleep(Duration::from_millis(600));
    let h20 = screen_hash(hwnd);
    let pass20 = h20 == h0;
    with_ctx(|c| {
        c.log.line(&format!(
            "SCREEN_TEST {} start={:X} after1={:X} end={:X}",
            if pass { "PASS" } else { "FAIL" },
            h0,
            a1,
            h2
        ));
        c.log.line(&format!(
            "SCREEN_TEST20 {} start={:X} after20={:X}",
            if pass20 { "PASS" } else { "FAIL" },
            h0,
            h20
        ));
    });
    SetWindowPos(
        hwnd,
        HWND_NOTOPMOST,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
    );
}

/// 按逻辑尺寸设置窗口客户区（present 时 UpdateLayeredWindow 会缩放到客户区大小）
fn present_frame(hwnd: HWND) {
    let k = with_ctx_return(|c| {
        if c.ui.set.open {
            c.ui.set.opacity
        } else {
            c.st.ui.opacity
        }
    });
    with_ctx(|c| unsafe {
        c.pres.present(hwnd, &c.canvas, k as f32);
    });
}

/// 三档窗口层。embed = GWL_HWNDPARENT 设 Progman 后压到 HWND_BOTTOM，才真正贴桌面层。
unsafe fn apply_mode(hwnd: HWND, mode: Mode) {
    MODE_ATOMIC.store(
        match mode {
            Mode::Embed => 0,
            Mode::Normal => 1,
            Mode::Topmost => 2,
        },
        Ordering::Relaxed,
    );
    match mode {
        Mode::Embed => {
            // Progman 是桌面图标层宿主；某些会话（远程/会话隔离）里窗口站看不到它，
            // 这时退回 GetShellWindow()（同样返回 Shell 的桌面窗口），保证仍然贴在桌面层。
            let mut parent = FindWindowW(wide("Progman").as_ptr(), std::ptr::null());
            let mut which = "Progman";
            if parent.is_null() {
                parent = GetShellWindow();
                which = "ShellWindow";
            }
            if !parent.is_null() {
                SetLastError(0);
                let prev = SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, parent as isize);
                let err = GetLastError();
                let back = GetWindowLongPtrW(hwnd, GWLP_HWNDPARENT);
                // 某些会话里跨进程设 GWLP_HWNDPARENT 会被系统拒（ERROR_INVALID_PARAMETER=87），
                // 这时退回 SetParent：窗口变成 Progman 的子窗口，同样贴在壁纸与图标层之间。
                let mut via_setparent = false;
                let mut via_longw = 0i64;
                if back == 0 {
                    // 32 位入口：句柄本身是 32 位值，某些环境下 longptr 入口会被拒
                    SetLastError(0);
                    via_longw = SetWindowLongW(hwnd, GWL_HWNDPARENT, parent as i32) as i64;
                }
                if back == 0 && via_longw == 0 {
                    SetLastError(0);
                    let ok = SetParent(hwnd, parent);
                    if ok.is_null() {
                        with_ctx(|c| {
                            c.log.line(&format!(
                                "[embed] SetParent failed err={} (keeping HWND_BOTTOM)",
                                GetLastError()
                            ))
                        });
                    } else {
                        via_setparent = true;
                    }
                }
                with_ctx(|c| {
                    c.log.line(&format!(
                        "[embed] parent=0x{:X} via={} prev=0x{:X} err={} readback=0x{:X} setparent={} longw=0x{:X}",
                        parent as usize, which, prev as usize, err, back as usize, via_setparent, via_longw as usize
                    ))
                });
            } else {
                with_ctx(|c| c.log.line("[embed] no desktop parent found; bottom only"));
            }
            // 挂在 Progman 下后要抬到该层顶部，否则被桌面图标层(SysListView32)盖住
            SetWindowPos(
                hwnd,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        Mode::Normal => {
            // 贴壁纸层：Win10/11 的壁纸画在一个「没有 SHELLDLL_DefView 子窗口的 WorkerW」里，
            // 把窗口 SetParent 到它 → 在壁纸之上、桌面图标与普通窗口之下。
            // 直接挂 Progman 会被那层 WorkerW 盖住（此前「看不见」的真因）。
            // 不做桌面子窗口：本机 Progman 的 DefView 会盖住子窗口、真正的壁纸层 WorkerW 又不存在。
            // 改为普通窗口压到 z 序底部 —— 桌面空着时看得见，被窗口盖住时自然让位（符合「不要放最前面」）。
            with_ctx(|c| {
                c.log.line("[float] bottom dock (top-level, HWND_BOTTOM)");
            });
            SetWindowPos(
                hwnd,
                HWND_BOTTOM,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        Mode::Topmost => {
            SetParent(hwnd, null_mut());
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
}

// ───────────────────────── 重绘路径 ─────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Draw {
    /// 整屏（首帧、数据热重载）
    Full,
    /// 只重画左卡内容区（切标签、勾选、删除、二次确认）
    Left,
    /// 只重画右卡内容区（翻月、选日）
    Right,
}

/// 等比缩放并把设计尺寸（980×566）居中放进窗口，避免非等比拉伸把字压扁。
/// 返回 (sx, sy, ox, oy)：sx==sy==s，调用方按像素放缩。
fn redraw(kind: Draw) {
    let hwnd = with_ctx_return(|c| c.hwnd) as HWND;
    if hwnd.is_null() {
        return;
    }
    let hwnd = hwnd as HWND;
    let t0 = std::time::Instant::now();
    with_ctx(|c| {
        let Ctx {
            data,
            st,
            ui,
            canvas,
            base,
            base_valid,
            base_hits,
            te,
            hits,
            pres,
            ..
        } = c;
        let v = build_view(data, st, ui);
        // 模态窗是整窗遮罩 + 最高 z 序，任何脏区重画都必须整屏重来
        let kind = if v.modal.open { Draw::Full } else { kind };
        // 静止层还没建（首帧走的是直接 render、或 base_valid 刚被数据刷新置 false）时，
        // 任何脏区重画都必须先升格为整屏，否则 copy_rect_from 会把全零的 base 贴进画布
        // —— 表现就是「翻一次月右卡变透明、桌面透出」，A07 截图哈希也因此对不上。
        let kind = if matches!(kind, Draw::Full) || *base_valid {
            kind
        } else {
            Draw::Full
        };
        match kind {
            Draw::Full => {
                // 1) 静止层：面板 SDF 很贵（980x566 逐像素），只在数据/字号变化时重建
                if !*base_valid {
                    let mut chrome = Hits { v: Vec::new() };
                    base.clear();
                    layout::render_chrome(base, te, &v, &mut chrome);
                    *base_hits = chrome;
                    *base_valid = true;
                }
                // 2) base -> canvas 整块 blit，命中区随之重置
                canvas.copy_rect_from(base, 0, 0, canvas.w, canvas.h);
                hits.v.clear();
                hits.v.extend(base_hits.v.iter().cloned());
                // 3) 动态层：标签胶囊 + 左卡内容 + 右卡内容
                layout::render_tabs(canvas, te, &v, hits);
                layout::render_left(canvas, te, &v, hits);
                layout::render_right(canvas, te, &v, hits);
                // 3.5) 附件悬停预览：内容之上、模态窗之下
                if !v.modal.open {
                    layout::draw_attachment_hover(canvas, te, &v, hits);
    layout::draw_note_hover(canvas, te, &v, hits);
                }
                // 4) 模态窗：遮罩 + 面板，z 序最高
                if v.modal.open {
                    layout::draw_modal(canvas, te, &v, hits);
                }
                layout::draw_settings(canvas, te, &v, hits);
            }
            Draw::Left => {
                let (x, y, w, h) = layout::left_rect();
                canvas.copy_rect_from(base, x, y, w, h);
                layout::render_left(canvas, te, &v, hits);
            }
            Draw::Right => {
                let (x, y, w, h) = layout::right_rect();
                canvas.copy_rect_from(base, x, y, w, h);
                layout::render_right(canvas, te, &v, hits);
            }
        }
        let k = if ui.set.open { ui.set.opacity } else { st.ui.opacity };
        unsafe {
            pres.present(hwnd, canvas, k as f32);
        }
        if std::env::var_os("RUST_WIDGET_TRACE").is_some() {
            let mut low = 0usize;
            let mut sum_a = 0u64;
            for px in canvas.px.iter() {
                let a = (px >> 24) & 0xff;
                if a < 250 { low += 1; }
                sum_a += a as u64;
            }
            let n = canvas.px.len() as u64;
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            c.log.line(&format!(
                "[alpha] avg={:.1} low={} / {}",
                sum_a as f64 / n as f64, low, n
            ));
            let kn = match kind { Draw::Full => "full", Draw::Left => "left", Draw::Right => "right" };
            c.log.line(&format!("[redraw] {} {:.2}ms", kn, ms));
        }
    });
}

/// 只更新内存状态 + 保存，不重绘（重绘由调用方选脏区）。
fn save_state() {
    with_ctx(|c| {
        c.st.ui.tab = c.ui.tab.clone();
        c.st.ui.win_mode = match c.mode {
            Mode::Embed => "embed".into(),
            _ => "float".into(),
        };
        state::save(&c.cfg.state, &c.st, &c.log);
        c.last_state_mtime = data::mtime(&c.cfg.state);
    });
}

// ───────────────────────── 命中 → 动作 ─────────────────────────

fn hit_at(x: i32, y: i32) -> Option<Hit> {
    with_ctx_return(|c| c.hits.hit(x, y).cloned())
}

/// 行内输入要键盘：临时摘掉 WS_EX_NOACTIVATE 并抢焦点；退出输入时还回去。
/// （widget 常态是不激活窗口，所以只在输入期间变成可激活。）
unsafe fn set_keyboard(on: bool) {
    let hwnd = with_ctx_return(|c| c.hwnd) as HWND;
    if hwnd.is_null() {
        return;
    }
    let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if on {
        if ex & WS_EX_NOACTIVATE != 0 {
            SetWindowLongW(hwnd, GWL_EXSTYLE, (ex & !WS_EX_NOACTIVATE) as i32);
        }
        SetForegroundWindow(hwnd);
        SetFocusW(hwnd);
    } else if ex & WS_EX_NOACTIVATE == 0 {
        SetWindowLongW(hwnd, GWL_EXSTYLE, (ex | WS_EX_NOACTIVATE) as i32);
    }
}

/// 回车提交新建待办：空行不创建。只改内存，落盘由调用方在锁外做。
fn commit_newtodo(c: &mut Ctx) -> bool {
    let text = c.ui.newtodo.text.trim().to_string();
    if text.is_empty() {
        c.log.line("[input] enter on empty -> no create");
        return false;
    }
    let due = c.ui.newtodo.due.clone();
    if !c.ui.newtodo.commit() {
        return false;
    }
    c.st.add_custom(&text, &due);
    c.log.line(&format!("[input] add custom {} due={}", text, due));
    c.ui.newtodo.cancel();
    true
}

// ───────────────────────── 新建待办模态窗 ─────────────────────────

/// 模态窗保存：空正文只记日志、不关闭。true = 已落盘（调用方负责 save_state + 整屏重绘）。
fn modal_save(c: &mut Ctx) -> bool {
    let text = c.ui.modal.text.trim().to_string();
    if text.is_empty() {
        // 有可见反馈：红字提示 + 保持弹窗 + 光标回到事项框（否则用户只看到「点了没反应」）
        c.ui.modal.err = "请先填写事项".to_string();
        c.ui.modal.field = 0;
        c.log.line("[modal] save blocked empty (hint shown)");
        return false;
    }
    c.ui.modal.err.clear();
    if !c.ui.modal.commit_custom() {
        c.log.line("[modal] due invalid (want YYYY-MM-DD or empty)");
        return false;
    }
    let due = c.ui.modal.due.clone();
    let prio = c.ui.modal.prio.clone();
    let note = c.ui.modal.note.trim().to_string();
    let img = c.ui.modal.img.clone();
    if !c.ui.modal.edit_id.is_empty() {
        let eid = c.ui.modal.edit_id.clone();
        let mut hit = false;
        if let Some(it) = c.st.custom.iter_mut().find(|x| x.id == eid) {
            it.text = text.clone();
            it.due = due.clone();
            it.prio = prio.clone();
            it.note = note.clone();
            it.img = img.clone();
            hit = true;
        }
        if !hit {
            if let Some(it) = c.st.department.iter_mut().find(|x| x.id == eid) {
                it.text = text.clone();
                it.due = if due.is_empty() { None } else { Some(due.clone()) };
                it.prio = prio.clone();
                it.note = note.clone();
                hit = true;
            }
        }
        c.log.line(&format!(
            "[modal] edit ok id={} text={} prio={} due={} img={} hit={}",
            eid, text, prio, due, img, hit
        ));
        if hit {
            let st = c.ui.modal.note_style.join(";");
            state::save_note_style(&c.cfg.state, &eid, &st);
            c.log.line(&format!("[modal] note style saved id={} st={}", eid, st));
        }
        c.ui.modal.cancel();
        return hit;
    }
    let id = c
        .st
        .add_custom_full(&text, &due, &prio, &note, &img)
        .unwrap_or_default();
    c.log.line(&format!(
        "[modal] save ok id={} prio={} due={} img={}",
        id, prio, due, img
    ));
    if !id.is_empty() {
        let st = c.ui.modal.note_style.join(";");
        state::save_note_style(&c.cfg.state, &id, &st);
        c.log.line(&format!("[modal] note style saved id={} st={}", id, st));
    }
    c.ui.modal.cancel();
    true
}

/// 字符进当前聚焦字段：模态打开时只喂模态（不再喂行内输入行）。
fn input_char(c: &mut Ctx, ch: char) -> Option<Draw> {
    // IME 上屏会经 WM_IME_COMPOSITION / WM_IME_CHAR / WM_CHAR 三条路径重复送达同一字符，
    // 「打一个字出两份」就是这里重复入库。两道去重：①上屏队列命中即吞；②同字符 60ms 内重复忽略。
    if let Some(pos) = c.ui.ime_pending.iter().position(|&p| p == ch) {
        c.ui.ime_pending.remove(pos);
        c.log
            .line(&format!("[input] dup skip(queued) ch=U+{:04X}", ch as u32));
        return None;
    }
    let now = std::time::Instant::now();
    if c.ui.last_ch == ch && now.duration_since(c.ui.last_at).as_millis() < 60 {
        c.log
            .line(&format!("[input] dup skip(60ms) ch=U+{:04X}", ch as u32));
        return None;
    }
    c.ui.last_ch = ch;
    c.ui.last_at = now;
    if c.ui.modal.open {
        if c.ui.modal.push_char(ch) {
            return Some(Draw::Full);
        }
        return None;
    }
    let nt = &mut c.ui.newtodo;
    if nt.active && nt.text.chars().count() < 60 {
        nt.text.push(ch);
        return Some(Draw::Left);
    }
    None
}

/// 系统文件选择框（只收图片），返回绝对路径。GetOpenFileNameW 自带模态消息循环，
/// 必须在 CTX 锁外调用。
unsafe fn pick_image(hwnd: HWND) -> Option<String> {
    let filter: Vec<u16> = "所有文件 (*.*)\0*.*\0\0".encode_utf16().collect();
    let title: Vec<u16> = "选择附件\0".encode_utf16().collect();
    let mut buf = vec![0u16; 1024];
    let mut ofn: OPENFILENAMEW = std::mem::zeroed();
    ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = hwnd;
    ofn.lpstrFilter = filter.as_ptr();
    ofn.lpstrFile = buf.as_mut_ptr();
    ofn.nMaxFile = buf.len() as u32;
    ofn.lpstrTitle = title.as_ptr();
    ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    if GetOpenFileNameW(&mut ofn) == 0 {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    if len == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// 点击命中总派发：按命中 id / Kind 分组，返回需要重绘的脏区（None = 不用重绘）。
/// 不拆函数：命中分支共用 `menu` / `kb` / `img_pick` / `preview` / `draw` 五个局部状态与同一个
/// `with_ctx` 闭包，拆开要跨函数传一堆状态，回归风险更高。
/// 命中类型/分区：① 全局守卫（locked 且非 Chip、modal 打开时屏蔽非 modal 命中）；
/// ② 设置面板 `ui:settings` + `set:*`（backdrop/cancel/save/opacity/tint./size./locked）；
/// ③ 模态 `modal:*`（backdrop/cancel/save/img/img.clear/preview/field:/nt./prio:/due:/cal:）；
/// ④ 新建待办 `newtodo.*` / `mine:add` 与删除确认 `:del`；
/// ⑤ 列表/归档 `todo:` `mine:` `arc:` `day:` 与头部 `head:` / `sig:`；
/// ⑥ 按 `Kind` 分派：Chip（标签/月历胶囊）、Circle（完成圈）、Row/Text（行点击）、Static。
fn on_click(h: &Hit) -> Option<Draw> {    let mut draw = None;
    let mut menu = false;
    // None=不变；Some(true)=抢占键盘输入；Some(false)=还回不激活态
    let mut kb: Option<bool> = None;
    // "modal:img"：系统文件框要在 CTX 锁外弹（它自带模态消息循环）
    let mut img_pick = false;
    // "modal:preview"：交给外壳打开已选附件
    let mut preview = false;

    with_ctx(|c| {
        if c.ui.locked && !matches!(h.kind, Kind::Chip) {
            return;
        }
        // 模态窗是唯一可交互层：遮罩/窗体内可点，其他命中一律忽略，
        // 否则会出现在模态上面又弹设置、又开归档的双层模态。
        if c.ui.modal.open
            && !h.id.starts_with("modal:")
            && h.id != "ui:settings"
            && h.kind != Kind::Static
        {
            return;
        }
        if c.ui.modal.open && h.id == "ui:settings" {
            // 设置只在没有模态时可用
            return;
        }
        if h.id == "dayback" {
            c.ui.sel_day = 0;
            c.log.line("[day] back to list");
            draw = Some(Draw::Full);
            return;
        }
        if h.id == "ui:emstat" {
            c.ui.em_tip = !c.ui.em_tip;
            c.log.line(&format!("[emstat] tip={}", c.ui.em_tip));
            draw = Some(Draw::Full);
            return;
        }
        if h.id == "ui:settings" {
            let t = view::TINTS
                .iter()
                .position(|(hex, _)| *hex == c.st.ui.tint)
                .unwrap_or(0);
            c.ui.set.tint = t;
            c.ui.set.opacity = c.st.ui.opacity.clamp(0.4, 1.0);
            c.ui.set.size = view::SIZES
                .iter()
                .position(|&(w, hh)| w == c.st.ui.win_w && hh == c.st.ui.win_h)
                .unwrap_or(1);
            c.ui.set.locked = c.st.ui.locked;
            c.ui.set.open = true;
            c.log.line("[settings] open");
            draw = Some(Draw::Full);
            return;
        }
        // 设置面板：z 序在模态之上，先于 modal 处理
        if c.ui.set.open {
            let mut act = false;
            if h.id == "set:backdrop" || h.id == "set:cancel" {
                c.ui.set.open = false;
                let t = view::TINTS
                    .iter()
                    .position(|(hex, _)| *hex == c.st.ui.tint)
                    .unwrap_or(0);
                layout::TINT.store(
                    ((view::TINTS[t].1 .0 as u32) << 16)
                        | ((view::TINTS[t].1 .1 as u32) << 8)
                        | view::TINTS[t].1 .2 as u32,
                    std::sync::atomic::Ordering::Relaxed,
                );
                c.log.line("[settings] cancel");
                act = true;
            } else if h.id == "set:save" {
                c.st.ui.opacity = c.ui.set.opacity;
                c.st.ui.tint = view::TINTS[c.ui.set.tint].0.to_string();
                // 尺寸以「窗口当前实际矩形」为准：用户自己拖出来的大小必须保留，
                // 绝不能用旧的预设值覆盖（这就是「改了尺寸保存不了」的根因）。
                let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                let _ = unsafe { GetWindowRect(c.hwnd as HWND, &mut rc) };
                let (w0, h0) = ((rc.right - rc.left).max(1), (rc.bottom - rc.top).max(1));
                c.st.ui.win_w = w0;
                c.st.ui.win_h = h0;
                c.st.ui.x = Some(rc.left);
                c.st.ui.y = Some(rc.top);
                c.st.ui.locked = c.ui.set.locked;
                layout::LOCKED.store(c.ui.set.locked, std::sync::atomic::Ordering::Relaxed);
                c.log.line(&format!(
                    "[settings] save opacity={:.2} tint={} size={}x{} locked={}",
                    c.st.ui.opacity, c.st.ui.tint, w0, h0, c.st.ui.locked
                ));
                let (path, st) = (c.cfg.state.clone(), c.st.clone());
                let saved = state::save(&path, &st, &c.log);
                c.ui.set.open = false;
                c.log.line(&format!("[settings] persisted={saved}"));
                act = true;
            } else if h.id == "set:opacity" {
                c.ui.set_dragging = true;
                c.ui.set.opacity = opacity_from_x(c.ui.mouse.0);
                c.log.line(&format!("[settings] opacity drag start {:.2}", c.ui.set.opacity));
                act = true;
            } else if let Some(rest) = h.id.strip_prefix("set:tint.") {
                if let Ok(i) = rest.parse::<usize>() {
                    if i < view::TINTS.len() {
                        c.ui.set.tint = i;
                        let (r, g, b) = view::TINTS[i].1;
                        layout::TINT.store(
                            ((r as u32) << 16) | ((g as u32) << 8) | b as u32,
                            std::sync::atomic::Ordering::Relaxed,
                        );
                        c.log.line(&format!("[settings] tint={}", view::TINTS[i].0));
                        act = true;
                    }
                }
            } else if let Some(rest) = h.id.strip_prefix("set:size.") {
                if let Ok(i) = rest.parse::<usize>() {
                    if i < view::SIZES.len() {
                        c.ui.set.size = i;
                        c.log.line(&format!(
                            "[settings] size={}x{}",
                            view::SIZES[i].0, view::SIZES[i].1
                        ));
                        act = true;
                    }
                }
            } else if h.id == "set:locked" {
                c.ui.set.locked = !c.ui.set.locked;
                c.log.line(&format!("[settings] locked={}", c.ui.set.locked));
                act = true;
            }
            if act {
                draw = Some(Draw::Full);
            }
            return;
        }
        // 模态窗打开时吞掉一切点击：只有 modal: 命中区有语义，点别处不再穿透到下层
        if c.ui.modal.open {
            if h.id == "modal:backdrop" {
                if c.ui.modal.cal_open {
                    c.ui.modal.cal_close();
                } else if c.ui.modal.due_menu {
                    c.ui.modal.due_menu = false;
                    c.log.line("[modal] due menu close src=outside");
                } else {
                    c.ui.modal.cancel();
                    c.log.line("[modal] cancel");
                    kb = Some(false);
                }
                draw = Some(Draw::Full);
            } else if h.id == "modal:cancel" {
                c.ui.modal.cancel();
                c.log.line("[modal] cancel");
                kb = Some(false);
                draw = Some(Draw::Full);
            } else if h.id == "modal:save" {
                if modal_save(c) {
                    kb = Some(false);
                }
                draw = Some(Draw::Full);
            } else if h.id == "modal:img.clear" {
                c.ui.modal.img.clear();
                c.log.line("[modal] img cleared");
                draw = Some(Draw::Full);
            } else if let Some(kind) = h.id.strip_prefix("modal:nt.") {
                let li = c.ui.modal.caret_line();
                let mut st = c.ui.modal.style_at(li);
                let mut parts: Vec<String> = st
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect();
                let has = |p: &[String], k: &str| p.iter().any(|x| x == k || x.starts_with(k));
                match kind {
                    "bold" => {
                        if has(&parts, "b") {
                            parts.retain(|x| x != "b");
                        } else {
                            parts.push("b".into());
                        }
                    }
                    "h" => {
                        let cur_h = parts.iter().find(|x| *x == "h1" || *x == "h2").cloned();
                        parts.retain(|x| x != "h1" && x != "h2");
                        parts.push(
                            match cur_h.as_deref() {
                                Some("h1") => "h2",
                                Some("h2") => "",
                                _ => "h1",
                            }
                            .into(),
                        );
                        parts.retain(|x| !x.is_empty());
                    }
                    "li" => {
                        parts.retain(|x| !x.starts_with("ol:"));
                        if has(&parts, "li") {
                            parts.retain(|x| x != "li");
                        } else {
                            parts.push("li".into());
                        }
                    }
                    "ol" => {
                        let cur_ol = parts.iter().find(|x| x.starts_with("ol:")).cloned();
                        parts.retain(|x| x != "li" && !x.starts_with("ol:"));
                        if cur_ol.is_none() {
                            let prev = if li > 0 {
                                c.ui
                                    .modal
                                    .style_at(li - 1)
                                    .split(',')
                                    .find_map(|x| x.strip_prefix("ol:").map(|s| s.to_string()))
                                    .and_then(|x| x.parse::<u32>().ok())
                                    .unwrap_or(0)
                            } else {
                                0
                            };
                            parts.push(format!("ol:{}", prev + 1));
                        }
                    }
                    _ => {}
                }
                st = parts.join(",");
                c.ui.modal.set_style(li, st.clone());
                c.log.line(&format!("[modal] note style {} line={} -> {}", kind, li, st));
                draw = Some(Draw::Full);

            } else if h.id == "modal:img" {
                img_pick = true;
            } else if h.id == "modal:preview" {
                preview = true;
            } else if h.id == "modal:field:0e" {
                // 点事项行右侧空白 = 光标放到末尾（不必精确点最后一个字）
                c.ui.modal.field = 0;
                c.ui.modal.caret_t = c.ui.modal.text.len();
                c.log.line(&format!("[modal] text caret -> {} (right-zone)", c.ui.modal.caret_t));
                kb = Some(true);
                draw = Some(Draw::Full);
            } else if h.id.starts_with("modal:field:") {
                c.ui.modal.field = h.id.rsplit(':').next().and_then(|s| s.parse().ok()).unwrap_or(0);
                // 点备注框：按点击位置把光标插到字中间，并记录拖选起点
                if c.ui.modal.field == 1 {
                    let hit = layout::note_hit(c.ui.mouse.0, c.ui.mouse.1);
                    let idx = match hit { Some(i) => i, None => c.ui.modal.note.len() };
                    c.ui.modal.caret = idx;
                    NOTE_ANCHOR.with(|a| a.set(idx));
                    NOTE_DRAG.with(|d| d.set(true));
                    c.ui.modal.sel = None;
                    c.log.line(&format!("[modal] caret -> {} (hit={})", idx, hit.is_some()));
                } else if c.ui.modal.field == 0 {
                    let hit = layout::item_hit(c.ui.mouse.0, c.ui.mouse.1);
                    c.log.line(&format!("[dbg] len={} mouse={} {}", c.ui.modal.text.len(), c.ui.mouse.0, layout::item_dbg()));
                    let idx = match hit { Some(i) => i, None => c.ui.modal.text.len() };
                    c.ui.modal.caret_t = idx;
                    c.ui.modal.sel_t = None;
                    ITEM_ANCHOR.with(|a| a.set(idx));
                    ITEM_DRAG.with(|d| d.set(true));
                    c.log.line(&format!("[modal] text caret -> {} (hit={})", idx, hit.is_some()));
                }
                kb = Some(true);
                draw = Some(Draw::Full);
            } else if let Some(p) = h.id.strip_prefix("modal:prio:") {
                c.ui.modal.set_prio(p);
                draw = Some(Draw::Full);
            } else if let Some(rest) = h.id.strip_prefix("modal:cal:day:") {
                if let Ok(d) = rest.parse::<u32>() {
                    c.ui.modal.cal_pick(d);
                    c.log.line(&format!("[modal] cal pick {}", c.ui.modal.due));
                    draw = Some(Draw::Full);
                }
            } else if h.id == "modal:cal:prev" || h.id == "modal:cal:next" {
                c.ui.modal.cal_move(if h.id.ends_with("prev") { -1 } else { 1 });
                draw = Some(Draw::Full);
            } else if h.id == "modal:cal:none" {
                c.ui.modal.cal_open = false;
                c.ui.modal.due_menu = false;
                c.ui.modal.due.clear();
                c.ui.modal.due_text.clear();
                c.ui.modal.due_sel = 6;
                c.ui.modal.due_custom = false;
                c.log.line("[modal] due -> none (cal)");
                draw = Some(Draw::Full);
            } else if h.id == "modal:cal:back" {
                c.ui.modal.cal_close();
                draw = Some(Draw::Full);
            } else if h.id == "modal:due" {
                let today = c.data.today.clone();
                if c.ui.modal.cal_open {
                    c.ui.modal.cal_close();
                    c.log.line("[modal] due menu close src=chip");
                } else if c.ui.modal.due_custom {
                    c.ui.modal.set_due(5, &today);
                    c.log.line("[modal] due menu open src=chip");
                } else {
                    c.ui.modal.due_menu = !c.ui.modal.due_menu;
                    c.log.line(if c.ui.modal.due_menu {
                        "[modal] due menu open src=chip"
                    } else {
                        "[modal] due menu close src=chip"
                    });
                }
                draw = Some(Draw::Full);
            } else if let Some(rest) = h.id.strip_prefix("modal:due:") {
                if let Ok(i) = rest.parse::<usize>() {
                    let today = c.data.today.clone();
                    c.ui.modal.set_due(i, &today);
                    c.log.line(&format!("[modal] due -> {} ({})", c.ui.modal.due, i));
                    c.log.line("[modal] due menu close src=item");
                    draw = Some(Draw::Full);
                }
            }
            return;
        }
        // 点别处即放弃正在输入的新建待办（＋ 自己不算「别处」）
        if c.ui.newtodo.active && !h.id.starts_with("newtodo.") && h.id != "mine:add" {
            // 点别处：已经打了字就当成确认落盘（不能把用户刚打的字丢掉），空行才取消
            if c.ui.newtodo.text.trim().is_empty() {
                c.ui.newtodo.cancel();
                c.log.line("[input] click away on empty -> cancel");
            } else if commit_newtodo(c) {
                c.log.line("[input] click away -> commit");
            } else {
                c.ui.newtodo.cancel();
            }
            kb = Some(false);
            draw = Some(Draw::Left);
        }
        // 删除二次确认：期间点别处即取消
        if let Some((pid, _)) = c.ui.pending.clone() {
            if pid != h.id {
                c.ui.pending = None;
                draw = Some(Draw::Left);
            }
        }
        match h.kind {
            Kind::Chip => {
                let target = if h.id == "tab.dept" {
                    "dept"
                } else if h.id == "tab.meet" {
                    "meet"
                } else {
                    "proj"
                };
                if h.id == "remind.close" {
                    if let Ok(mut g) = REMIND.lock() { *g = None; }
                    REMIND_AT.store(0, Ordering::Relaxed);
                    if MODE_ATOMIC.load(Ordering::Relaxed) != 2 {
                        unsafe {
                            let w = win_hwnd();
                            SetWindowPos(w, HWND_NOTOPMOST, 0, 0, 0, 0, 0x0002 | 0x0001 | 0x0010);
                        }
                    }
                    c.log.line("[remind] closed by user");
                    draw = Some(Draw::Full);
                    menu = true;
                } else if let Some(n) = h.id.strip_prefix("meet.span.").and_then(|x| x.parse::<u8>().ok()) {
                    MEET.with(|m| {
                        if let Some(md) = m.borrow_mut().as_mut() {
                            md.span = n;
                        }
                    });
                    c.log.line(&format!("[meet] span -> {}", n));
                    draw = Some(Draw::Full);
                } else if h.id.starts_with("tab.") && c.ui.tab != target {
                    c.ui.tab = target.to_string();
                    c.ui.pending = None;
                    // 标签胶囊在头部、月历点源也随标签变 → 必须整屏（静止层走 blit，很便宜）
                    draw = Some(Draw::Full);
                    menu = true; // 借这个标记：动作后落盘
                }
            }
            Kind::Circle => {
                c.ui.pending = None;
                if let Some(idx) = h.id.strip_prefix("todo:").and_then(|s| s.parse::<usize>().ok()) {
                    let key = c
                        .data
                        .todo
                        .get(idx)
                        .map(|t| format!("{}|{}|{}", t.model, t.name, t.d))
                        .unwrap_or_default();
                    if !key.is_empty() {
                        c.st.toggle(&key);
                        c.log.line(&format!("[click] toggle key={}", key));
                    }
                } else if let Some(idx) = h
                    .id
                    .strip_prefix("dept:")
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    // 部门待办：勾上 = 立即沉归档（不在列表里留删除线）
                    let cur = c
                        .st
                        .department
                        .get(idx)
                        .map(|it| (it.id.clone(), it.text.clone(), it.done));
                    if let Some((id, text, done)) = cur {
                        if !done {
                            let ok = c.st.archive_remove(&id, "done");
                            c.log.line(&format!(
                                "[click] dept done -> archive id={} ok={} {}",
                                id,
                                ok,
                                c.st.summary()
                            ));
                        } else if let Some(it) = c.st.department.get_mut(idx) {
                            it.done = false;
                            c.log.line(&format!("[click] dept un-done {}", text));
                        }
                    }
                } else if let Some(idx) = h
                    .id
                    .strip_prefix("mine:")
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    // 自建待办：同上，勾上即归档
                    let cur = c
                        .st
                        .custom
                        .get(idx)
                        .map(|it| (it.id.clone(), it.text.clone(), it.done));
                    if let Some((id, text, done)) = cur {
                        if !done {
                            let ok = c.st.archive_remove(&id, "done");
                            c.log.line(&format!(
                                "[click] custom done -> archive id={} ok={} {}",
                                id,
                                ok,
                                c.st.summary()
                            ));
                        } else if let Some(it) = c.st.custom.get_mut(idx) {
                            it.done = false;
                            c.log.line(&format!("[click] custom un-done {}", text));
                        }
                    }
                }
                draw = Some(Draw::Left);
            }
            Kind::Button => {
                if h.id == "arc:open" {
                    c.ui.arc_mode = true;
                    c.ui.arc_scroll = 0;
                    c.ui.arc_pending = None;
                    c.log.line("[click] archive view open");
                    draw = Some(Draw::Left);
                } else if h.id == "arc:back" {
                    c.ui.arc_mode = false;
                    c.ui.arc_scroll = 0;
                    c.ui.arc_pending = None;
                    c.log.line("[click] archive view back");
                    draw = Some(Draw::Left);
                } else if h.id.starts_with("arc:") {
                    draw = Some(archive_click(c, &h.id));
                } else if h.id == "mine:add" {
                    // ＋ 现在开模态窗；行内输入行（newtodo）保留但不再从这里触发
                    c.ui.modal.open(&c.data.today);
                    c.ui.pending = None;
                    kb = Some(true);
                    c.log.line("[modal] open");
                    draw = Some(Draw::Full);
                } else if h.id == "newtodo.datechip" {
                    let nt = &mut c.ui.newtodo;
                    nt.menu_open = !nt.menu_open;
                    if nt.menu_open {
                        nt.menu_sel = nt.date_sel;
                    }
                    draw = Some(Draw::Left);
                } else if let Some(rest) = h.id.strip_prefix("newtodo.datemenu.") {
                    if let Ok(i) = rest.parse::<usize>() {
                        let today = c.data.today.clone();
                        let (y, mo) = (c.ui.y, c.ui.mo);
                        c.ui.newtodo.pick_date(i, &today, y, mo);
                        c.log.line(&format!("[click] newtodo date -> {} ({})", c.ui.newtodo.due, i));
                        draw = Some(Draw::Left);
                    }
                } else if let Some(_rest) = h.id.strip_suffix(":del") {
                    let now = Instant::now();
                    let armed = matches!(&c.ui.pending, Some((id, deadline)) if id == &h.id && *deadline > now);
                    if armed {
                        c.ui.pending = None;
                        delete_row(c, &h.id);
                        c.log.line(&format!("[click] delete confirmed {}", h.id));
                    } else {
                        c.ui.pending = Some((h.id.clone(), now + DEL_CONFIRM));
                        c.log.line(&format!("[click] delete armed {} (3s)", h.id));
                    }
                    draw = Some(Draw::Left);
                }
            }
            Kind::Text => {
                if h.id == "newtodo.input" {
                    c.ui.newtodo.focus = true;
                    c.ui.newtodo.menu_open = false;
                    kb = Some(true);
                    draw = Some(Draw::Left);
                }
            }
            Kind::MonthPrev => {
                c.ui.pending = None;
                c.ui.sel_day = 0;
                flip(c, -1);
                draw = Some(Draw::Right);
            }
            Kind::MonthNext => {
                c.ui.pending = None;
                c.ui.sel_day = 0;
                flip(c, 1);
                draw = Some(Draw::Right);
            }
            Kind::DayCell => {
                c.ui.pending = None;
                if let Some(day) = h
                    .id
                    .strip_prefix("day:")
                    .and_then(|s| s.rsplit('-').next().and_then(|d| d.parse::<i32>().ok()))
                {
                    c.ui.sel_day = day;
                    c.log.line(&format!("[click] day {} ({})", day, h.id));
                    // 选中日期后左卡要切成「当天节点」，所以必须整窗重绘
                    draw = Some(Draw::Full);
                } else {
                    draw = Some(Draw::Right);
                }
            }
            _ => {}
        }
    });

    if let Some(on) = kb {
        unsafe {
            set_keyboard(on);
        }
    }
    // 系统文件选择框：阻塞式，必须在 CTX 锁外调用
    if img_pick {
        let hwnd = with_ctx_return(|c| c.hwnd) as HWND;
        let picked = unsafe { pick_image(hwnd) };
        with_ctx(|c| match picked {
            Some(p) => {
                c.log.line(&format!("[modal] img picked {}", p));
                c.ui.modal.img = p;
            }
            None => c.log.line("[modal] img pick cancelled"),
        });
        return Some(Draw::Full);
    }
    if preview {
        let path = with_ctx_return(|c| c.ui.modal.img.clone());
        if !path.is_empty() {
            open_path(&path);
            with_ctx(|c| c.log.line(&format!("[modal] preview {}", path)));
        }
        return Some(Draw::Full);
    }
    if menu || draw.is_some() {
        save_state();
    }
    draw
}

fn flip(c: &mut Ctx, delta: i32) {
    let mut m = c.ui.mo as i32 + delta;
    let mut y = c.ui.y;
    while m < 1 {
        m += 12;
        y -= 1;
    }
    while m > 12 {
        m -= 12;
        y += 1;
    }
    c.ui.mo = m as u32;
    c.ui.y = y;
}

/// `×` 不是物理删除：对齐小黄条 state=2 的软删 —— 移入归档（reason=deleted）。
fn delete_row(c: &mut Ctx, id: &str) {
    let core = id.trim_end_matches(":del");
    if let Some((prefix, idxs)) = core.split_once(':') {
        if let Ok(idx) = idxs.parse::<usize>() {
            let item_id = if prefix == "dept" {
                c.st.department.get(idx).map(|it| it.id.clone())
            } else if prefix == "mine" {
                c.st.custom.get(idx).map(|it| it.id.clone())
            } else {
                None
            };
            if let Some(item_id) = item_id {
                let ok = c.st.archive_remove(&item_id, "deleted");
                c.log.line(&format!(
                    "[click] archive_remove id={} ok={} {}",
                    item_id,
                    ok,
                    c.st.summary()
                ));
            }
        }
    }
}

/// 归档视图内的按钮：restore / purge（二次确认）。
fn archive_click(c: &mut Ctx, id: &str) -> Draw {
    let rest = match id.strip_prefix("arc:") {
        Some(r) => r,
        None => return Draw::Left,
    };
    let (i_s, verb) = match rest.split_once(':') {
        Some(v) => v,
        None => return Draw::Left,
    };
    let idx = match i_s.parse::<usize>() {
        Ok(i) => i,
        Err(_) => return Draw::Left,
    };
    let aid = match c.st.archive_list().get(idx) {
        Some(a) => a.id.clone(),
        None => return Draw::Left,
    };
    match verb {
        "restore" => {
            let ok = c.st.archive_restore(&aid);
            c.ui.arc_pending = None;
            c.log.line(&format!("[click] archive_restore id={} ok={}", aid, ok));
            state::save(&c.cfg.state, &c.st, &c.log);
            Draw::Left
        }
        "purge" => {
            let now = Instant::now();
            let armed = matches!(&c.ui.arc_pending, Some((pid, dl)) if pid == id && *dl > now);
            if armed {
                c.ui.arc_pending = None;
                let ok = c.st.archive_purge(&aid);
                c.log.line(&format!("[click] archive_purge id={} ok={}", aid, ok));
                state::save(&c.cfg.state, &c.st, &c.log);
            } else {
                c.ui.arc_pending = Some((id.to_string(), now + DEL_CONFIRM));
                c.log.line(&format!("[click] archive purge armed id={} (3s)", aid));
            }
            Draw::Left
        }
        _ => Draw::Left,
    }
}

// ───────────────────────── 新增待办：输入弹窗 ─────────────────────────

#[allow(dead_code)]
fn open_editor_window() {
    unsafe {
        let owner = with_ctx_return(|c| c.hwnd);
        if owner == 0 {
            return;
        }
        if EDIT_OWNER.load(Ordering::Relaxed) != 0 {
            return;
        }
        let hinstance = GetModuleHandleW(null_mut());
        let class_name = wide(EDIT_CLASS);
        let (wx, wy) = window_origin();
        let ew = 320;
        let eh = 104;
        let ex = wx + (WIN_W - ew) / 2;
        let ey = wy + (WIN_H - eh) / 2;
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name.as_ptr(),
            wide("新增待办").as_ptr(),
            WS_POPUP | WS_VISIBLE,
            ex,
            ey,
            ew,
            eh,
            owner as HWND,
            null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return;
        }
        EDIT_OWNER.store(hwnd as isize, Ordering::Relaxed);
        CreateWindowExW(
            0,
            wide("EDIT").as_ptr(),
            wide("").as_ptr(),
            WS_CHILD | WS_VISIBLE | 0x0004 /* ES_MULTILINE */ | 0x0080 /* ES_AUTOHSCROLL */,
            12,
            10,
            296,
            48,
            hwnd,
            1001 as *mut c_void,
            hinstance,
            std::ptr::null(),
        );
        CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide("添加 (Enter)").as_ptr(),
            WS_CHILD | WS_VISIBLE | 0x0001 /* BS_DEFPUSHBUTTON */,
            12,
            66,
            140,
            26,
            hwnd,
            101 as *mut c_void,
            hinstance,
            std::ptr::null(),
        );
        CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide("取消 (Esc)").as_ptr(),
            WS_CHILD | WS_VISIBLE,
            164,
            66,
            140,
            26,
            hwnd,
            102 as *mut c_void,
            hinstance,
            std::ptr::null(),
        );
        if let Some(edit) = non_null(GetDlgItem(hwnd, 1001)) {
            set_focus_raw(edit);
        }
        let _ = ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
    }
}

fn non_null(p: HWND) -> Option<HWND> {
    if p.is_null() {
        None
    } else {
        Some(p)
    }
}

unsafe fn set_focus_raw(h: HWND) {
    SetFocusW(h);
}

fn window_origin() -> (i32, i32) {
    unsafe {
        let hwnd = with_ctx_return(|c| c.hwnd);
        if hwnd == 0 {
            return (0, 0);
        }
        let mut r: RECT = std::mem::zeroed();
        let _ = GetWindowRect(hwnd as HWND, &mut r);
        (r.left, r.top)
    }
}

unsafe fn commit_editor(hwnd: HWND) {
    let mut buf = [0u16; 512];
    let n = GetWindowTextW(hwnd_for_text(hwnd), buf.as_mut_ptr(), 511);
    let text = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
    DestroyWindow(hwnd);
    EDIT_OWNER.store(0, Ordering::Relaxed);
    let t = text.trim().to_string();
    if t.is_empty() {
        return;
    }
    let today = with_ctx_return(|c| c.data.today.clone());
    with_ctx(|c| {
        c.st.add_custom(&t, &today);
        c.log.line(&format!("[click] custom added {}", t));
    });
    save_state();
    redraw(Draw::Left);
}

fn hwnd_for_text(hwnd: HWND) -> HWND {
    unsafe {
        match non_null(GetDlgItem(hwnd, 1001)) {
            Some(e) => e,
            None => hwnd,
        }
    }
}

unsafe extern "system" fn editor_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = wparam & 0xFFFF;
            if id == 101 || id == 1 {
                commit_editor(hwnd);
            } else if id == 102 {
                DestroyWindow(hwnd);
                EDIT_OWNER.store(0, Ordering::Relaxed);
            }
            0
        }
        WM_DESTROY => {
            EDIT_OWNER.store(0, Ordering::Relaxed);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ───────────────────────── 托盘菜单（embed 下临时置顶） ─────────────────────────

/// 托盘左键：显示/隐藏切换（隐藏时不抢焦点）
/// 退出：先摘托盘图标，再销毁窗口（WM_DESTROY 里 PostQuitMessage）。
/// 只有托盘菜单「退出」（以及 RW_TEST_HOOK 下的验收消息）走这里；关闭窗口只隐藏。
unsafe fn quit_app(hwnd: HWND, why: &str) {
    with_ctx(|c| c.log.line(&format!("[tray] quit via {}", why)));
    if TRAY_ON.swap(0, Ordering::Relaxed) != 0 {
        with_ctx(|c| {
            tray::remove(hwnd, &c.log);
        });
    }
    DestroyWindow(hwnd);
}

unsafe fn toggle_visible(hwnd: HWND) {
    let vis = IsWindowVisible(hwnd) != 0;
    let _ = ShowWindow(hwnd, if vis { SW_HIDE } else { SW_SHOWNOACTIVATE });
    with_ctx(|c| c.log.line(&format!("[tray] toggle visible -> {}", !vis)));
}

unsafe fn show_menu(hwnd: HWND) {
    let hidden = IsWindowVisible(hwnd) == 0;
    let auto_on = startup::enabled();
    let toggle_label = if hidden { "显示看板" } else { "隐藏看板" };
    let startup_label = if auto_on { "开机自启（已开启）" } else { "开机自启" };
    let items: Vec<(String, usize, bool)> = vec![
        (toggle_label.to_string(), MENU_TOGGLE, false),
        ("立即刷新".to_string(), MENU_RELOAD, false),
        ("打开看板网页".to_string(), MENU_OPEN, false),
        (startup_label.to_string(), MENU_STARTUP, auto_on),
        ("退出".to_string(), MENU_QUIT, false),
    ];
    // 自绘菜单窗口在本会话 CreateWindowExW 恒失败（reg=atom 成功但建窗返回 0），
    // 改用系统原生弹出菜单：同一份 items、同一个 menu_pick 分发，行为一致且不会卡。
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, DestroyMenu, SetForegroundWindow, TrackPopupMenu,
            MF_CHECKED, MF_STRING, TPM_RETURNCMD, TPM_RIGHTBUTTON,
        };
        let hm = CreatePopupMenu();
        if hm.is_null() {
            with_ctx(|c| c.log.line("[tray] CreatePopupMenu failed"));
            return;
        }
        for (label, id, checked) in &items {
            let w = wide(label);
            let flags = MF_STRING | if *checked { MF_CHECKED } else { 0 };
            AppendMenuW(hm, flags, *id, w.as_ptr());
        }
        let mut p = POINT { x: 0, y: 0 };
        let _ = GetCursorPos(&mut p);
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            hm,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            p.x,
            p.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        DestroyMenu(hm);
        with_ctx(|c| c.log.line(&format!("[tray] menu pick={}", cmd)));
        if cmd != 0 {
            menu_pick(hwnd, cmd as usize);
        }
    }
}

/// 菜单项动作：自绘菜单点击后由 wndproc 统一执行（WM_MENU lParam=0x9000）。
unsafe fn menu_pick(hwnd: HWND, id: usize) {
    let name = match id {
        MENU_TOGGLE => "隐藏看板/显示看板",
        MENU_RELOAD => "立即刷新",
        MENU_OPEN => "打开看板网页",
        MENU_STARTUP => "开机自启",
        MENU_QUIT => "退出",
        _ => "?",
    };
    with_ctx(|c| c.log.line(&format!("[menu] pick={}", name)));
    match id {
        MENU_TOGGLE => toggle_visible(hwnd),
        MENU_RELOAD => menu_reload(),
        MENU_OPEN => {
            with_ctx(|c| {
                c.log.line("[menu] open kanban");
                open_kanban(&c.log);
            });
        }
        MENU_STARTUP => {
            let mut now = false;
            with_ctx(|c| {
                let lg = c.log.clone();
                now = startup::toggle(&lg);
                c.log.line(&format!("[menu] startup -> {}", now));
            });
            let _ = now;
        }
        MENU_QUIT => quit_app(hwnd, "menu"),
        _ => {}
    }
}

/// 「立即刷新」：重读磁盘 + 触发一轮采集。菜单项 MENU_RELOAD 与验收钩子（WM_MENU lParam=0x7FFE）共用这一条路径。
/// 菜单分区：托盘 5 项（显示/隐藏、立即刷新、打开看板网页、开机自启、退出）由 `menu_pick` 派发，
/// 本函数只承接「立即刷新」；`wndproc` 的 WM_MENU 分支另处理 0x9000 点选与 0x7FFF 退出钩子。
unsafe fn menu_reload() {
    with_ctx(|c| {
        let (d, ok) = data::load(&c.cfg.data, &c.log);
        c.data = d;
        c.last_mtime = data::mtime(&c.cfg.data);
        c.st = state::load(&c.cfg.state, &c.cfg.yynote, &c.log);
        c.last_state_mtime = data::mtime(&c.cfg.state);
        c.log
            .line(&format!("[menu] reload ok={} {}", ok, c.data.summary()));
        // 除了立即重读磁盘，还触发一轮采集（下一 tick 开跑）
        let lg = c.log.clone();
        c.pull.trigger_now(&lg);
        c.log.line("[menu] reload -> pull triggered");
    });
    redraw(Draw::Full);
}

// ───────────────────────── 窗口过程 ─────────────────────────

/// 窗口过程：单一 match 分发全部窗口消息；消息分支共享 CTX / hwnd / 命中链状态，不拆函数。
/// 菜单项分区（WM_MENU）：左键 WM_LBUTTONUP 显隐、0x0205/0 弹菜单，0x9000 → `menu_pick`
///   （显示/隐藏、立即刷新、打开看板网页、开机自启、退出），0x7FFE 刷新钩子、0x7FFF 退出钩子。
/// 消息/命中分组：① 绘制 WM_PAINT / WM_ERASEBKGND；② 命中与尺寸 WM_NCHITTEST /
///   WM_GETMINMAXINFO / WM_SIZE / WM_SIZING / WM_EXITSIZEMOVE（980..1900 × 566..1200）；
///   ③ 光标/滚轮 WM_SETCURSOR / WM_MOUSEWHEEL；④ 定时 WM_TIMER；
///   ⑤ 键盘 WM_KEYDOWN（Enter/←/→/Del/Ctrl+A/Ctrl+C/Esc/Tab/Back）；
///   ⑥ 输入法 WM_IME_CHAR / WM_IME_COMPOSITION / WM_CHAR（中文去重复送达）；
///   ⑦ 鼠标命中 WM_LBUTTONDOWN / WM_MOUSEMOVE / WM_LBUTTONUP（拖窗 drag_allowed、双击打开、删除确认）；
///   ⑧ 生命周期 WM_CLOSE（隐藏到托盘）/ WM_DESTROY（摘托盘图标 + PostQuitMessage）。
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let _ = BeginPaint(hwnd, &mut ps);
            let _ = EndPaint(hwnd, &ps);
            0
        }
        WM_ERASEBKGND => 1,
        // t1 的 HTCAPTION 必须去掉：整窗当标题栏会让控件点不到（验收 A10/A11 依赖真实命中）
        WM_NCHITTEST => {
            if layout::LOCKED.load(Ordering::Relaxed) {
                return HTCLIENT as LRESULT;
            }
            let sx = (lparam & 0xFFFF) as i16 as i32;
            let sy = ((lparam >> 16) & 0xFFFF) as i16 as i32;
            let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            let _ = GetWindowRect(hwnd, &mut rc);
            let (rx, ry) = (sx - rc.left, sy - rc.top);
            let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
            let b = 6;
            let (left, right, top, bottom) = (rx < b, rx >= w - b, ry < b, ry >= h - b);
            let hit = match (left, right, top, bottom) {
                (true, _, true, _) => HTTOPLEFT,
                (_, true, true, _) => HTTOPRIGHT,
                (true, _, _, true) => HTBOTTOMLEFT,
                (_, true, _, true) => HTBOTTOMRIGHT,
                (true, _, _, _) => HTLEFT,
                (_, true, _, _) => HTRIGHT,
                (_, _, true, _) => HTTOP,
                (_, _, _, true) => HTBOTTOM,
                _ => HTCLIENT,
            };
            hit as LRESULT
        }
        WM_GETMINMAXINFO => {
            let mmi = lparam as *mut MINMAXINFO;
            if !mmi.is_null() {
                (*mmi).ptMinTrackSize.x = 980;
                (*mmi).ptMinTrackSize.y = 566;
                (*mmi).ptMaxTrackSize.x = 1900;
                (*mmi).ptMaxTrackSize.y = 1200;
            }
            0
        }
        WM_SIZE => {
            // 每次尺寸变化都按新尺寸重排（否则会出现「右边被裁掉」：画布/位图还是旧大小）
            let mut lw = (lparam & 0xFFFF) as i32;
            let mut lh = ((lparam >> 16) & 0xFFFF) as i32;
            // 无边框窗口不吃 WM_GETMINMAXINFO，这里自己兜住最小/最大尺寸，
            // 否则可以被压到 700x400，右卡日历与卡片就被裁掉。
            let (minw, minh, maxw, maxh) = (980i32, 566i32, 1900i32, 1200i32);
            let cw = lw.clamp(minw, maxw);
            let ch = lh.clamp(minh, maxh);
            if cw != lw || ch != lh {
                unsafe {
                    SetWindowPos(hwnd, 0 as HWND, 0, 0, cw, ch, SWP_NOMOVE | SWP_NOZORDER);
                }
                lw = cw;
                lh = ch;
            }
            if lw > 0 && lh > 0 {
                let need = with_ctx_return(|c| c.canvas.w != lw || c.canvas.h != lh);
                if need {
                    with_ctx(|c| {
                        layout::set_box(lw, lh);
                        let mut nv = canvas::Canvas::new(lw, lh);
                        nv.sx = 1.0;
                        nv.sy = 1.0;
                        nv.ox = 0.0;
                        nv.oy = 0.0;
                        c.canvas = nv;
                        let mut nb = canvas::Canvas::new(lw, lh);
                        nb.sx = 1.0;
                        nb.sy = 1.0;
                        nb.ox = 0.0;
                        nb.oy = 0.0;
                        c.base = nb;
                        c.base_valid = false;
                        c.pres = Present::new(lw, lh);
                        c.st.ui.win_w = lw;
                        c.st.ui.win_h = lh;
                    });
                    redraw(Draw::Full);
                }
            }
            0
        }
        // 拖边角时锁死 980:566 长宽比 —— 位图放大是等比的，界面不会被压扁。
        WM_SIZING => {
            // 自由缩放：布局按新尺寸重排（左卡变宽、右卡贴右），字号与图形不放大
            1
        }
        WM_EXITSIZEMOVE => {
            let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            let _ = GetWindowRect(hwnd, &mut rc);
            let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
            with_ctx(|c| {
                c.st.ui.x = Some(rc.left);
                c.st.ui.y = Some(rc.top);
                c.st.ui.win_w = w;
                c.st.ui.win_h = h;
                // 真正重排：画布按窗口像素尺寸重建，布局坐标仍按 980×566 写，
                // 由画布自带的 sx/sy 放大 —— 文字按目标字号重画，所以不会糊。
                layout::set_box(w, h);
                let (rsx, rsy, rox, roy) = (1.0f32, 1.0f32, 0.0f32, 0.0f32);
                let mut nv = canvas::Canvas::new(w.max(1), h.max(1));
                nv.sx = rsx;
                nv.sy = rsy;
                nv.ox = rox;
                nv.oy = roy;
                let sc = rsx;
                c.canvas = nv;
                let mut nb = canvas::Canvas::new(w.max(1), h.max(1));
                nb.sx = rsx;
                nb.sy = rsy;
                nb.ox = rox;
                nb.oy = roy;
                nb.sx = rsx;
                nb.sy = rsy;
                nb.ox = rox;
                nb.oy = roy;
                c.base = nb;
                c.base_valid = false;
                // 呈现用的 DIB 必须跟着窗口长，否则超出部分被裁掉（右/下留一块空白）
                c.pres = Present::new(w.max(1), h.max(1));
                crate::state::save(&c.cfg.state, &c.st, &c.log);
                c.log.line(&format!("[size] {}x{} at {},{} scale={:.3} saved", w, h, rc.left, rc.top, sc));
            });
            redraw(Draw::Full);
            0
        }
        WM_SETCURSOR => {
            unsafe { set_cursor_by_pos(hwnd) };
            1
        }
        WM_MOUSEWHEEL => {
            // 模态窗打开时不滚底层（归档视图在遮罩下面）
            if with_ctx_return(|c| c.ui.modal.open) {
                return 0;
            }
            // 归档视图：9 行固定，滚轮一次 3 行（外层矩形不变）
            let delta = ((wparam >> 16) & 0xFFFF) as u16 as i16;
            let up = delta > 0;
            let changed = with_ctx_return(|c| {
                let mut v = build_view(&c.data, &c.st, &c.ui);
                let moved = v.arc_wheel(up);
                if moved {
                    c.ui.arc_scroll = v.arc_scroll;
                }
                moved
            });
            if changed {
                redraw(Draw::Left);
            }
            // 待办列表（部门页非归档视图）：超出 9 行时滚轮翻 3 行
            let list_moved = with_ctx_return(|c| {
                let v = build_view(&c.data, &c.st, &c.ui);
                if v.arc_mode || v.tab != view::Tab::Dept {
                    return false;
                }
                let n = v.dept.len() + v.custom.len();
                let max = n.saturating_sub(layout::l_rows());
                if max == 0 {
                    return false;
                }
                let cur = c.ui.scroll.min(max);
                let next = if up { cur.saturating_sub(3) } else { (cur + 3).min(max) };
                if next != c.ui.scroll {
                    c.ui.scroll = next;
                    true
                } else {
                    false
                }
            });
            if list_moved {
                redraw(Draw::Left);
            }
            0
        }
        WM_TIMER => {
            // 两路定时器共用窗口：1) 每秒调度/热载 tick  2) 500ms 光标闪烁
            if wparam == 0x77 {
                let blink = with_ctx_return(|c| {
                    if c.ui.modal.open && c.ui.modal.field == 1 {
                        let on = !layout::CARET_ON.load(Ordering::Relaxed);
                        layout::CARET_ON.store(on, Ordering::Relaxed);
                        true
                    } else {
                        false
                    }
                });
                if blink {
                    redraw(Draw::Full);
                }
            } else {
                tick(hwnd);
            }
            0
        }
        WM_KEYDOWN => {
            let vk = wparam as u16 as u32;
            let mut done: Option<Draw> = None;
            let mut saved = false;
            with_ctx(|c| {
                if c.ui.set.open {
                    if vk == 0x1B {
                        c.ui.set.open = false;
                        let t = view::TINTS
                            .iter()
                            .position(|(hex, _)| *hex == c.st.ui.tint)
                            .unwrap_or(0);
                        let (r, g, b) = view::TINTS[t].1;
                        layout::TINT.store(
                            ((r as u32) << 16) | ((g as u32) << 8) | b as u32,
                            std::sync::atomic::Ordering::Relaxed,
                        );
                        c.log.line("[settings] cancel");
                        done = Some(Draw::Full);
                    }
                    return;
                }
                // 模态窗最高优先：键盘只喂它，行内输入行完全不碰
                if c.ui.modal.open {
                    match vk {
                        0x0D => {
                            if c.ui.modal.field == 1 {
                                if c.ui.modal.note_newline() {
                                    done = Some(Draw::Full);
                                }
                            } else if modal_save(c) {
                                saved = true;
                                done = Some(Draw::Full);
                            }
                        }
                        0x25 if c.ui.modal.field == 1 => {
                            if c.ui.modal.move_caret(-1) {
                                done = Some(Draw::Full);
                            }
                        }
                        0x27 if c.ui.modal.field == 1 => {
                            if c.ui.modal.move_caret(1) {
                                done = Some(Draw::Full);
                            }
                        }
                        0x25 if c.ui.modal.field == 0 => {
                            if c.ui.modal.move_caret_t(-1) {
                                done = Some(Draw::Full);
                            }
                        }
                        0x27 if c.ui.modal.field == 0 => {
                            if c.ui.modal.move_caret_t(1) {
                                done = Some(Draw::Full);
                            }
                        }
                        0x2E if c.ui.modal.field == 0 => {
                            if c.ui.modal.delete_sel_t() {
                                done = Some(Draw::Full);
                            }
                        }
                        0x2E if c.ui.modal.field == 1 => {
                            if c.ui.modal.delete_selection() {
                                done = Some(Draw::Full);
                            }
                        }
                        0x41 if GetKeyState(0x11) < 0 && c.ui.modal.open && c.ui.modal.field == 0 => {
                            c.ui.modal.sel_t = Some((0, c.ui.modal.text.len()));
                            c.log.line("[modal] text select all");
                            done = Some(Draw::Full);
                        }
                        0x43 if GetKeyState(0x11) < 0 && c.ui.modal.open && c.ui.modal.field == 0 => {
                            let s2 = match c.ui.modal.sel_t {
                                Some((a, b)) => c.ui.modal.text.get(a..b).unwrap_or("").to_string(),
                                None => c.ui.modal.text.clone(),
                            };
                            let ok = copy_text(&s2);
                            c.log.line(&format!("[modal] text copy chars={} ok={}", s2.chars().count(), ok));
                            done = Some(Draw::Full);
                        }
                        0x43 if GetKeyState(0x11) < 0 => {
                            let picked = match c.ui.modal.sel {
                                Some((a, b)) => c
                                    .ui
                                    .modal
                                    .note
                                    .get(a.min(b)..a.max(b))
                                    .unwrap_or("")
                                    .to_string(),
                                None => c.ui.modal.note.clone(),
                            };
                            let ok = copy_text(&picked);
                            c.log.line(&format!(
                                "[modal] copy chars={} ok={}",
                                picked.chars().count(),
                                ok
                            ));
                            done = Some(Draw::Full);
                        }
                        0x1B => {
                            if c.ui.modal.cal_open {
                                c.ui.modal.cal_close();
                            } else if c.ui.modal.due_menu {
                                c.ui.modal.due_menu = false;
                                c.log.line("[modal] due menu close src=outside");
                            } else {
                                c.ui.modal.cancel();
                                c.log.line("[modal] cancel");
                            }
                            done = Some(Draw::Full);
                        }
                        0x09 => {
                            // Tab / Shift+Tab：两个字段来回切
                            let back = GetKeyState(0x10) < 0;
                            c.ui.modal.tab(back);
                            done = Some(Draw::Full);
                        }
                        0x08 => {
                            c.ui.modal.backspace();
                            done = Some(Draw::Full);
                        }
                        _ => {}
                    }
                    return;
                }
                if !c.ui.newtodo.active {
                    return;
                }
                match vk {
                    0x0D => {
                        // Enter：空行不创建
                        if commit_newtodo(c) {
                            saved = true;
                            done = Some(Draw::Left);
                        }
                    }
                    0x1B => {
                        c.ui.newtodo.cancel();
                        c.log.line("[input] cancel (Esc)");
                        done = Some(Draw::Left);
                    }
                    0x08 => {
                        c.ui.newtodo.text.pop();
                        done = Some(Draw::Left);
                    }
                    _ => {}
                }
            });
            if let Some(k) = done {
                if saved {
                    save_state();
                }
                let active = with_ctx_return(|c| c.ui.modal.open || c.ui.newtodo.active);
                if !active {
                    unsafe { set_keyboard(false) };
                }
                redraw(k);
            }
            0
        }
        WM_IME_CHAR => {
            // 部分输入法（兼容模式）直接把已上屏字符发进来
            let ch = wparam as u32;
            let Some(ch) = char::from_u32(ch) else {
                return 0;
            };
            let mut drew: Option<Draw> = None;
            with_ctx(|c| {
                drew = input_char(c, ch);
            });
            if let Some(k) = drew {
                redraw(k);
            }
            0
        }
        WM_IME_COMPOSITION => {
            // 中文上屏：从 IME 取 GCS_RESULTSTR（已确定的字符串），追加到输入框
            if lparam as u32 & GCS_RESULTSTR != 0 {
                let mut s = String::new();
                unsafe {
                    let himc = ImmGetContext(hwnd);
                    if !himc.is_null() {
                        let n = ImmGetCompositionStringW(himc, GCS_RESULTSTR, null_mut(), 0);
                        if n > 0 {
                            let mut buf = vec![0u16; (n as usize / 2) + 1];
                            let got = ImmGetCompositionStringW(
                                himc,
                                GCS_RESULTSTR,
                                buf.as_mut_ptr() as *mut c_void,
                                (buf.len() * 2) as u32,
                            );
                            if got > 0 {
                                s = String::from_utf16_lossy(&buf[..(got as usize / 2)]);
                            }
                        }
                        ImmReleaseContext(hwnd, himc);
                    }
                }
                if !s.is_empty() {
                    let mut drew: Option<Draw> = None;
                    with_ctx(|c| {
                        for ch in s.chars() {
                            if let Some(k) = input_char(c, ch) {
                                // 只有 composition 成功入库的字符才登记：随后 WM_IME_CHAR / WM_CHAR
                                // 会把同一字再送一次，input_char 靠此队列吞掉重复（两道去重见其头注释）。
                                c.ui.ime_pending.push(ch);
                                drew = Some(k);
                            }
                        }
                    });
                    if let Some(k) = drew {
                        redraw(k);
                    }
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CHAR => {
            let ch = wparam as u32;
            // 控制字符交给 WM_KEYDOWN（回车/退格/Esc 已在上面处理）
            if ch < 0x20 || ch == 0x7F {
                return 0;
            }
            let mut drew: Option<Draw> = None;
            with_ctx(|c| {
                if let Some(ch) = char::from_u32(ch) {
                    drew = input_char(c, ch);
                }
            });
            if let Some(k) = drew {
                redraw(k);
            }
            0
        }
        WM_LBUTTONDOWN => {
            let x = (lparam & 0xFFFF) as i16 as i32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
            let (x, y) = map_mouse(hwnd, x, y);
            with_ctx(|c| c.ui.mouse = (x, y));
            let hit = hit_at(x, y);
            let locked = LOCKED_ATOMIC.load(Ordering::Relaxed) != 0;
            // 双击打开项目看板：只对 正文/项目行/日期格/优先级胶囊 生效；
            // 空白与拖拽区、勾选圈、按钮、标签页胶囊都退回单击语义。
            // 命中双击时吞掉这一次按下（不派发 on_click、不进入拖拽），日志里就是 1 次 [click] + 1 次 [open]。
            // 部门进度标签里双击不打开网页（只允许项目进度）
            // 部门进度里双击自建待办 → 打开编辑弹窗
            if !locked && with_ctx_return(|c| c.ui.tab.as_str() == "dept") {
                if let Some(h) = &hit {
                    // 双击任一待办行（自建 custom / 部门导入 department）都打开编辑弹窗
                    let parsed = if let Some(rest) = h.id.strip_prefix("mine:") {
                        rest.split(':').next().and_then(|s| s.parse::<usize>().ok()).map(|i| (0u8, i))
                    } else if let Some(rest) = h.id.strip_prefix("dept:") {
                        rest.split(':').next().and_then(|s| s.parse::<usize>().ok()).map(|i| (1u8, i))
                    } else {
                        None
                    };
                    if let Some((grp, i)) = parsed {
                        if is_double_click(x, y) {
                            let data = with_ctx_return(|c| {
                                if grp == 0 {
                                    c.st.custom.get(i).map(|it| {
                                        (it.id.clone(), it.text.clone(), it.note.clone(), it.prio.clone(), it.img.clone(), it.due.clone())
                                    })
                                } else {
                                    c.st.department.get(i).map(|it| {
                                        (it.id.clone(), it.text.clone(), it.note.clone(), it.prio.clone(), String::new(), it.due.clone().unwrap_or_default())
                                    })
                                }
                            });
                            if let Some((id, tx, nt2, pr, im, du)) = data {
                                with_ctx(|c| {
                                    c.ui.modal.open_edit(&id, &tx, &nt2, &pr, &im, &du);
                unsafe { SetTimer(hwnd, 0x77, 500, None) };
                let stored = state::load_note_style(&c.cfg.state, &id);
                if !stored.is_empty() {
                    let mut vs: Vec<String> = stored.split(';').map(|s| s.to_string()).collect();
                    let need = c.ui.modal.note.split('\n').count();
                    while vs.len() < need {
                        vs.push(String::new());
                    }
                    c.ui.modal.note_style = vs;
                    c.log.line(&format!("[modal] note style loaded id={} st={}", id, stored));
                } else if c.ui.modal.note_style.is_empty() {
                    c.ui.modal.note_style = vec![String::new()];
                }
                                    c.log.line(&format!("[modal] edit open id={} grp={}", id, grp));
                                });
                                set_keyboard(true);
                                redraw(Draw::Full);
                                return 0;
                            }
                        }
                    }
                }
            }
            // 行拖拽排序：按住自建待办行（非勾选圈）开始，松开时按落点换位
            if !locked {
                if let Some(h) = &hit {
                    // 整行可拖：命中行体(Row)或行内正文(Text)都算；排除 :del/:due/:att 这类子命中
                    let row_like = matches!(h.kind, Kind::Row | Kind::Text)
                        && (h.id.starts_with("mine:") || h.id.starts_with("dept:"))
                        && h.id.split(':').count() == 2;
                    if row_like {
                        let hit_group = if let Some(rest) = h.id.strip_prefix("mine:") {
                            rest.split(':')
                                .next()
                                .and_then(|s| s.parse::<usize>().ok())
                                .map(|i| (0u8, i))
                        } else if let Some(rest) = h.id.strip_prefix("dept:") {
                            rest.split(':')
                                .next()
                                .and_then(|s| s.parse::<usize>().ok())
                                .map(|i| (1u8, i))
                        } else {
                            None
                        };
                        if let Some((grp, i)) = hit_group {
                            with_ctx(|c| {
                                let dlen = c.st.department.len();
                                c.ui.drag_u = Some(if grp == 1 { i } else { dlen + i });
                                c.ui.drag_to = c.ui.drag_u;
                                c.ui.row_drag = Some((grp, i, y));
                                c.log.line(&format!("[drag] start grp={} idx={} y={}", grp, i, y));
                            });
                            SetCapture(hwnd);
                        }
                    }
                }
            }
            let openable = hit.as_ref().map(|h| h.kind.openable()).unwrap_or(false)
                && with_ctx_return(|c| c.ui.tab.as_str() == "proj");
            if openable && !locked && is_double_click(x, y) {                // ShellExecuteW 会走 shell 的 DDE/COM 握手，需要本线程继续泵消息；
                // 在窗口过程里同步调用 = 自锁，Windows 会判 AppHangB1 直接关掉进程。
                // 因此：锁内只取 Logger，真正 ShellExecute 丢到后台线程。
                // 冷却 1.2s：双击的第二下、连点都会再进来一次，避免一次双击开出一堆标签页
                let now_ms = unsafe { GetTickCount64() };
                let last = LAST_OPEN.load(Ordering::Relaxed);
                if now_ms.saturating_sub(last) < 1200 {
                    return 0;
                }
                LAST_OPEN.store(now_ms, Ordering::Relaxed);
                let mut lg_opt: Option<Logger> = None;
                with_ctx(|c| {
                    lg_opt = Some(c.log.clone());
                });
                if let Some(lg) = lg_opt {
                    std::thread::spawn(move || open_kanban(&lg));
                }
                return 0;
            }
            match hit {
                Some(h) if h.kind.draggable() && !locked && drag_allowed(hwnd, x, y) => {
                    let mut p = POINT { x: 0, y: 0 };
                    let _ = GetCursorPos(&mut p);
                    let mut r: RECT = std::mem::zeroed();
                    let _ = GetWindowRect(hwnd, &mut r);
                    let ox = p.x - r.left;
                    let oy = p.y - r.top;
                    with_ctx(|c| {
                        c.drag = Some((ox, oy));
                        c.log.line(&format!("[drag] start at {},{} kind={}", x, y, h.kind.name()));
                    });
                    DRAGGING.store(1, Ordering::Relaxed);
                    SetCapture(hwnd);
                }
                Some(h) => {
                    if let Some(kind) = on_click(&h) {
                        redraw(kind);
                    }
                }
                None => {}
            }
            0
        }
        WM_MOUSEMOVE => {
            // WM_SETCURSOR 只在指针"物理进入"时由系统投递；程序化 SetCursorPos
            // （验收脚本 A12 就是这么测的）不一定触发，所以移动时也刷一次光标形状。
            unsafe { set_cursor_by_pos(hwnd) };
            let (mx0, my0) = (
                (lparam & 0xFFFF) as i16 as i32,
                ((lparam >> 16) & 0xFFFF) as i16 as i32,
            );
            let (mx, my) = map_mouse(hwnd, mx0, my0);
            let now = unsafe { GetTickCount64() };
            let mut do_redraw = false;
            with_ctx(|c| {
                c.ui.mouse = (mx, my);
                // 备注框：按住左键拖过文字即选中（黄底），松手保留
                    if ITEM_DRAG.with(|d| d.get()) && c.ui.modal.open && c.ui.modal.field == 0 {
                        let hit = layout::item_hit(c.ui.mouse.0, c.ui.mouse.1);
                        let idx = match hit { Some(k) => k, None => c.ui.modal.text.len() };
                        let a0 = ITEM_ANCHOR.with(|v| v.get());
                        c.ui.modal.caret_t = idx;
                        c.ui.modal.sel_t = if idx == a0 { None } else { Some((a0.min(idx), a0.max(idx))) };
                        do_redraw = true;
                    }
                if NOTE_DRAG.with(|d| d.get()) && c.ui.modal.open && c.ui.modal.field == 1 {
                    if let Some(idx) = layout::note_hit(mx, my) {
                        let a = NOTE_ANCHOR.with(|v| v.get());
                        c.ui.modal.sel = if idx == a { None } else { Some((a.min(idx), a.max(idx))) };
                        do_redraw = true;
                    }
                }
                // 齿轮悬停：进出都立刻重绘一次（不做节流，状态变化很少）
                let ghov = (940..=976).contains(&mx) && (0..=34).contains(&my);
                if ghov != layout::GEAR_HOVER.load(Ordering::Relaxed) {
                    layout::GEAR_HOVER.store(ghov, Ordering::Relaxed);
                    do_redraw = true;
                }
                if c.ui.row_drag.is_some() {
                    if let Some((grp, from, y0)) = c.ui.row_drag {
                        let dy = my - y0;
                        let _ = (grp, from);
                        let n = c.st.department.len() + c.st.custom.len();
                        if n > 1 {
                            let base = c.ui.drag_u.unwrap_or(0) as i32;
                            let t = (base + (dy as f32 / layout::L_ROW_H as f32).round() as i32)
                                .clamp(0, n as i32 - 1) as usize;
                            c.ui.drag_to = Some(t);
                        }
                        do_redraw = true;
                    }
                }
                if c.ui.set_dragging {
                    if (wparam as u32) & 0x0001 != 0 {
                        let v = opacity_from_x(mx);
                        if (v - c.ui.set.opacity).abs() > 0.001 {
                            c.ui.set.opacity = v;
                            do_redraw = true;
                        }
                    } else {
                        c.ui.set_dragging = false;
                        c.log.line(&format!("[settings] opacity drag end {:.2}", c.ui.set.opacity));
                        do_redraw = true;
                    }
                }
                if !c.ui.modal.open {
                    // 左卡列表区域才需要悬停预览；节流 80ms，避免拖动窗口时满帧重绘
                    let inside = (24..=24 + 584).contains(&mx) && (96..=96 + 454).contains(&my);
                    if inside && now.saturating_sub(c.ui.hover_ms) >= 80 {
                        c.ui.hover_ms = now;
                        do_redraw = true;
                    }
                }
            });
            if DRAGGING.load(Ordering::Relaxed) == 0 {
                if do_redraw {
                    redraw(Draw::Full);
                }
                return 0;
            }
            let drag = with_ctx_return(|c| c.drag);
            if let Some((ox, oy)) = drag {
                let mut p = POINT { x: 0, y: 0 };
                let _ = GetCursorPos(&mut p);
                SetWindowPos(
                    hwnd,
                    0 as HWND,
                    p.x - ox,
                    p.y - oy,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            0
        }
        WM_LBUTTONUP => {
            NOTE_DRAG.with(|d| d.set(false));
            ITEM_DRAG.with(|d| d.set(false));
            // 行拖拽排序：落点命中另一条自建待办才换位
            let (_rx, ry) = map_mouse(
                hwnd,
                (lparam & 0xFFFF) as i16 as i32,
                ((lparam >> 16) & 0xFFFF) as i16 as i32,
            );
            let row_move = with_ctx_return(|c| c.ui.row_drag.take());
            if let Some((grp, from, y0)) = row_move {
                ReleaseCapture();
                let (drag_u, drag_to) = with_ctx_return(|c| (c.ui.drag_u, c.ui.drag_to));
                with_ctx(|c| c.log.line(&format!("[drag] up grp={} from={} dy={}", grp, from, ry - y0)));
                if let (Some(u), Some(t)) = (drag_u, drag_to) {
                    if u != t && (ry - y0).abs() >= 8 {
                        let mut ok = false;
                        with_ctx(|c| {
                            let dlen = c.st.department.len();
                            let total = dlen + c.st.custom.len();
                            if t < total {
                                let fg = grp;
                                let fi = from;
                                let (tg, ti) = if t < dlen { (1u8, t) } else { (0u8, t - dlen) };
                                // 跨列表也允许（面板视觉上是一条列表：部门在前、自建在后）。
                                // 落到别的组时该条会挂到目标组，顺序按用户拖放结果持久化。
                                // t 已经是「移除自身之后」的目标下标，不能再 -1。
                                ok = c.st.move_todo_to(fg, fi, tg, ti);
                                if fg != tg {
                                    c.log.line(&format!("[row] move-cross {}:{} -> {}:{} ok={}", fg, fi, tg, ti, ok));
                                }
                                c.log.line(&format!(
                                    "[row] move {}:{} -> {}:{} ok={}",
                                    fg, fi, tg, ti, ok
                                ));
                            }
                        });
                        if ok {
                            save_state();
                        }
                    }
                }
                with_ctx(|c| {
                    c.ui.drag_u = None;
                    c.ui.drag_to = None;
                });
                redraw(Draw::Full);
            }
            if DRAGGING.load(Ordering::Relaxed) != 0 {
                DRAGGING.store(0, Ordering::Relaxed);
                ReleaseCapture();
                let mut r: RECT = std::mem::zeroed();
                let _ = GetWindowRect(hwnd, &mut r);
                with_ctx(|c| {
                    c.drag = None;
                    c.cfg.x = Some(r.left);
                    c.cfg.y = Some(r.top);
                    c.st.ui.x = Some(r.left);
                    c.st.ui.y = Some(r.top);
                    c.log.line(&format!("[drag] end pos={},{}", r.left, r.top));
                });
                save_state();
            }
            0
        }
        WM_MENU => {
            // 托盘回调：lParam 低字是鼠标消息。左键=显示/隐藏；右键=菜单；
            // 0（验收脚本 PostMessage(hwnd, WM_MENU, 0, 0)）=也弹菜单。
            let low = (lparam as u32) & 0xFFFF;
            // 0x7FFF 是验收钩子（只有 RW_TEST_HOOK=1 时生效）：走和菜单「退出」同一条路径
            let hook = std::env::var_os("RW_TEST_HOOK").is_some();
            match low {
                WM_LBUTTONUP => toggle_visible(hwnd),
                0x0205 /* WM_RBUTTONUP */ => show_menu(hwnd),
                0 /* PostMessage(hwnd, WM_MENU, 0, 0) 验收钩子：手动弹菜单 */ => show_menu(hwnd),
                0x9000 /* 自绘菜单点选：wParam 是菜单项 id */ => {
                    menu_pick(hwnd, wparam)
                }
                // 验收钩子（RW_TEST_HOOK=1）：与菜单「立即刷新」同一条代码路径
                0x7FFE if hook => {
                    with_ctx(|c| c.log.line("[tray] reload via test-hook"));
                    menu_reload();
                }
                0x7FFF if hook => quit_app(hwnd, "test-hook"),
                // 其余鼠标消息（WM_MOUSEMOVE 0x0200 / WM_LBUTTONDOWN 0x0201 …）一律忽略：
                // 之前这里落到 show_menu，导致鼠标悬停托盘图标就弹菜单。
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            // 关闭按钮/Alt+F4 只隐藏到托盘，不退出
            let _ = ShowWindow(hwnd, SW_HIDE);
            with_ctx(|c| c.log.line("[tray] close -> hidden (process alive)"));
            0
        }
        WM_DESTROY => {
            with_ctx(|c| c.log.line("[gui] destroy"));
            // 先摘托盘图标再退出，避免死图标残留
            if TRAY_ON.swap(0, Ordering::Relaxed) != 0 {
                with_ctx(|c| {
                    tray::remove(hwnd, &c.log);
                });
            }
            let icon = TRAY_ICON.swap(0, Ordering::Relaxed);
            if icon != 0 {
                tray::destroy_icon(icon as HICON);
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// 光标形状唯一来源：命中链 + 锁定/拖拽状态（A12 契约）
unsafe fn set_cursor_by_pos(hwnd: HWND) {
    let mut p = POINT { x: 0, y: 0 };
    let _ = GetCursorPos(&mut p);
    let mut c = p;
    ScreenToClient(hwnd, &mut c);
    let locked = LOCKED_ATOMIC.load(Ordering::Relaxed) != 0;
    let dragging = DRAGGING.load(Ordering::Relaxed) != 0;
    let kind = with_ctx_return(|k| k.hits.hit(c.x, c.y).map(|h| (h.kind, h.id.clone())));
    let sizeall = !locked
        && (dragging
            || kind
                .as_ref()
                .map(|(k, _)| k.draggable())
                .unwrap_or(false));
    // 输入框给 I 形光标，让用户一眼看出「这里能打字」
    let ibeam = kind
        .as_ref()
        .map(|(_, id)| id.starts_with("modal:field") || id == "newtodo.input" || id == "modal:nt")
        .unwrap_or(false);
    let cur = LoadCursorW(
        null_mut(),
        if ibeam {
            IDC_IBEAM
        } else if sizeall {
            IDC_SIZEALL
        } else {
            IDC_ARROW
        },
    );
    SetCursor(cur);
}

fn tick(hwnd: HWND) {
    let (ticks, data_due, state_due) = with_ctx_return(|c| {
        c.ticks = c.ticks.wrapping_add(1);
        let reload_ticks = c.cfg.reload_secs.max(1);
        let due = c.ticks % reload_ticks == 0;
        let dm = if due {
            let cur = data::mtime(&c.cfg.data);
            if cur != c.last_mtime {
                c.last_mtime = cur;
                true
            } else {
                false
            }
        } else {
            false
        };
        let cur_st = data::mtime(&c.cfg.state);
        let sm = cur_st != c.last_state_mtime;
        (c.ticks, dm, sm)
    });

    check_remind(hwnd);

    // 删除 / 归档彻底删除的二次确认超时：3 秒内没再点就撤回
    let expired = with_ctx_return(|c| {
        let mut hit = false;
        if let Some((_, deadline)) = &c.ui.pending {
            if *deadline <= Instant::now() {
                c.ui.pending = None;
                hit = true;
            }
        }
        if let Some((_, deadline)) = &c.ui.arc_pending {
            if *deadline <= Instant::now() {
                c.ui.arc_pending = None;
                hit = true;
            }
        }
        hit
    });
    if expired {
        redraw(Draw::Left);
    }

    if state_due {
        with_ctx(|c| {
            c.st = state::load(&c.cfg.state, &c.cfg.yynote, &c.log);
            let n = c.st.sweep_done();
            if n > 0 {
                state::save(&c.cfg.state, &c.st, &c.log);
            }
            c.last_state_mtime = data::mtime(&c.cfg.state);
            c.log.line(&format!("[state] reloaded {} swept={}", c.st.summary(), n));
        });
        redraw(Draw::Full);
    }

    if data_due {
        with_ctx(|c| {
            let (next, ok) = data::load(&c.cfg.data, &c.log);
            c.data = next;
            c.log.line(&format!("[data] reload ok={} {}", ok, c.data.summary()));
            // 文本位图 key 含内容，旧条目不会复现；整清一次保证不涨
            c.te.clear_cache();
            load_meet(&c.cfg, &c.log);
            c.base_valid = false; // 头部数字/头部文案变了，静止层要重建
        });
        redraw(Draw::Full);
    }

    // 采集调度（t7）：每秒推进 state machine；开跑/收工都立刻刷新页面
    let (p_started, p_finished, u_finished) = with_ctx_return(|c| {
        let lg = c.log.clone();
        let r = c.pull.tick(&lg);
        (r.started, r.finished, r.unread_finished)
    });
    // t8：轻量短连接写完 widget_data.json 立刻热载，不等 reload-secs 轮询
    if p_finished || u_finished {
        with_ctx(|c| {
            let (next, ok) = data::load(&c.cfg.data, &c.log);
            c.data = next;
            c.last_mtime = data::mtime(&c.cfg.data);
            // 文本位图 key 含内容：整清一次，避免旧条目常驻
            c.te.clear_cache();
            load_meet(&c.cfg, &c.log);
            c.base_valid = false;
            let summary = c.data.summary();
            c.log
                .line(&format!(
                    "[data] reload after {} ok={} {}",
                    if u_finished && !p_finished { "unread-live" } else { "pull" },
                    ok,
                    summary
                ));
        });
        redraw(Draw::Full);
    } else if p_started {
        redraw(Draw::Full);
    }

    if MEET_REFRESH_DONE.swap(false, std::sync::atomic::Ordering::SeqCst) {
        with_ctx(|c| load_meet(&c.cfg, &c.log));
        redraw(Draw::Full);
    }
    maybe_refresh_meet();

    if ticks % MEM_EVERY_TICKS == 0 {
        unsafe { log_mem("timer") };
    }
}

// ───────────────────────── 内存采样 ─────────────────────────

unsafe fn log_mem(tag: &str) {
    let mut pmc: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
    pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    let ok = GetProcessMemoryInfo(
        GetCurrentProcess(),
        &mut pmc as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS,
        pmc.cb,
    );
    if ok == 0 {
        return;
    }
    let line = format!(
        "[{tag}] working_set={} KB private_bytes={} KB ws_peak={} KB\n",
        pmc.WorkingSetSize / 1024,
        pmc.PrivateUsage / 1024,
        pmc.PeakWorkingSetSize / 1024
    );
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let path = dir.join("mem-baseline.log");
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = f.write_all(line.as_bytes());
            }
        }
    }
    print!("{line}");
    let _ = std::io::stdout().flush();
}

/// 把文本放进系统剪贴板（CF_UNICODETEXT）。返回是否成功。
fn copy_text(s: &str) -> bool {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    const CF_UNICODETEXT: u32 = 13;
    if s.is_empty() {
        return false;
    }
    let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * 2;
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return false;
        }
        EmptyClipboard();
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if h.is_null() {
            CloseClipboard();
            return false;
        }
        let p = GlobalLock(h) as *mut u16;
        if p.is_null() {
            CloseClipboard();
            return false;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
        GlobalUnlock(h);
        let ok = !SetClipboardData(CF_UNICODETEXT, h as *mut core::ffi::c_void).is_null();
        CloseClipboard();
        ok
    }
}


// ---- 会议数据自刷新（内置 10 分钟一次，替代外部计划任务）----
static MEET_REFRESH_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static LAST_MEET_REFRESH: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

fn find_python(root: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut cands: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(v) = std::env::var("FRD_PYTHON") {
        if !v.trim().is_empty() {
            cands.push(std::path::PathBuf::from(v));
        }
    }
    cands.push(root.join("tools").join("python").join("python.exe"));
    for c in cands {
        if c.is_file() {
            return Some(c);
        }
    }
    Some(std::path::PathBuf::from("python.exe"))
}

fn maybe_refresh_meet() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let last = LAST_MEET_REFRESH.load(std::sync::atomic::Ordering::SeqCst);
    if last != 0 && now - last < 600 {
        return;
    }
    if last != 0 && MEET_REFRESH_DONE.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    LAST_MEET_REFRESH.store(now, std::sync::atomic::Ordering::SeqCst);
    let mut data_opt: Option<std::path::PathBuf> = None;
    let mut log_opt: Option<Logger> = None;
    with_ctx(|c| {
        data_opt = Some(c.cfg.data.clone());
        log_opt = Some(c.log.clone());
    });
    let (data, log) = match (data_opt, log_opt) {
        (Some(d), Some(l)) => (d, l),
        _ => return,
    };
    let root = data
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let script = root.join("research").join("ec_meeting_fetch.py");
    if !script.is_file() {
        log.line(&format!("[meet] refresh skip no-script path={}", script.display()));
        return;
    }
    let py = match find_python(&root) {
        Some(p) => p,
        None => return,
    };
    let out = data.parent().map(|p| p.join("meeting_data.json")).unwrap_or_else(|| std::path::PathBuf::from("meeting_data.json"));
    log.line(&format!("[meet] refresh start py={} out={}", py.display(), out.display()));
    std::thread::spawn(move || {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new(&py);
        cmd.arg("-X").arg("utf8").arg(&script).arg("--out").arg(&out).arg("--quiet");
        cmd.current_dir(&root);
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        match cmd.status() {
            Ok(st) => {
                let code = st.code().unwrap_or(-1);
                log.line(&format!("[meet] refresh done code={}", code));
                if code == 0 {
                    MEET_REFRESH_DONE.store(true, std::sync::atomic::Ordering::SeqCst);
                } else {
                    // 失败不置位：下个 10 分钟周期会重试，不会整会话失联
                    log.line("[meet] refresh 非零退出，本轮不重绘，下个周期重试");
                }
            }
            Err(e) => log.line(&format!("[meet] refresh fail err={}", e)),
        }
    });
}
