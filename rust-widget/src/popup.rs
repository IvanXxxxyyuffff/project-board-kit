//! 会议提醒独立弹窗。
//!
//! 设计要点（用户要求）：
//! - 只有这个弹窗被置顶/前台，主挂件保持普通层级，不抢焦点；
//! - 内容必须写全：会议名称、时间、地点、参会人员；
//! - 必须点「知道了」才关闭，**没有自动收起**。
//!
//! 实现是用一个独立的 WS_POPUP 顶层窗口 + GDI 自绘，避免和主窗的消息循环互相干扰。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
    GetStockObject, SelectObject, SetBkMode, SetTextColor, DT_LEFT, DT_SINGLELINE, DT_VCENTER,
    NULL_BRUSH, PAINTSTRUCT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetSystemMetrics, RegisterClassW,
    SetForegroundWindow, SetWindowPos, ShowWindow, CW_USEDEFAULT, HWND_TOPMOST, SM_CXSCREEN,
    SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_SHOW, WM_CLOSE, WM_ERASEBKGND,
    WM_LBUTTONUP, WM_PAINT, WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

/// 弹窗尺寸（逻辑像素）。
pub const PW: i32 = 460;
pub const PH: i32 = 214;

const BTN_W: i32 = 88;
const BTN_H: i32 = 30;

/// 提醒内容：时间、名称、地点、参会人、剩余分钟。
#[derive(Clone, Default)]
pub struct Body {
    pub time: String,
    pub name: String,
    pub place: String,
    pub people: String,
    pub left: i32,
}

static BODY: Mutex<Option<Body>> = Mutex::new(None);
static HWNDS: Mutex<isize> = Mutex::new(0);
static ALIVE: AtomicBool = AtomicBool::new(false);

/// 弹窗是否还开着（主程序用它判断是否需要清掉提醒态）。
pub fn alive() -> bool {
    ALIVE.load(Ordering::Relaxed)
}

fn hwnd_opt() -> Option<HWND> {
    let v = HWNDS.lock().ok().map(|g| *g).unwrap_or(0);
    if v == 0 {
        None
    } else {
        Some(v as HWND)
    }
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

unsafe extern "system" fn wnd_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            paint(h);
            0
        }
        WM_LBUTTONUP => {
            let x = (lp as i32) & 0xFFFF;
            let y = ((lp as i32) >> 16) & 0xFFFF;
            let bx = PW - 18 - BTN_W;
            let by = PH - 16 - BTN_H;
            if x >= bx && x <= bx + BTN_W && y >= by && y <= by + BTN_H {
                close();
            }
            0
        }
        WM_CLOSE => {
            close();
            0
        }
        WM_ERASEBKGND => 1,
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

unsafe fn paint(h: HWND) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(h, &mut ps);
    let rc: RECT = RECT { left: 0, top: 0, right: PW, bottom: PH };

    // 背景 + 描边
    let bg = CreateSolidBrush(rgb(34, 24, 26));
    FillRect(hdc, &rc, bg);
    DeleteObject(bg as _);
    let line = CreateSolidBrush(rgb(255, 99, 90));
    for band in [
        RECT { left: 0, top: 0, right: rc.right, bottom: 2 },
        RECT { left: 0, top: rc.bottom - 2, right: rc.right, bottom: rc.bottom },
        RECT { left: rc.right - 2, top: 0, right: rc.right, bottom: rc.bottom },
    ] {
        FillRect(hdc, &band, line);
    }
    DeleteObject(line as _);
    // 左侧红色竖条
    let bar: RECT = RECT {
        left: 0,
        top: 0,
        right: 5,
        bottom: rc.bottom,
    };
    let red = CreateSolidBrush(rgb(255, 92, 82));
    FillRect(hdc, &bar, red);
    DeleteObject(red as _);

    let body = BODY.lock().ok().and_then(|g| (*g).clone()).unwrap_or_default();
    SetBkMode(hdc, 1); // TRANSPARENT
    let old = SelectObject(hdc, GetStockObject(NULL_BRUSH));

    let name_font = CreateFontW(
        -20, 0, 0, 0, 700, 0, 0, 0, 134, 0, 0, 0, 0,
        wide("Microsoft YaHei").as_ptr(),
    );
    let text_font = CreateFontW(
        -15, 0, 0, 0, 400, 0, 0, 0, 134, 0, 0, 0, 0,
        wide("Microsoft YaHei").as_ptr(),
    );

    // 第一行：还有几分钟 + 开始时间
    let mut r1 = RECT {
        left: 20,
        top: 16,
        right: rc.right - 18,
        bottom: 40,
    };
    SelectObject(hdc, text_font);
    SetTextColor(hdc, rgb(255, 138, 128));
    let head = if body.left > 0 {
        format!("{} 分钟后开始 · {}", body.left, body.time)
    } else {
        format!("即将开始 · {}", body.time)
    };
    draw(hdc, &head, &mut r1);

    // 第二行：会议名称
    let mut r2 = RECT {
        left: 20,
        top: 44,
        right: rc.right - 18,
        bottom: 74,
    };
    SelectObject(hdc, name_font);
    SetTextColor(hdc, rgb(240, 242, 247));
    draw(hdc, &body.name, &mut r2);

    // 第三行：地点
    let mut r3 = RECT {
        left: 20,
        top: 84,
        right: rc.right - 18,
        bottom: 106,
    };
    SelectObject(hdc, text_font);
    SetTextColor(hdc, rgb(176, 182, 194));
    let place = if body.place.trim().is_empty() {
        "地点：—".to_string()
    } else {
        format!("地点：{}", body.place)
    };
    draw(hdc, &place, &mut r3);

    // 第四行：参会人员（超长截断）
    let mut r4 = RECT {
        left: 20,
        top: 108,
        right: rc.right - 18,
        bottom: 152,
    };
    let people = if body.people.trim().is_empty() {
        "参会人员：—".to_string()
    } else {
        format!("参会人员：{}", truncate(&body.people, 30))
    };
    draw(hdc, &people, &mut r4);

    // 按钮：知道了
    let bx = rc.right - 18 - BTN_W;
    let by = rc.bottom - 16 - BTN_H;
    let btn = RECT {
        left: bx,
        top: by,
        right: bx + BTN_W,
        bottom: by + BTN_H,
    };
    SelectObject(hdc, old as _);
    let btn_bg = CreateSolidBrush(rgb(255, 194, 71));
    FillRect(hdc, &btn, btn_bg);
    DeleteObject(btn_bg as _);
    SelectObject(hdc, text_font);
    SetTextColor(hdc, rgb(28, 24, 20));
    let mut rb = RECT {
        left: bx,
        top: by + 6,
        right: bx + BTN_W,
        bottom: by + BTN_H,
    };
    draw_center(hdc, "知道了", &mut rb);

    DeleteObject(name_font as _);
    DeleteObject(text_font as _);
    EndPaint(h, &ps);
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn truncate(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let t: String = s.chars().take(max).collect();
    format!("{}…", t)
}

unsafe fn draw(hdc: *mut core::ffi::c_void, s: &str, rc: &mut RECT) {
    let mut buf = wide(s);
    let n = (buf.len() - 1) as i32;
    DrawTextW(
        hdc,
        buf.as_mut_ptr(),
        n,
        rc,
        DT_LEFT | DT_SINGLELINE | DT_VCENTER,
    );
}

unsafe fn draw_center(hdc: *mut core::ffi::c_void, s: &str, rc: &mut RECT) {
    let mut buf = wide(s);
    let n = (buf.len() - 1) as i32;
    let flags = DT_CENTER_FLAG | DT_SINGLELINE | DT_VCENTER;
    DrawTextW(hdc, buf.as_mut_ptr(), n, rc, flags);
}

const DT_CENTER_FLAG: u32 = 0x0000_0001;

fn ensure_class() {
    unsafe {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let cls = wide("FrdMeetPopup");
            let mut wc: WNDCLASSW = std::mem::zeroed();
            wc.lpfnWndProc = Some(wnd_proc);
            wc.hInstance = std::ptr::null_mut();
            wc.lpszClassName = cls.as_ptr();
            wc.hbrBackground = GetStockObject(NULL_BRUSH) as _;
            RegisterClassW(&wc);
        });
    }
}

/// 弹出提醒窗：置顶 + 前台，只影响本窗，不动主挂件。
pub fn show(b: Body) {
    ensure_class();
    if let Some(h) = hwnd_opt() {
        if let Ok(mut g) = BODY.lock() {
            *g = Some(b);
        }
        unsafe {
            invalidate(h);
            ShowWindow(h, SW_SHOW);
            SetWindowPos(
                h,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            SetForegroundWindow(h);
        }
        return;
    }
    if let Ok(mut g) = BODY.lock() {
        *g = Some(b);
    }
    unsafe {
        let cw = GetSystemMetrics(SM_CXSCREEN);
        let ch = GetSystemMetrics(SM_CYSCREEN);
        let x = (cw - PW) / 2;
        let y = ch / 3;
        let cls = wide("FrdMeetPopup");
        let title = wide("会议提醒");
        let h = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            cls.as_ptr(),
            title.as_ptr(),
            WS_POPUP | WS_VISIBLE,
            if cw <= 0 { CW_USEDEFAULT } else { x },
            y,
            PW,
            PH,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        );
        if h.is_null() {
            return;
        }
        if let Ok(mut g) = HWNDS.lock() {
            *g = h as isize;
        }
        ALIVE.store(true, Ordering::Relaxed);
        SetWindowPos(
            h,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        SetForegroundWindow(h);
    }
}

fn close() {
    if let Some(h) = hwnd_opt() {
        unsafe {
            DestroyWindow(h);
        }
    }
    if let Ok(mut g) = HWNDS.lock() {
        *g = 0;
    }
    ALIVE.store(false, Ordering::Relaxed);
    if let Ok(mut g) = BODY.lock() {
        *g = None;
    }
}

fn invalidate(h: HWND) {
    unsafe {
        windows_sys::Win32::Graphics::Gdi::InvalidateRect(h, std::ptr::null(), 0);
    }
}
