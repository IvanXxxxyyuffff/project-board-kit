//! 顶层常量与无障碍/坐标/命令行小工具（自 main.rs 拆出）。
use super::*;

pub(crate) const WM_SIZING: u32 = 0x0214;
pub(crate) const WIN_W: i32 = layout::W;
pub(crate) const WIN_H: i32 = layout::H;

/// 双击打开的看板页面（与项目根目录同级）
pub(crate) const KANBAN_HTML: &str = r"项目数据看板.html";
/// 双击判定窗口：必须短于单击间隔（验收 A17a/A17b 用 450ms 连点三次，不能被误判成双击）
pub(crate) const DBL_GAP_MS: u64 = 350;
pub(crate) static LAST_OPEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// 双击落点容差（像素）
pub(crate) const DBL_SLOP: i32 = 4;

/// 上一次「可打开目标」上按下的时间与坐标：(tick_ms, x, y)
pub(crate) static LAST_DOWN: Mutex<Option<(u64, i32, i32)>> = Mutex::new(None);

/// 判定这次按下是否构成对同一目标的双击。命中即清空，避免三击连开。
pub(crate) fn is_double_click(x: i32, y: i32) -> bool {
    let now = unsafe { GetTickCount64() };
    let mut slot = match LAST_DOWN.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let dbl = match *slot {
        Some((t, px, py)) => now.saturating_sub(t) <= DBL_GAP_MS
            && (px - x).abs() <= DBL_SLOP
            && (py - y).abs() <= DBL_SLOP,
        None => false,
    };
    *slot = if dbl { None } else { Some((now, x, y)) };
    dbl
}

/// 打开项目看板：ShellExecuteW 交给系统默认浏览器，并写一行 [open] 日志。
pub(crate) fn open_kanban(log: &Logger) {
    let kanban = config::resolve_rel(KANBAN_HTML);
    let kanban_s = kanban.display().to_string();
    let exists = kanban.is_file();
    log.line(&format!("[open] {} exists={}", kanban_s, exists));
    let verb = wide("open");
    let path = wide(&kanban_s);
    unsafe {
        let r = ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            path.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
        // ShellExecuteW 成功时返回 >32 的伪句柄，失败返回错误码
        if (r as isize) <= 32 {
            log.line(&format!("[open] shell_execute failed code={}", r as isize));
        }
    }
}

/// 用系统默认程序打开任意文件（附件预览）。
pub(crate) fn open_path(path: &str) {
    let verb = wide("open");
    let p = wide(path);
    unsafe {
        let _ = ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            p.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// IME：中文必须处理这两条，否则自绘输入框永远拿不到已上屏的汉字。
pub(crate) const WM_IME_CHAR: u32 = 0x0286;
pub(crate) const WM_IME_COMPOSITION: u32 = 0x010F;
pub(crate) const GCS_RESULTSTR: u32 = 0x0800;
/// 设置面板：把鼠标 x 换算成 0.40–1.00 的不透明度（5% 步进）
pub(crate) fn opacity_from_x(mx: i32) -> f64 {
    let x0 = layout::SET_SLIDER_X.load(Ordering::Relaxed);
    let w = layout::SET_SLIDER_W.load(Ordering::Relaxed).max(1);
    let rel = ((mx - x0) as f64 / w as f64).clamp(0.0, 1.0);
    (((0.4 + rel * 0.6) * 20.0).round() / 20.0).clamp(0.4, 1.0)
}

/// 只有标题条（y<88）与四周 18px 留白能拖窗；卡片正文一律不拖，
/// 否则点待办列表的空白处会把窗口拽走，用户看到的就是「点一下窗口没了」。
pub(crate) fn drag_allowed(hwnd: HWND, x: i32, y: i32) -> bool {
    if layout::LOCKED.load(std::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    // 用实际客户区尺寸算右/下边界，别再写死 980×566：窗口可缩放（WM_GETMINMAXINFO 允许
    // 980..1900 × 566..1200），固定阈值会在放大后把大片正文误判成可拖区。取不到时退回 WIN_W/WIN_H。
    // map_mouse 仍是 1:1，不换算坐标（历史教训：压缩坐标会让全部命中区错位）。
    let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let (w, h) = unsafe {
        if GetClientRect(hwnd, &mut rc) != 0 {
            (rc.right - rc.left, rc.bottom - rc.top)
        } else {
            (WIN_W, WIN_H)
        }
    };
    y < 88 || x < 18 || y > h - 18 || x > w - 18
}

pub(crate) fn parse_hex_rgb(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some((((v >> 16) & 255) as u8, ((v >> 8) & 255) as u8, (v & 255) as u8))
}

/// 客户区坐标 → 命中坐标：保持 1:1，不做缩放。
/// 布局已按窗口尺寸实时重排（layout::set_box），命中区就是窗口坐标；历史上按 980×566 比例
/// 压缩坐标会让所有命中区整体错位（点哪都不中），因此这里绝不换算。
pub(crate) fn map_mouse(_hwnd: HWND, x: i32, y: i32) -> (i32, i32) {
    (x, y)
}

