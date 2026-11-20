//! 980×566 深色玻璃面板绘制 + 命中区登记（纯 alpha 贴图，无模糊 / 无 Acrylic / 无点击穿透）。
//! 版式：头部（大日号 + 周几年月 + 3 计数胶囊）｜左卡（抓取待办 + 我的待办 + 未读消息）｜右卡（月历）。
//! 所有几何都是整数字面量：命中区跟着同一批常量走，布局一改两边一起变，不会漂。

use crate::canvas::{Canvas, Rgba};
use crate::data::{TodoItem as Todo, WidgetData as Data};
use crate::hit::{Hits, Kind};
use crate::lunar;
use crate::text::TextEngine;
use crate::view::{DeptRow, Tab, View, NEWDATE_OPTIONS};
use std::sync::atomic::{AtomicI32, Ordering};

pub const W: i32 = 980;
pub const H: i32 = 566;

// —— 主题色（与 桌面挂件.html 对齐）——
pub const INK: Rgba = (238, 241, 246, 255);
pub const INK2: Rgba = (167, 174, 189, 255);
pub const INK3: Rgba = (111, 118, 134, 255);
pub const ACC: Rgba = (255, 159, 67, 255);
pub const P0C: Rgba = (255, 95, 86, 255);
pub const P1C: Rgba = (255, 191, 71, 255);
pub const P2C: Rgba = (90, 169, 255, 255);
/// 面板底色（脏区回填必须用它，不能 clear 成透明）。
/// 验收/截图用：把面板 alpha 顶到 255，去掉桌面透出，像素哈希才可比。
pub static OPAQUE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_opaque(on: bool) {
    OPAQUE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// 当前主题底色（0xRRGGBB），由 state.ui.tint 设置
pub static TINT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x12141A);
/// 固定位置（锁）状态：齿轮旁显示小锁，拖拽被禁用
pub static LOCKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 齿轮是否处于鼠标悬停（由主窗口 WM_MOUSEMOVE 维护）
pub static GEAR_HOVER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 设置面板里「不透明度」滑块的绝对几何（绘制时写入，供拖拽换算用）
pub static SET_SLIDER_X: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
pub static SET_SLIDER_W: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(260);

pub fn tint_rgb() -> (u8, u8, u8) {
    // 固定纯黑底：主题色功能已按用户要求移除（"还是黑的好看"）。
    // 仍读 TINT 是为了兼容旧 state 文件，但不再跟随它变色。
    let _ = TINT.load(std::sync::atomic::Ordering::Relaxed);
    (18, 20, 26)
}

fn panel_bg() -> Rgba {
    let (r, g, b) = tint_rgb();
    if OPAQUE.load(std::sync::atomic::Ordering::Relaxed) {
        (r, g, b, 255)
    } else {
        (r, g, b, 246)
    }
}

// —— 字号（t3 契约：基准 13 / 次级 11 / 标签 10 / 大日号 44 / 栏标题 20）——
// 字号基准（t3 契约）：13 / 11 / 10 / 44 / 20。--font-size 走 FONT_PCT 整体缩放，
// 版式栅格（行高、列宽）不变 —— 固定命中矩形优先于填满。
/// 左卡 / 右卡矩形（脏区回填与命中裁剪用，窗口层直接读这里）
pub fn left_rect() -> (i32, i32, i32, i32) {
    (LEFT_X, CARD_Y, left_w(), card_h())
}
pub fn right_rect() -> (i32, i32, i32, i32) {
    (right_x(), CARD_Y, RIGHT_W, card_h())
}

pub const F_BASE_PX: i32 = 13;
pub const F_SUB_PX: i32 = 11;
pub const F_TAG_PX: i32 = 10;
pub const F_BIG_PX: i32 = 44;
pub const F_TITLE_PX: i32 = 20;

pub static FONT_PCT: AtomicI32 = AtomicI32::new(100);

/// 只允许 80%–140%（13→16 是 123%），避免撑破固定行高。
pub fn set_font_pct(pct: i32) {
    FONT_PCT.store(pct.clamp(80, 140), Ordering::Relaxed);
}

#[inline]
fn sc(px: i32) -> i32 {
    (px * FONT_PCT.load(Ordering::Relaxed) + 50) / 100
}

pub fn f_base() -> i32 {
    sc(F_BASE_PX)
}
pub fn f_sub() -> i32 {
    sc(F_SUB_PX)
}
pub fn f_tag() -> i32 {
    sc(F_TAG_PX)
}
pub fn f_big() -> i32 {
    sc(F_BIG_PX)
}
pub fn f_title() -> i32 {
    sc(F_TITLE_PX)
}

// —— 头部 ——
const HEAD_Y: i32 = 16;
const HEAD_H: i32 = 74;

// —— 卡片 ——
const CARD_Y: i32 = 96;
const CARD_H0: i32 = 454;
const LEFT_X: i32 = 24;
const RIGHT_W: i32 = 348;
const PAD: i32 = 18;

// —— 响应式：窗口变宽时左卡变宽、右卡贴右，字号与图形尺寸不变 ——
static BOX_W: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(W);
static BOX_H: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(H);

pub fn set_box(w: i32, h: i32) {
    BOX_W.store(w.max(W), std::sync::atomic::Ordering::Relaxed);
    BOX_H.store(h.max(H), std::sync::atomic::Ordering::Relaxed);
}
pub fn box_w() -> i32 {
    BOX_W.load(std::sync::atomic::Ordering::Relaxed)
}
pub fn box_h() -> i32 {
    BOX_H.load(std::sync::atomic::Ordering::Relaxed)
}
#[inline]
fn right_x() -> i32 {
    box_w() - 24 - RIGHT_W
}
#[inline]
fn left_w() -> i32 {
    right_x() - 24 - LEFT_X
}
#[inline]
fn l_w() -> i32 {
    left_w() - 2 * PAD
}
#[inline]
fn r_x() -> i32 {
    right_x() + PAD
}
#[inline]
fn x_days() -> i32 {
    L_X1 + l_w() - G_DAYS
}
#[inline]
fn x_model() -> i32 {
    x_days() - G_COLGAP - G_MODEL
}
#[inline]
fn x_name_r() -> i32 {
    x_model() - G_COLGAP
}
pub fn card_h() -> i32 {
    (box_h() - CARD_Y - 24).max(CARD_H0)
}
#[inline]
fn r_ch() -> i32 {
    let avail = card_h() - (R_GY0 - CARD_Y) - 42;
    (avail / 6).clamp(46, 58)
}

// —— 左卡内部：单列全宽固定栅格（不按内容自适应，命中矩形才稳定）——
// [勾选圈 17][间距 10][P0/P1 胶囊 34][间距 10][名称 flex][间距 12][机型 76 右对齐][间距 12][天数 46 右对齐]
const L_X1: i32 = LEFT_X + PAD; // 42
const G_CIRCLE: i32 = 17;
const G_GAP: i32 = 10;
const G_CHIP: i32 = 34;
const G_MODEL: i32 = 76;
const G_DAYS: i32 = 46;
const G_COLGAP: i32 = 12;
const X_CHIP: i32 = L_X1 + G_CIRCLE + G_GAP; // 69
const X_NAME: i32 = X_CHIP + G_CHIP + G_GAP; // 113
const L_TITLE_Y: i32 = 110;
const L_TITLE_H: i32 = 28;
const L_DIV_Y: i32 = 146;
const L_LIST_TOP: i32 = 158;
pub const L_ROW_H: i32 = 32;
pub const L_ROWS_PROJ: usize = 7;

/// 部门/归档列表当前窗口高度下真正能放下的行数（底部预留 24px 给「另有 N 条」提示）
pub fn l_rows() -> usize {
    // 底部预留：未读区块 176px（分隔线 + 标题 + 3 行 + 时间戳）+ 提示行 24px
    let h = (card_bottom() - 176 - L_LIST_TOP - 24).max(0);
    ((h / L_ROW_H).max(1)) as usize
}
/// 归档视图固定 9 行（超出靠滚轮，外层矩形/命中坐标不变）
pub const L_ROWS_ARC: usize = 9;
// 「未读消息」固定 96px：分隔线 390 + 标题 20 + 3 行 × 22
#[inline]
fn l_sig_div_y() -> i32 {
    card_bottom() - 176
}
#[inline]
fn l_sig_title_y() -> i32 {
    card_bottom() - 166
}
const L_SIG_TITLE_H: i32 = 20;
#[inline]
fn l_sig_top() -> i32 {
    card_bottom() - 146
}
pub const L_SIG_H: i32 = 22;

#[inline]
fn l_stamp_y() -> i32 {
    card_bottom() - 44
}
#[inline]
fn card_bottom() -> i32 {
    CARD_Y + card_h()
}

// —— 右卡月历 ——
const R_TITLE_Y: i32 = 110;
const R_TITLE_H: i32 = 28;
fn r_prev() -> (i32, i32) { (right_x() + RIGHT_W - 44, 112) } // 命中区左上
fn r_next() -> (i32, i32) { (right_x() + RIGHT_W - 20, 112) }
const R_ARROW_CY: i32 = 124;
const R_WEEK_Y: i32 = 146;
const R_WEEK_H: i32 = 18;
const R_CW: f32 = 312.0 / 7.0;
const R_GY0: i32 = 158; // 日历首行顶（原 168，底行会被卡片下沿裁掉）
const R_CH: i32 = 54; // 行高（原 58）：6 行 54×6=324，158+324=482 稳在卡片内

/// 数据层的 prio 是 "P0"/"P1"/"P2" 字符串（t2 契约）。
pub fn prio_color_s(p: &str) -> Rgba {
    match p {
        "P0" => P0C,
        "P1" => P1C,
        "P2" => P2C,
        _ => INK3,
    }
}

pub fn prio_label_s(p: &str) -> &'static str {
    match p {
        "P0" => "P0",
        "P1" => "P1",
        "P2" => "P2",
        _ => "--",
    }
}

#[allow(dead_code)]
pub fn prio_color(p: u8) -> Rgba {
    match p {
        0 => P0C,
        1 => P1C,
        2 => P2C,
        _ => INK3,
    }
}

#[allow(dead_code)]
fn prio_label(p: u8) -> &'static str {
    match p {
        0 => "P0",
        1 => "P1",
        2 => "P2",
        _ => "--",
    }
}

#[allow(clippy::too_many_arguments)]
fn band_text(
    cv: &mut Canvas,
    te: &mut TextEngine,
    text: &str,
    x: i32,
    by: i32,
    bh: i32,
    px: i32,
    weight: i32,
    c: Rgba,
) -> i32 {
    let (w, h) = te.measure(text, px, weight);
    te.draw(cv, text, x, by + (bh - h) / 2, px, weight, c);
    w
}

#[allow(clippy::too_many_arguments)]
fn band_center(
    cv: &mut Canvas,
    te: &mut TextEngine,
    text: &str,
    cx: i32,
    by: i32,
    bh: i32,
    px: i32,
    weight: i32,
    c: Rgba,
) {
    let (w, h) = te.measure(text, px, weight);
    te.draw(cv, text, cx - w / 2, by + (bh - h) / 2, px, weight, c);
}

fn card(cv: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
    let (r, g, b) = tint_rgb();
    cv.rrect(
        x as f32,
        y as f32,
        w as f32,
        h as f32,
        14.0,
        (r.saturating_add(16), g.saturating_add(18), b.saturating_add(22), 100),
    );
    cv.rrect_stroke(x as f32, y as f32, w as f32, h as f32, 14.0, 1.0, (255, 255, 255, 16));
}

/// 静止层（chrome）：面板 + 头部 + 两张卡片底框。跟标签/月份无关，
/// 只在这里画一次，脏区重画时用 copy_rect_from 回填，避免半透明底二次混合
/// （二次混合会让边缘像素漂移，A07 翻月往返截图哈希就是因此对不上）。
pub fn render_chrome(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits) {
    hits.clear();
    panel(cv, hits);
    header(cv, te, v, hits);
    card(cv, LEFT_X, CARD_Y, left_w(), card_h());
    card(cv, right_x(), CARD_Y, RIGHT_W, card_h());
    hits.push(Kind::Static, "card:left", LEFT_X, CARD_Y, left_w(), card_h());
    hits.push(Kind::Static, "card:right", right_x(), CARD_Y, RIGHT_W, card_h());
}

/// 兼容旧调用：chrome + 标签栏。
pub fn render_bg(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits) {
    render_chrome(cv, te, v, hits);
    draw_tabs(cv, te, v, hits);
    draw_emstat(cv, te, v, hits);
    draw_gear(cv, te, hits);
}

/// 左卡内容区（脏区重画）：调用前调用方已把该矩形回填成面板底色。
pub fn render_left(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits) {
    hits.retain_outside_keep_container(LEFT_X, CARD_Y, left_w(), card_h());
    match v.tab {
        Tab::Proj => left_card_proj(cv, te, v, hits),
        Tab::Dept => left_card_dept(cv, te, v, hits),
        Tab::Meet => left_card_meet(cv, te, v, hits),
    }
}

/// 右卡内容区（翻月专用脏区）：只重登记右卡内的命中区。
pub fn render_right(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits) {
    hits.retain_outside_keep_container(right_x(), CARD_Y, RIGHT_W, card_h());
    right_card(cv, te, v, hits);
}

/// 整屏绘制（首次出图 / 数据热重载后）。
pub fn render(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits) {
    render_bg(cv, te, v, hits);
    render_left(cv, te, v, hits);
    render_right(cv, te, v, hits);
    // 附件悬停预览：在内容之上、模态窗之下
    if !v.modal.open {
        draw_attachment_hover(cv, te, v, hits);
    }
    // 模态窗 z 序最高：遮罩 + 面板压在所有内容之上
    draw_modal(cv, te, v, hits);
    draw_settings(cv, te, v, hits);
    draw_remind(cv, te, v, hits);
}

fn panel(cv: &mut Canvas, hits: &mut Hits) {
    cv.rrect(0.0, 0.0, box_w() as f32, box_h() as f32, 18.0, panel_bg());
    cv.rrect_stroke(0.5, 0.5, box_w() as f32 - 1.0, box_h() as f32 - 1.0, 18.0, 1.2, (255, 255, 255, 22));
    for i in 0..8 {
        let a = (10 - i) as u8;
        cv.hline(120 + i * 30, 1, 740 - i * 60, (255, 255, 255, a));
    }
    // 兜底拖拽区：整屏。后面的控件与卡片会盖在它上面。
    hits.push(Kind::DragBlank, "panel", 0, 0, box_w(), box_h());
}

fn header(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits) {
    let d: &Data = v.d;
    let dn = d.today_day.to_string();
    let (nw, nh) = te.measure(&dn, f_big(), 700);
    te.draw(cv, &dn, 24, HEAD_Y + (HEAD_H - nh) / 2, f_big(), 700, INK);
    hits.push(Kind::Chip, "head:day", 20, HEAD_Y + 8, nw + 8, HEAD_H - 16);

    let (_, h13) = te.measure(&d.week, f_base(), 500);
    let (_, h11) = te.measure(&d.ym, f_sub(), 400);
    let block = h13 + 2 + h11;
    let y0 = HEAD_Y + (HEAD_H - block) / 2;
    let tx = 24 + nw + 14;
    te.draw(cv, &d.week, tx, y0, f_base(), 500, INK2);
    te.draw(cv, &d.ym, tx, y0 + h13 + 2, f_sub(), 400, INK3);
    let (ww, _) = te.measure(&d.ym, f_sub(), 400);
    hits.push(Kind::Chip, "head:week", tx - 2, y0 - 2, ww + 6, block + 4);

    let chips: [(&str, Option<Rgba>, &str); 3] = [
        (&format!("近7天 {} 节点", d.counts.nodes7), Some(ACC), "chip:n7"),
        (&format!("P0 {}", d.counts.p0), Some(P0C), "chip:p0"),
        (&format!("在管 {}", d.counts.projects), None, "chip:proj"),
    ];
    let ch = 24;
    let cy = HEAD_Y + (HEAD_H - ch) / 2;
    let mut right = box_w() - 24;
    for (label, dot, id) in chips {
        let (tw, th) = te.measure(label, f_tag(), 500);
        let dotw = if dot.is_some() { 11 } else { 0 };
        let cw = tw + dotw + 20;
        let x = right - cw;
        cv.rrect(x as f32, cy as f32, cw as f32, ch as f32, 12.0, (255, 255, 255, 15));
        cv.rrect_stroke(x as f32, cy as f32, cw as f32, ch as f32, 12.0, 1.0, (255, 255, 255, 12));
        let mut tx = x + 10;
        if let Some(c) = dot {
            cv.circle((tx + 3) as f32, (cy + ch / 2) as f32, 3.0, c);
            tx += 11;
        }
        te.draw(cv, label, tx, cy + (ch - th) / 2, f_tag(), 500, INK2);
        hits.push(Kind::Chip, id, x, cy, cw, ch);
        right = x - 8;
    }
}


/// 未读消息区块（分隔线 + 标题 + 最多 3 行 + 空态），供三只标签共用。
/// 底部数据时间戳：三张卡片共用同一处绘制，避免同一行被重复叠加导致偏亮。
fn footer_stamp(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits) {
    let st = format!("数据 {} 更新", v.d.stamp);
    let sw = band_text(cv, te, &st, L_X1, l_stamp_y(), 22, f_tag(), 400, (111, 118, 134, 150));
    hits.push(Kind::Text, "stamp", L_X1, l_stamp_y(), sw, 22);
}

fn unread_block(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits) {
    cv.hline(L_X1, l_sig_div_y(), l_w(), (255, 255, 255, 10));
    let stw = band_text(cv, te, "未读消息", L_X1, l_sig_title_y(), L_SIG_TITLE_H, f_sub(), 600, INK3);
    hits.push(Kind::Text, "sig:title", L_X1, l_sig_title_y(), stw, L_SIG_TITLE_H);
    if v.signals.is_empty() {
        cv.circle((L_X1 + 4) as f32, (l_sig_top() + L_SIG_H / 2) as f32, 4.0, (58, 65, 82, 255));
        band_text(
            cv,
            te,
            "没有未读消息",
            L_X1 + 14,
            l_sig_top(),
            L_SIG_H,
            f_sub(),
            400,
            (111, 118, 134, 150),
        );
    }
    for i in 0..v.signals.len().min(3) {
        let s = &v.signals[i];
        let y = l_sig_top() + i as i32 * L_SIG_H;
        // 来源角标：企微(绿) / EM(蓝) + 群/私，未带来源的旧数据不画
        let mut ox = L_X1;
        if !s.src.is_empty() {
            let tag = format!(
                "{}·{}",
                s.src,
                if s.kind == "direct" { "私" } else { "群" }
            );
            let col = if s.src == "企微" {
                (86, 200, 150, 225)
            } else {
                (110, 170, 255, 225)
            };
            let bw = band_text(cv, te, &tag, ox, y, L_SIG_H, f_tag(), 600, col);
            ox += bw + 8;
        }
        // 顺序：来源角标 · 群名 · 发言人 · 内容；时间右对齐放在行尾
        if !s.conv.is_empty() {
            let label = format!("@{}", s.conv);
            let cw = band_text(cv, te, &label, ox, y, L_SIG_H, f_tag(), 500, (139, 176, 210, 225));
            ox += cw + 8;
        }
        let sx = ox;
        let sw = band_text(cv, te, &s.sender, sx, y, L_SIG_H, f_sub(), 500, INK2);
        let nx = sx + sw + 10;
        // 行尾时间（灰、右对齐）
        let (tw2, _) = te.measure(&s.t, f_tag(), 400);
        let tx = (L_X1 + l_w() - tw2).max(nx + 60);
        band_text(cv, te, &s.t, tx, y, L_SIG_H, f_tag(), 400, (111, 118, 134, 190));
        let avail = (tx - nx - 10).max(40);
        let body = te.fit(&s.text, avail, f_sub(), 400);
        band_text(cv, te, &body, nx, y, L_SIG_H, f_sub(), 400, (139, 147, 164, 235));
        hits.push(Kind::Text, format!("sig:{}", i), L_X1, y, l_w(), L_SIG_H);
    }


    // 采集状态（t7）：抓取中… / 上次抓取 … · 下次 …
    if let Some((text, running)) = crate::pull::footer_text() {
        let (tw, _) = te.measure(&text, f_tag(), 400);
        let stw2 = te.measure(&format!("数据 {} 更新", v.d.stamp), f_tag(), 400).0;
        let tx = (L_X1 + l_w() - tw).max(L_X1 + stw2 + 14);
        let col = if running { ACC } else { (111, 118, 134, 150) };
        band_text(cv, te, &text, tx, l_stamp_y(), 22, f_tag(), 400, col);
        hits.push(Kind::Text, "pull", tx, l_stamp_y(), tw, 22);
    }}

fn left_card_proj(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits) {

    // 标题行：抓取待办（标签一只读，没有「我的待办」列，也没有 ＋）
    let tw = band_text(cv, te, "抓取待办", L_X1, L_TITLE_Y, L_TITLE_H, f_title(), 600, INK);
    hits.push(Kind::Text, "title:todo", L_X1, L_TITLE_Y, tw, L_TITLE_H);
    let cnt_txt = if v.d.todo.len() > L_ROWS_PROJ {
        format!("{} · 显示前 {}", v.d.todo.len(), L_ROWS_PROJ)
    } else {
        format!("{}", v.d.todo.len())
    };
    let cx = L_X1 + tw + 8;
    let cw2 = band_text(cv, te, &cnt_txt, cx, L_TITLE_Y, L_TITLE_H, f_sub(), 500, (111, 118, 134, 200));
    hits.push(Kind::Text, "title:todo:count", cx, L_TITLE_Y, cw2, L_TITLE_H);

    cv.hline(L_X1, L_DIV_Y, l_w(), (255, 255, 255, 14));

    // 抓取待办：固定 7 行（158 + 7×32 = 382，正好落在信号区分隔线 390 之上）
    for i in 0..L_ROWS_PROJ {
        let y = L_LIST_TOP + i as i32 * L_ROW_H;
        if let Some(t) = v.d.todo.get(i) {
            todo_row(cv, te, v, hits, L_X1, y, i, t);
        }
    }
    if v.d.todo.is_empty() {
        band_text(cv, te, "今日无待办", L_X1, L_LIST_TOP, L_ROW_H, f_base(), 400, INK3);
    }

    // 未读消息：三只标签共用同一区块（数据同源，项目/部门/会议都显示）
    unread_block(cv, te, v, hits);
    footer_stamp(cv, te, v, hits);
}

/// 行尾剩余天数文案 + 颜色：逾期 N 天（红）> 今天（橙）> 明天（橙淡）> N 天（灰）。
fn due_text(dd: i32) -> (String, Rgba) {
    if dd < 0 {
        (format!("逾期{}天", -dd), P0C)
    } else if dd == 0 {
        ("今天".to_string(), ACC)
    } else if dd == 1 {
        ("明天".to_string(), (255, 176, 92, 255))
    } else {
        (format!("{}天", dd), INK3)
    }
}

fn circle_mark(cv: &mut Canvas, cx: i32, cy: i32, done: bool) {
    if done {
        cv.circle(cx as f32, cy as f32, 8.0, (255, 159, 67, 130));
        cv.circle_stroke(cx as f32, cy as f32, 8.0, 1.0, (255, 159, 67, 210));
        // 勾：两段短线
        cv.rrect(cx as f32 - 4.0, cy as f32, 3.2, 1.3, 0.65, (255, 255, 255, 235));
        cv.rrect(cx as f32 - 1.6, cy as f32 + 1.0, 1.5, 1.3, 0.65, (255, 255, 255, 235));
        cv.rrect(cx as f32 - 1.0, cy as f32 + 2.6, 6.0, 1.3, 0.65, (255, 255, 255, 235));
    } else {
        cv.circle_stroke(cx as f32, cy as f32, 8.0, 1.4, (255, 255, 255, 70));
    }
}

#[allow(clippy::too_many_arguments)]
fn todo_row(
    cv: &mut Canvas,
    te: &mut TextEngine,
    v: &View,
    hits: &mut Hits,
    x: i32,
    y: i32,
    idx: usize,
    t: &Todo,
) {
    let cy = y + L_ROW_H / 2;
    let key = v.key_of_todo(idx);
    let done = v.is_done(&key);
    hits.push(Kind::Row, format!("todo:{}", idx), x - 4, y, l_w() + 8, L_ROW_H);

    // 列 0：勾选圈（17 宽，圆圈居中）
    let ccx = x + G_CIRCLE / 2;
    circle_mark(cv, ccx, cy, done);
    hits.push(Kind::Circle, format!("todo:{}", idx), ccx - 10, cy - 10, 20, 20);

    // 列 1：优先级胶囊（固定 34 宽，文字居中）
    let label = prio_label_s(&t.prio);
    let pc = prio_color_s(&t.prio);
    let chip_x = x + (X_CHIP - L_X1);
    cv.rrect(chip_x as f32, (cy - 8) as f32, G_CHIP as f32, 15.0, 7.5, (pc.0, pc.1, pc.2, 44));
    let (tw, th) = te.measure(label, f_tag(), 600);
    te.draw(cv, label, chip_x + (G_CHIP - tw) / 2, cy - th / 2, f_tag(), 600, pc);
    hits.push(Kind::Tag, format!("todo:{}", idx), chip_x, cy - 9, G_CHIP, 18);

    // 列 2：名称（flex，截断）
    let name_x = x + (X_NAME - L_X1);
    let name_r = x + (x_name_r() - L_X1);
    let name = te.fit(&t.name, (name_r - name_x).max(24), f_base(), 400);
    let (nw, nh) = te.measure(&name, f_base(), 400);
    let nc = if done { INK3 } else { INK };
    te.draw(cv, &name, name_x, cy - nh / 2, f_base(), 400, nc);
    if done {
        cv.hline(name_x, cy, nw, (111, 118, 134, 150));
    }
    hits.push(Kind::Text, format!("todo:{}", idx), name_x, cy - 9, nw, 18);

    // 列 3：机型（76 宽，右对齐，完整显示不截断；空则跳过）
    if !t.model.is_empty() {
        let model_x = x + (x_model() - L_X1);
        let m = te.fit(&t.model, G_MODEL, f_sub(), 400);
        let (mw, mh) = te.measure(&m, f_sub(), 400);
        let mc = if done { (95, 102, 116, 200) } else { (128, 136, 152, 235) };
        te.draw(cv, &m, model_x + G_MODEL - mw, cy - mh / 2, f_sub(), 400, mc);
        hits.push(Kind::Text, format!("todo:{}:model", idx), model_x, cy - 9, G_MODEL, 18);
    }

    // 列 4：剩余天数（46 宽，右对齐）
    let (dd_txt, dc) = due_text(t.dd);
    let (dw, dh) = te.measure(&dd_txt, f_sub(), 500);
    let days_x = x + (x_days() - L_X1) + G_DAYS - dw;
    te.draw(cv, &dd_txt, days_x, cy - dh / 2, f_sub(), 500, dc);
    hits.push(Kind::Text, format!("todo:{}:due", idx), days_x, cy - 9, dw, 18);
}

#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
fn custom_row(
    cv: &mut Canvas,
    te: &mut TextEngine,
    v: &View,
    hits: &mut Hits,
    x: i32,
    y: i32,
    w: i32,
    idx: usize,
    text: &str,
    due: &str,
    done_flag: bool,
) {
    let cy = y + L_ROW_H / 2;
    let key = v.key_of_custom(idx);
    let done = done_flag || v.is_done(&key);
    hits.push(Kind::Row, format!("mine:{}", idx), x - 4, y, w + 8, L_ROW_H);

    let ccx = x + 9;
    circle_mark(cv, ccx, cy, done);
    hits.push(Kind::Circle, format!("mine:{}", idx), ccx - 10, cy - 10, 20, 20);

    let nx = x + 26;
    let dw = if due.is_empty() { 0 } else { 44 };
    let avail = (x + w - dw - nx).max(24);
    let body = te.fit(text, avail, f_base(), 400);
    let (bw, bh) = te.measure(&body, f_base(), 400);
    let nc = if done { (111, 118, 134, 255) } else { INK };
    te.draw(cv, &body, nx, cy - bh / 2, f_base(), 400, nc);
    if done {
        cv.hline(nx, cy, bw, (111, 118, 134, 150));
    }
    hits.push(Kind::Text, format!("mine:{}", idx), nx, cy - 9, bw, 18);

    if !due.is_empty() {
        let (dw2, dhh) = te.measure(due, f_sub(), 500);
        te.draw(cv, due, x + w - dw2, cy - dhh / 2, f_sub(), 500, INK3);
    }
}

fn right_card(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits) {

    let title = format!("{} 年 {} 月", v.y, v.mo);
    let tw = band_text(cv, te, &title, r_x(), R_TITLE_Y, R_TITLE_H, f_title(), 600, INK);
    hits.push(Kind::Text, "cal:title", r_x(), R_TITLE_Y, tw, R_TITLE_H);

    // 翻月三角：20×24 的可点矩形
    let (px_, py_) = r_prev();
    let (nx_, ny_) = r_next();
    cv.triangle((px_ + 10) as f32, R_ARROW_CY as f32, 9.0, 1, (255, 255, 255, 95));
    cv.triangle((nx_ + 10) as f32, R_ARROW_CY as f32, 9.0, -1, (255, 255, 255, 95));
    hits.push(Kind::MonthPrev, "month:prev", px_, py_, 20, 24);
    hits.push(Kind::MonthNext, "month:next", nx_, ny_, 20, 24);

    // 星期表头
    let names = ["一", "二", "三", "四", "五", "六", "日"];
    for (i, n) in names.iter().enumerate() {
        let cx = (r_x() as f32 + R_CW * (i as f32 + 0.5)).round() as i32;
        band_center(cv, te, n, cx, R_WEEK_Y, R_WEEK_H, f_tag(), 500, INK3);
    }
    hits.push(Kind::DragCalNonDay, "cal:week", r_x() - 4, R_WEEK_Y - 2, 320, R_WEEK_H + 4);

    let pad = v.start_pad();
    let days_in = v.days_in() as i32;
    let today = v.today_day();

    for slot in 0..42 {
        let col = slot % 7;
        let rowi = slot / 7;
        let x0 = (r_x() as f32 + R_CW * col as f32).round() as i32;
        let x1 = (r_x() as f32 + R_CW * (col + 1) as f32).round() as i32;
        let y0 = R_GY0 + r_ch() * rowi;
        let day = slot - pad + 1;

        if day < 1 || day > days_in {
            hits.push(Kind::DragCalNonDay, format!("cal:pad:{}", slot), x0, y0, x1 - x0, R_CH);
            continue;
        }

        hits.push(Kind::DayCell, format!("day:{}", v.iso(day)), x0, y0, x1 - x0, R_CH);

        let cxf = x0 as f32;
        let cyf = y0 as f32;
        let is_today = day == today;
        let is_sel = day == v.sel_day;
        let weekend = col >= 5;

        if is_today {
            cv.rrect(cxf + 3.0, cyf + 4.0, (x1 - x0) as f32 - 6.0, 26.0, 9.0, (255, 159, 67, 34));
            cv.rrect_stroke(
                cxf + 3.0,
                cyf + 4.0,
                (x1 - x0) as f32 - 6.0,
                26.0,
                8.0,
                1.35,
                (255, 159, 67, 120),
            );
        } else if is_sel {
            cv.rrect(cxf + 3.0, cyf + 4.0, (x1 - x0) as f32 - 6.0, 26.0, 9.0, (255, 255, 255, 14));
            cv.rrect_stroke(
                cxf + 3.0,
                cyf + 4.0,
                (x1 - x0) as f32 - 6.0,
                26.0,
                8.0,
                1.35,
                (255, 255, 255, 60),
            );
        }

        let mid = (x0 + x1) / 2;
        let num = day.to_string();
        let nw = if is_today { 700 } else { 400 };
        let (bl2, bt2, num_w, num_h) = te.ink_box(&num, f_base(), nw);
        let chip_cy = cyf + 17.0;
        let ncol = if is_today {
            ACC
        } else if weekend {
            INK2
        } else {
            INK
        };
        let nx0 = mid - (bl2 + num_w / 2);
        let ny0 = chip_cy.round() as i32 - (bt2 + num_h / 2);
        te.draw(cv, &num, nx0, ny0, f_base(), nw, ncol);

        // 农历 / 节气
        if let Some(lab) = lunar::label(v.y, v.mo, day as u32) {
            let lc = if is_today {
                (255, 159, 67, 235)
            } else if lunar::jieqi(v.y, v.mo, day as u32).is_some() {
                (255, 191, 71, 210)
            } else {
                (111, 118, 134, 205)
            };
            band_center(cv, te, &lab, mid, y0 + 33, 12, f_tag(), 400, lc);
        }

        // 节点点（最多 3 个）：标签一取项目节点、标签二取待办 due 日
        let dots = v.day_dots(day);
        if !dots.is_empty() {
            let n = dots.len().min(3);
            let gap = 7.0;
            let total = gap * (n as f32 - 1.0);
            let mut dx = mid as f32 - total * 0.5;
            for d in dots.iter().take(n) {
                cv.circle(dx, y0 as f32 + 50.5, 1.5, *d);
                dx += gap;
            }
        }
    }

    // 图例
    let mut lx = r_x() as f32;
    for (lab, c) in [("P0", P0C), ("P1", P1C), ("P2", P2C)] {
        cv.circle(lx + 3.0, (card_bottom() - 36) as f32, 3.0, c);
        let (w, _) = te.measure(lab, f_tag(), 500);
        te.draw(cv, lab, (lx + 9.0) as i32, card_bottom() - 42, f_tag(), 500, INK3);
        lx += 9.0 + w as f32 + 16.0;
    }
}

// —— 标签栏：segmented 两段，选中 12/600 橙字 + 亮底 + 2px 下划线 ——
const TAB_X: i32 = 300;
const TAB_Y: i32 = 40;
const TAB_W: i32 = 246;
const TAB_H: i32 = 26;
const TAB_SEG_W: i32 = 80;

pub fn render_tabs(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits) {
    draw_tabs(cv, te, v, hits);
    draw_emstat(cv, te, v, hits);
    draw_gear(cv, te, hits);
}

/// E-Mobile 联通状态灯：齿轮左侧 8px，绿/黄/红三态；点击展开说明浮层。
fn draw_emstat(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits) {
    // 紧挨齿轮（齿轮中心 = box_w-22），间距 26px，跟着窗口宽度走
    let cx = (box_w() as f32) - 48.0;
    let cy = 18.0f32;
    let col = match v.emstat {
        1 => (65, 186, 82, 255),
        2 => (255, 194, 71, 255),
        3 => (255, 107, 97, 255),
        _ => (120, 128, 140, 255),
    };
    cv.circle(cx, cy, 3.2, col);
    hits.push(Kind::Static, "ui:emstat", (cx as i32) - 12, 4, 24, 24);
    if v.em_tip {
        let (w, h) = (248, 54);
        let (x, y) = ((cx as i32) - 200, 40);
        cv.rrect(x as f32, y as f32, w as f32, h as f32, 10.0, (24, 27, 34, 250));
        cv.rrect_stroke(x as f32, y as f32, w as f32, h as f32, 10.0, 1.0, (255, 255, 255, 26));
        let e = &v.d.health.emobile;
        let (l1, l2) = match v.emstat {
            1 => (
                format!(
                    "已连接 · 上次抓取 {} 分钟前 · {} 条（周期 {} 分钟）",
                    e.age_min, e.msgs, e.interval_min
                ),
                format!("{}:{}", e.host, e.port),
            ),
            2 => (
                format!(
                    "抓取正常但数据滞后 {} 分钟（周期 {} 分钟）",
                    e.age_min, e.interval_min
                ),
                format!("{}:{}", e.host, e.port),
            ),
            _ => (
                if e.live_err.is_empty() {
                    "未连接 · 上一轮抓取失败或已超两个周期".to_string()
                } else {
                    format!("未连接 · {}", e.live_err)
                },
                if e.at.is_empty() {
                    "还没有成功记录".to_string()
                } else {
                    format!("上次成功 {}", e.at)
                },
            ),
        };
        let s1 = te.fit(&l1, w - 20, 12, 500);
        let s2 = te.fit(&l2, w - 20, 11, 400);
        te.draw(cv, &s1, x + 10, y + 9, 12, 500, INK);
        te.draw(cv, &s2, x + 10, y + 30, 11, 400, INK3);
        hits.push(Kind::Static, "ui:emstat:tip", x, y, w, h);
    }
}

/// 右上角设置齿轮；固定位置开启时左侧多一个小锁
fn draw_gear(cv: &mut Canvas, _te: &mut TextEngine, hits: &mut Hits) {
    // 齿轮：抗锯齿圆环 + 8 个圆齿（不再用 3×3 像素块，避免放大后发糊）
    let cx = (box_w() - 22) as f32;
    let cy = 18.0f32;
    let hov = GEAR_HOVER.load(std::sync::atomic::Ordering::Relaxed);
    let col = if hov {
        (232, 238, 250, 248)
    } else {
        (150, 158, 175, 185)
    };
    if hov {
        cv.circle(cx, cy, 12.0, (255, 255, 255, 16));
    }
    let teeth = 8;
    for i in 0..teeth {
        let a = i as f32 * std::f32::consts::TAU / teeth as f32;
        cv.circle(cx + a.cos() * 5.4, cy + a.sin() * 5.4, 1.55, col);
    }
    cv.circle_stroke(cx, cy, 3.9, 2.1, col);
    if LOCKED.load(std::sync::atomic::Ordering::Relaxed) {
        cv.rrect(cx - 20.0, 12.0, 9.0, 7.0, 2.0, (255, 194, 71, 200));
        cv.circle_stroke(cx - 15.5, 12.0, 3.0, 1.1, (255, 194, 71, 200));
    }
    hits.push(
        Kind::Static,
        "ui:settings",
        (cx as i32) - 16,
        2,
        32,
        32,
    );
}

/// 设置面板（与新建待办同一套模态语言，z 序更高）
pub fn draw_settings(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits) {
    let f = &v.set;
    if !f.open {
        return;
    }
    const PW: i32 = 520; // 设置面板宽
    const PH: i32 = 300;
    let px = (box_w() - PW) / 2;
    let py = (box_h() - PH) / 2;
    cv.rect(0, 0, box_w(), box_h(), (0, 0, 0, 130));
    hits.push(Kind::Static, "set:backdrop", 0, 0, box_w(), box_h());
    cv.rrect(px as f32, py as f32, PW as f32, PH as f32, 14.0, (18, 20, 26, 250));
    cv.rrect_stroke(px as f32, py as f32, PW as f32, PH as f32, 14.0, 1.0, (255, 255, 255, 22));
    hits.push(Kind::Static, "set:panel", px, py, PW, PH);
    te.draw(cv, "设置", px + 24, py + 18, f_base() + 3, 600, INK);
    te.draw(cv, "外观与行为", px + 24, py + 40, f_tag(), 400, INK3);

    let lx = px + 24;
    let cx0 = px + 150;

    // 1) 不透明度 40%–100%，步进 5%
    te.draw(cv, "不透明度", lx, py + 66, f_tag() + 1, 500, INK3);
    let tx = cx0;
    let tw = 260;
    cv.rrect(tx as f32, (py + 73) as f32, tw as f32, 4.0, 2.0, (255, 255, 255, 26));
    let k = ((f.opacity - 0.4) / 0.6).clamp(0.0, 1.0) as f32;
    let fillw = (tw as f32 * k) as i32;
    cv.rrect(tx as f32, (py + 73) as f32, fillw as f32, 4.0, 2.0, ACC);
    cv.circle((tx + fillw) as f32, (py + 75) as f32, 6.0, (255, 255, 255, 240));
    cv.circle_stroke((tx + fillw) as f32, (py + 75) as f32, 6.0, 1.2, ACC);
    SET_SLIDER_X.store(tx, Ordering::Relaxed);
    SET_SLIDER_W.store(tw, Ordering::Relaxed);
    hits.push(Kind::Static, "set:opacity", tx - 8, py + 65, tw + 16, 22);
    te.draw(
        cv,
        &format!("{}%", (f.opacity * 100.0).round() as i32),
        tx + tw + 16,
        py + 67,
        f_sub() + 1,
        600,
        INK,
    );

    // 2) 尺寸改成拖窗口边角，这里只放一行提示
    te.draw(cv, "拖动窗口边角可调整大小", cx0, py + 118, f_tag(), 400, INK3);

    // 4) 固定位置
    te.draw(cv, "固定位置", lx, py + 158, f_tag() + 1, 500, INK3);
    let sx = cx0;
    let sy = py + 152;
    cv.rrect(sx as f32, sy as f32, 40.0, 22.0, 11.0, if f.locked { (255, 159, 67, 235) } else { (255, 255, 255, 22) });
    let knob = if f.locked { sx + 21 } else { sx + 3 };
    cv.circle((knob + 8) as f32, (sy + 11) as f32, 8.0, (250, 250, 252, 245));
    hits.push(Kind::Static, "set:locked", sx, sy, 40, 22);
    te.draw(cv, if f.locked { "已锁定，不可拖动" } else { "可拖动" }, sx + 52, sy + 5, f_tag() + 1, 400, INK3);

    // 底部按钮
    let by = py + PH - 46;
    for (x, id, label, primary) in [
        (px + PW - 154, "set:cancel", "取消", false),
        (px + PW - 78, "set:save", "保存", true),
    ] {
        if primary {
            cv.rrect(x as f32, by as f32, 62.0, 28.0, 8.0, ACC);
        } else {
            cv.rrect(x as f32, by as f32, 62.0, 28.0, 8.0, (255, 255, 255, 14));
        }
        let (tw3, th3) = te.measure(label, f_sub() + 1, 600);
        te.draw(
            cv,
            label,
            x + (62 - tw3) / 2,
            by + (28 - th3) / 2,
            f_sub() + 1,
            600,
            if primary { (20, 24, 30, 255) } else { INK2 },
        );
        hits.push(Kind::Static, id, x, by, 62, 28);
    }
}

/// 解析 "1. 文本" 这种编号行，返回 ("1.", "文本")
pub const MODAL_W: i32 = 560;
pub const MODAL_H: i32 = 380;
const M_LABEL_W: i32 = 56;
const M_ROW1: i32 = 46;
const M_ROW2: i32 = 93;
const M_ROW3: i32 = 132;
const M_ROW4: i32 = 232;
const M_ROW5: i32 = 273;
const M_BTN_Y: i32 = 330;

fn modal_label(cv: &mut Canvas, te: &mut TextEngine, text: &str, x: i32, y: i32, h: i32) {
    let (_, th) = te.measure(text, f_tag() + 1, 400);
    te.draw(cv, text, x, y + (h - th) / 2, f_tag() + 1, 400, INK3);
}

#[allow(clippy::too_many_arguments)]
fn modal_pill(cv: &mut Canvas, x: i32, y: i32, w: i32, h: i32, on: bool, c: Rgba) {
    if on {
        cv.rrect(x as f32, y as f32, w as f32, h as f32, 7.0, (c.0, c.1, c.2, 235));
    } else {
        cv.rrect(x as f32, y as f32, w as f32, h as f32, 7.0, (c.0, c.1, c.2, 20));
        cv.rrect_stroke(x as f32, y as f32, w as f32, h as f32, 7.0, 1.0, (c.0, c.1, c.2, 80));
    }
}

/// 附件悬停预览：鼠标停在带附件的行上时，画缩略图 / 类型卡 + 文件名
/// 鼠标悬停在带备注的待办行上：弹出备注浮窗（宽 300，最多 10 行）。
pub fn draw_note_hover(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &Hits) {
    let (mx, my) = v.mouse;
    if mx < 0 || my < 0 { return; }
    let mut found: Option<String> = None;
    for h in hits.v.iter().rev() {
        if h.kind == Kind::Row && mx >= h.x && mx < h.x + h.w && my >= h.y && my < h.y + h.h {
            if let Some((prefix, rest)) = h.id.split_once(':') {
                if let Ok(i) = rest.parse::<usize>() {
                    let row = match prefix { "dept" => v.dept.get(i), "custom" => v.custom.get(i), _ => None };
                    if let Some(r) = row { if !r.note.trim().is_empty() { found = Some(r.note.clone()); } }
                }
            }
            break;
        }
    }
    let Some(note) = found else { return };
    let pw = 300i32; let pad = 12i32; let tw = pw - pad * 2; let px = 13;
    let mut lines: Vec<String> = Vec::new();
    for raw in note.split('\n') {
        let mut cur = String::new(); let mut w = 0;
        for ch in raw.chars() {
            let cw = te.advance(ch, px, 400).max(1);
            if w + cw > tw && !cur.is_empty() { lines.push(cur.clone()); cur.clear(); w = 0; }
            cur.push(ch); w += cw;
        }
        lines.push(cur);
    }
    if lines.len() > 10 { lines.truncate(10); }
    let lh = 18i32; let ph = pad * 2 + lines.len() as i32 * lh;
    let mut x = mx + 16; let mut y = my + 18;
    if x + pw > box_w() - 8 { x = mx - pw - 16; }
    if y + ph > box_h() - 8 { y = my - ph - 12; }
    if x < 8 { x = 8; }
    if y < 8 { y = 8; }
    cv.rrect(x as f32, y as f32, pw as f32, ph as f32, 10.0, (24, 27, 35, 252));
    cv.rrect_stroke(x as f32, y as f32, pw as f32, ph as f32, 10.0, 1.0, (255, 255, 255, 34));
    for (i, ln) in lines.iter().enumerate() {
        te.draw(cv, ln, x + pad, y + pad + i as i32 * lh, px, 400, (226, 230, 238, 255));
    }
}
pub fn draw_attachment_hover(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits) {
    let (mx, my) = v.mouse;
    if mx < 0 || my < 0 {
        return;
    }
    let mut found: Option<String> = None;
    for h in hits.v.iter().rev() {
        if h.kind == Kind::Row && mx >= h.x && mx < h.x + h.w && my >= h.y && my < h.y + h.h {
            if let Some((prefix, rest)) = h.id.split_once(':') {
                if let Ok(i) = rest.parse::<usize>() {
                    let row = match prefix {
                        "dept" => v.dept.get(i),
                        "custom" => v.custom.get(i),
                        _ => None,
                    };
                    if let Some(r) = row {
                        if !r.img.is_empty() {
                            found = Some(r.img.clone());
                        }
                    }
                }
            }
            break;
        }
    }
    let Some(path) = found else { return };
    let file = path.rsplit(['\\', '/']).next().unwrap_or(&path).to_string();
    let is_img = crate::thumb::is_image(&path);
    let thumb = if is_img { crate::thumb::load(&path, 256) } else { None };

    let cw = 236;
    let pad = 10;
    let inner = cw - pad * 2;
    let (ph, chh) = if is_img { (130, 130) } else { (64, 64) };
    let total_h = pad + chh + 8 + 18 + 6 + 14 + pad;
    let mut cx = mx + 18;
    let mut cy = my + 12;
    if cx + cw > W - 10 {
        cx = mx - cw - 18;
    }
    if cy + total_h > H - 10 {
        cy = my - total_h - 12;
    }
    cx = cx.clamp(10, W - cw - 10);
    cy = cy.clamp(10, H - total_h - 10);

    cv.rrect(
        cx as f32,
        cy as f32,
        cw as f32,
        total_h as f32,
        10.0,
        (18, 20, 26, 246),
    );
    cv.rrect_stroke(
        cx as f32,
        cy as f32,
        cw as f32,
        total_h as f32,
        10.0,
        1.0,
        (255, 255, 255, 30),
    );

    let ix = cx + pad;
    let iy = cy + pad;
    if let Some(t) = thumb {
        let scale = (inner as f32 / t.w as f32).min(ph as f32 / t.h as f32).min(1.0);
        let dw = ((t.w as f32 * scale) as i32).max(1);
        let dh = ((t.h as f32 * scale) as i32).max(1);
        cv.rrect(ix as f32, iy as f32, inner as f32, ph as f32, 6.0, (255, 255, 255, 10));
        cv.blit(ix + (inner - dw) / 2, iy + (ph - dh) / 2, dw, dh, &t.px, t.w, t.h);
    } else {
        cv.rrect(ix as f32, iy as f32, inner as f32, chh as f32, 6.0, (255, 255, 255, 10));
        let g = attach_glyph(&path);
        let (gw, gh) = te.measure(g, 28, 700);
        te.draw(
            cv,
            g,
            ix + (inner - gw) / 2,
            iy + (chh - gh) / 2,
            28,
            700,
            (255, 194, 71, 230),
        );
    }

    let nx = ix;
    let nw = inner;
    let shown = te.fit(&file, nw, 12, 500);
    te.draw(cv, &shown, nx, iy + chh + 8, 12, 500, INK);
    let meta = format!(
        "{} · {}",
        if is_img { "图片" } else { "附件" },
        attach_size(&path)
    );
    te.draw(cv, &meta, nx, iy + chh + 8 + 18 + 4, 11, 400, INK3);
}

fn attach_size(path: &str) -> String {
    match std::fs::metadata(path) {
        Ok(m) => {
            let n = m.len();
            if n >= 1024 * 1024 {
                format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
            } else if n >= 1024 {
                format!("{} KB", n / 1024)
            } else {
                format!("{} B", n)
            }
        }
        Err(_) => "未找到".to_string(),
    }
}

/// 模态编辑框绘制 + 命中登记：绘制与命中同源（同一套坐标推 hits），不拆函数以免两者坐标漂移。
/// 分区：① 遮罩 `modal:backdrop` + 面板 `modal:panel`；
/// ② 标题/备注字段 `modal:field:0` `:0e` `:1`、富文本工具 `modal:nt.*`、优先级 `modal:prio:*`；
/// ③ 图片 `modal:img` `:img.clear` `:preview`；
/// ④ 截止日 `modal:due` `modal:due:*` 与内嵌日历 `modal:cal:prev|next|day:*|none|back`；
/// ⑤ 底部 `modal:cancel` / `modal:save`。命中统一写入 `hits`，由 `on_click` 的 `modal:*` 分支消费。
pub fn draw_modal(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits) {
    let m = &v.modal;
    if !m.open {
        return;
    }
    // 1) 遮罩：rect 走 alpha over，整窗压暗（fill_rect 是覆盖，会打穿桌面）
    cv.rect(0, 0, box_w(), box_h(), (0, 0, 0, 120));
    hits.push(Kind::Static, "modal:backdrop", 0, 0, box_w(), box_h());

    // 2) 面板：圆角 12，深底 + 1px 边（按当前窗口居中，命中和绘制同源）
    let pw = MODAL_W;
    let ph = MODAL_H;
    let px = (box_w() - pw) / 2;
    let py = (box_h() - ph) / 2;
    cv.rrect(px as f32, py as f32, pw as f32, ph as f32, 14.0, (18, 20, 26, 248));
    cv.rrect_stroke(
        px as f32 + 0.5,
        py as f32 + 0.5,
        pw as f32 - 1.0,
        ph as f32 - 1.0,
        14.0,
        1.0,
        (255, 255, 255, 22),
    );
    hits.push(Kind::Static, "modal:panel", px, py, pw, ph);

    let x0 = px + 20; // 230
    let cw = pw - 40; // 520
    let cx = x0 + M_LABEL_W; // 286：控件列左沿
    let cwid = cw - M_LABEL_W; // 464

    // 标题
    te.draw(cv, "新建待办", x0, py + 18, 16, 600, INK);

    // 事项
    let fh = 30;
    modal_label(cv, te, "事项", x0, py + M_ROW1, fh);
    {
        let iy = py + M_ROW1;
        cv.rrect(cx as f32, iy as f32, cwid as f32, fh as f32, 9.0, (255, 255, 255, 10));
        if m.field == 0 {
            cv.rrect_stroke(cx as f32 + 0.5, iy as f32 + 0.5, cwid as f32 - 1.0, fh as f32 - 1.0, 9.0, 1.2, ACC);
        }
        let ty = iy + (fh - 15) / 2;
        item_cells_clear();
        if m.text.is_empty() {
            te.draw(cv, "要做什么…", cx + 10, ty, f_base(), 400, INK3);
        } else {
            let mut x = cx + 10;
            for (bi, ch) in m.text.char_indices() {
                let w = te.advance(ch, f_base(), 400).max(1);
                if let Some((sa, sb)) = m.sel_t {
                    if bi >= sa && bi < sb {
                        cv.rect(x, ty - 1, w, 17, (255, 194, 71, 80));
                    }
                }
                item_cell_push(bi, bi + ch.len_utf8(), x, ty, w);
                let cs: String = m.text[bi..].chars().take(1).collect();
                te.draw(cv, &cs, x, ty, f_base(), 400, INK);
                x += w;
            }
            item_cell_push(m.text.len(), m.text.len(), x, ty, 2);
        }
        if m.field == 0 {
            let idx = m.caret_t.min(m.text.len());
            let (ccx, ccy) = item_caret_pos(idx).unwrap_or((cx + 10, ty));
            if CARET_ON.load(std::sync::atomic::Ordering::Relaxed) {
                cv.rect(ccx - 1, ccy, 2, 15, NT_BAR);
            }
        }
    }
    hits.push(Kind::Static, "modal:field:0", cx, py + M_ROW1, cwid, fh);
    // 事项行右侧的空白（输入框外、弹窗内）也算“点到末尾”，避免用户点右边没反应
    {
        let ex0 = cx + cwid;
        let ex1 = px + pw - 12;
        if ex1 > ex0 {
            hits.push(Kind::Static, "modal:field:0e", ex0, py + M_ROW1, ex1 - ex0, fh);
        }
    }
    // 校验失败的红字提示（例如空事项点保存）——必须让用户看到原因，不能静默
    if !m.err.is_empty() {
        // 红字提示画在事项输入框内右侧，既不压标签也不压按钮
        let (ew, eh2) = te.measure(&m.err, f_sub(), 400);
        te.draw(cv, &m.err, cx + cwid - ew - 6, py + M_ROW1 + (fh - eh2) / 2, f_sub(), 400, (255, 107, 97, 235));
    }

    // 优先级：P0 / P1 / P2
    let (prio_w, prio_h) = (56, 22);
    modal_label(cv, te, "优先级", x0, py + M_ROW2, prio_h);
    let prios = [("P0", (255, 107, 97, 255)), ("P1", (255, 194, 71, 255)), ("P2", (90, 169, 255, 255))];
    for (i, (p, c)) in prios.iter().enumerate() {
        let bx = cx + i as i32 * (prio_w + 8);
        let on = m.prio == *p;
        modal_pill(cv, bx, py + M_ROW2, prio_w, prio_h, on, *c);
        let (tw, th) = te.measure(p, f_sub() + 1, 600);
        te.draw(
            cv,
            p,
            bx + (prio_w - tw) / 2,
            py + M_ROW2 + (prio_h - th) / 2,
            f_sub() + 1,
            600,
            if on { (20, 24, 30, 255) } else { (c.0, c.1, c.2, 215) },
        );
        hits.push(
            Kind::Static,
            format!("modal:prio:{}", p),
            bx,
            py + M_ROW2,
            prio_w,
            prio_h,
        );
    }

    // 备注：多行 Markdown（# 标题 / - 列表 / 1. 编号 / **加粗**）
    let note_h = 68;
    let nt_y = py + M_ROW3 + 24;
    modal_label(cv, te, "备注", x0, py + M_ROW3 + 2, 20);
    // 迷你排版工具栏：加粗 / 标题 / 项目符号 / 编号
    for (i, (id, glyph)) in [
        ("modal:nt.bold", "B"),
        ("modal:nt.h", "H"),
        ("modal:nt.li", "•"),
        ("modal:nt.ol", "1."),
    ]
    .iter()
    .enumerate()
    {
        let bx = cx + i as i32 * 28;
        let by = py + M_ROW3;
        cv.rrect(bx as f32, by as f32, 24.0, 20.0, 6.0, (255, 255, 255, 14));
        cv.rrect_stroke(bx as f32, by as f32, 24.0, 20.0, 6.0, 1.0, (255, 255, 255, 26));
        let (gw, gh) = te.measure(glyph, f_sub() + 1, 600);
        te.draw(cv, glyph, bx + (24 - gw) / 2, by + (20 - gh) / 2, f_sub() + 1, 600, INK2);
        hits.push(Kind::Static, *id, bx, by, 24, 20);
    }
    cv.rrect(cx as f32, nt_y as f32, cwid as f32, note_h as f32, 8.0, (255, 255, 255, 12));
    cv.rrect_stroke(
        cx as f32,
        nt_y as f32,
        cwid as f32,
        note_h as f32,
        8.0,
        1.0,
        if m.field == 1 { (255, 194, 71, 170) } else { (255, 255, 255, 24) },
    );
    note_cells_clear();
    if m.note.is_empty() {
        if m.field == 1 {
            // 聚焦但还没内容：只画光标
            cv.rect(cx + 10, nt_y + 9, 2, 15, NT_BAR);
        } else {
            te.draw(cv, "备注（可选，支持 B 加粗 / H 标题 / • 列表 / 1. 编号）…", cx + 10, nt_y + 9, 13, 400, NT_PH);
        }
    } else {
        let lines: Vec<&str> = m.note.split('\n').collect();
        let start = lines.len().saturating_sub(3);
        let mut lstart = 0usize;
        for part in lines.iter().take(start) {
            lstart += part.len() + 1;
        }
        let (sel_a, sel_b) = match m.sel {
            Some((a, b)) => (a.min(b), a.max(b)),
            None => (usize::MAX, 0),
        };
        let mut ly = nt_y + 9;
        for (i, line) in lines.iter().enumerate().skip(start) {
            let style = m.style_at(i);
            let has = |f: &str| style.split(',').any(|x| x == f);
            let bold = has("b");
            let heading = if has("h2") { 2 } else if has("h1") { 1 } else { 0 };
            let li = has("li");
            let oln = style
                .split(',')
                .find_map(|f| f.strip_prefix("ol:"))
                .unwrap_or("");
            let mut tx = cx + 10;
            if li {
                cv.circle((cx + 12) as f32, (ly + 8) as f32, 2.0, INK3);
                tx = cx + 22;
            } else if !oln.is_empty() {
                te.draw(cv, &format!("{}.", oln), cx + 10, ly, 13, 400, INK3);
                tx = cx + 22;
            }
            let (px, weight) = if heading == 1 {
                (15, 600)
            } else if heading == 2 {
                (14, 600)
            } else {
                (13, if bold { 600 } else { 400 })
            };
            // 逐字：先量宽排格子（超宽自动换行），再逐字绘制（绘制位置与格子同源，光标才不会飘进字里）
            let wrap_r = cx + cwid - 10;
            let cont_x = if li || !oln.is_empty() { cx + 22 } else { cx + 10 };
            let mut glyphs: Vec<(usize, usize, i32, i32, i32)> = Vec::new();
            let mut x = tx;
            let mut vy = ly;
            for (bi, ch) in line.char_indices() {
                let w = te.advance(ch, px, weight).max(1);
                if ch != ' ' && x + w > wrap_r && x > cont_x {
                    x = cont_x;
                    vy += 19;
                }
                let (ba, bb) = (lstart + bi, lstart + bi + ch.len_utf8());
                glyphs.push((ba, bb, x, vy, w));
                x += w;
            }
            let clip_t = nt_y - 2;
            let clip_b = nt_y + note_h - 15;
            for (ba, bb, bx0, gy, w) in glyphs.iter() {
                if *gy < clip_t || *gy > clip_b { continue; }
                note_cell_push(*ba, *bb, *bx0, *gy, *w);
                if m.field == 1 && *ba < sel_b && *bb > sel_a {
                    cv.rect(*bx0, *gy, (*w).max(2), 15, (255, 194, 71, 80));
                }
            }
            note_cell_push(lstart + line.len(), lstart + line.len(), x, vy, 2);
            let fake_bold = bold || heading > 0;
            let tcol = if px >= 14 || fake_bold { INK } else { INK2 };
            for (ba, _bb, bx0, gy, _w) in glyphs.iter() {
                if *gy < clip_t || *gy > clip_b { continue; }
                let cs: String = line[*ba - lstart..].chars().take(1).collect();
                te.draw(cv, &cs, *bx0, *gy, px, weight, tcol);
                if fake_bold {
                    te.draw(cv, &cs, *bx0 + 1, *gy, px, weight, tcol);
                }
            }
            if m.field == 1 && m.caret >= lstart && m.caret <= lstart + line.len() {
                let (cxx, cyy) = note_boundary(m.caret).unwrap_or((tx, vy));
                note_caret_set(cxx, cyy);
            }
            ly = vy + 19;
            lstart += line.len() + 1;
        }
        if m.field == 1 {
            let (cxp, cyp) = note_caret_pos(0).unwrap_or((cx + 10, nt_y + 9));
            if CARET_ON.load(std::sync::atomic::Ordering::Relaxed) {
                cv.rect(cxp - 1, cyp, 2, 15, NT_BAR);
            }
        }
    }
    hits.push(Kind::Static, "modal:field:1", cx, nt_y, cwid, note_h);

    // 图片：选择按钮 + 已选文件名（只存路径，不画缩略图）
    let (img_w, img_h) = (108, 24);
    modal_label(cv, te, "附件", x0, py + M_ROW4, img_h);
    cv.rrect(cx as f32, (py + M_ROW4) as f32, img_w as f32, img_h as f32, 7.0, (255, 255, 255, 16));
    cv.rrect_stroke(
        cx as f32,
        (py + M_ROW4) as f32,
        img_w as f32,
        img_h as f32,
        7.0,
        1.0,
        (255, 255, 255, 40),
    );
    let (lw, lh) = te.measure("选择附件…", f_sub() + 1, 500);
    te.draw(
        cv,
        "选择附件…",
        cx + (img_w - lw) / 2,
        py + M_ROW4 + (img_h - lh) / 2,
        f_sub() + 1,
        500,
        INK,
    );
    hits.push(Kind::Static, "modal:img", cx, py + M_ROW4, img_w, img_h);

    let name_x = cx + img_w + 12;
    let mut name_w = x0 + cw - name_x;
    let prev_w = 52;
    if !m.img.is_empty() {
        let pvx = x0 + cw - prev_w;
        cv.rrect(pvx as f32, (py + M_ROW4) as f32, prev_w as f32, img_h as f32, 7.0, (255, 255, 255, 16));
        cv.rrect_stroke(
            pvx as f32,
            (py + M_ROW4) as f32,
            prev_w as f32,
            img_h as f32,
            7.0,
            1.0,
            (255, 255, 255, 40),
        );
        let (plw, plh) = te.measure("预览", f_sub() + 1, 500);
        te.draw(
            cv,
            "预览",
            pvx + (prev_w - plw) / 2,
            py + M_ROW4 + (img_h - plh) / 2,
            f_sub() + 1,
            500,
            INK,
        );
        hits.push(Kind::Static, "modal:preview", pvx, py + M_ROW4, prev_w, img_h);
        // 删除附件：预览左侧的小 ×
        let clx = pvx - 26;
        cv.rrect(clx as f32, (py + M_ROW4) as f32, 20.0, img_h as f32, 6.0, (255, 255, 255, 12));
        let (xw, xh) = te.measure("×", f_sub() + 2, 500);
        te.draw(cv, "×", clx + (20 - xw) / 2, py + M_ROW4 + (img_h - xh) / 2, f_sub() + 2, 500, (255, 107, 97, 220));
        hits.push(Kind::Static, "modal:img.clear", clx, py + M_ROW4, 20, img_h);
        name_w -= prev_w + 34;
    }
    if m.img.is_empty() {
        te.draw(cv, "未选择", name_x, py + M_ROW4 + 4, f_sub() + 1, 400, INK3);
    } else {
        let file = m.img.rsplit(['\\', '/']).next().unwrap_or(&m.img);
        let mut shown = String::from("已选：");
        shown.extend(file.chars().take(28));
        let shown = te.fit(&shown, name_w, f_sub() + 1, 400);
        te.draw(cv, &shown, name_x, py + M_ROW4 + 4, f_sub() + 1, 400, INK2);
    }

    // 到期：单个胶囊 + 下拉菜单（今天/明天/3 天/7 天/14 天/自定义）
    let (due_w, due_h) = (140, 28);
    modal_label(cv, te, "到期", x0, py + M_ROW5 + 7, due_h);
    let chip = m.due_label();
    let chip_on = m.due_sel >= 0;
    modal_pill(cv, cx, py + M_ROW5, due_w, due_h, chip_on, ACC);
    let (cw2, ch2) = te.measure(&chip, f_sub() + 1, 500);
    te.draw(
        cv,
        &chip,
        cx + 12,
        py + M_ROW5 + (due_h - ch2) / 2,
        f_sub() + 1,
        500,
        if chip_on { (20, 24, 30, 255) } else { INK2 },
    );
    let _ = cw2;
    // 右侧小折角（展开指示）
    let chx = cx + due_w - 16;
    let chy = py + M_ROW5 + due_h / 2 - 1;
    cv.hline(chx, chy, 5, if chip_on { ACC } else { INK3 });
    cv.hline(chx + 1, chy + 3, 3, if chip_on { ACC } else { INK3 });
    hits.push(Kind::Static, "modal:due", cx, py + M_ROW5, due_w, due_h);

    // 下拉菜单：宽 132，覆盖在表单之上
    if m.due_menu {
        let mw = 132;
        let item_h: i32 = 24;
        let n = crate::view::ModalForm::DUE_LABELS.len();
        let mh = n as i32 * item_h + 12;
        let mx = cx;
        let mut my = py + M_ROW5 + due_h + 6;
        // 菜单下方放不下就向上翻，避免最后一项被弹窗下沿裁掉
        if my + mh > py + ph - 8 {
            my = (py + M_ROW5 - mh - 6).max(py + 8);
        }
        cv.rrect(mx as f32, my as f32, mw as f32, mh as f32, 10.0, (24, 27, 35, 252));
        cv.rrect_stroke(mx as f32, my as f32, mw as f32, mh as f32, 10.0, 1.0, (255, 255, 255, 26));
        for i0 in 0..n {
            let i = i0 as i32;
            let iy = my + 6 + i * item_h;
            let sel = m.due_sel == i;
            if sel {
                cv.rrect((mx + 6) as f32, iy as f32, (mw - 12) as f32, item_h as f32, 6.0, (255, 255, 255, 18));
                cv.rect(mx + 6, iy + 5, 2, item_h - 10, ACC);
            }
            let lab = crate::view::ModalForm::DUE_LABELS[i as usize];
            te.draw(cv, lab, mx + 16, iy + 7, f_sub() + 1, 500, if sel { INK } else { INK2 });
            if sel {
                te.draw(cv, "✓", mx + mw - 22, iy + 5, 12, 600, ACC);
            }
            hits.push(Kind::Static, format!("modal:due:{}", i), mx + 6, iy, mw - 12, item_h);
        }
    }

    // 底部按钮：右对齐，取消（次色）+ 保存（主色实底）
    let (bw, bh) = (72, 28);
    let by = py + M_BTN_Y;
    let sx = px + pw - 20 - bw;
    let cxl = sx - 8 - bw;
    cv.rrect(cxl as f32, by as f32, bw as f32, bh as f32, 8.0, (255, 255, 255, 16));
    cv.rrect_stroke(
        cxl as f32,
        by as f32,
        bw as f32,
        bh as f32,
        8.0,
        1.0,
        (255, 255, 255, 46),
    );
    let (lw, lh) = te.measure("取消", f_base() + 1, 500);
    te.draw(cv, "取消", cxl + (bw - lw) / 2, by + (bh - lh) / 2, f_base() + 1, 500, INK2);
    hits.push(Kind::Static, "modal:cancel", cxl, by, bw, bh);

    cv.rrect(sx as f32, by as f32, bw as f32, bh as f32, 8.0, ACC);
    let (lw, lh) = te.measure("保存", f_base(), 400);
    te.draw(cv, "保存", sx + (bw - lw) / 2, by + (bh - lh) / 2, f_base(), 400, (20, 24, 30, 255));
    hits.push(Kind::Static, "modal:save", sx, by, bw, bh);

    // 自定义到期：日历选择器（覆盖表单主体，最后绘制，命中区在最上层）
    if m.cal_open {
        let ox = px + 12;
        let oy = py + 54;
        let ow = pw - 24;
        let oh = ph - 66;
        cv.rrect(ox as f32, oy as f32, ow as f32, oh as f32, 12.0, (16, 18, 24, 255));
        cv.rrect_stroke(ox as f32, oy as f32, ow as f32, oh as f32, 12.0, 1.0, (255, 255, 255, 26));
        let mtxt = format!("{} 年 {} 月", m.cal_y, m.cal_m);
        let (mw, _mh) = te.measure(&mtxt, f_base(), 500);
        let _ = mw;
        te.draw(cv, &mtxt, ox + 18, oy + 14, f_base() + 1, 600, INK);
        let nav_y = oy + 14;
        let prev_x = ox + ow - 92;
        let next_x = ox + ow - 40;
        cv.rrect(prev_x as f32, nav_y as f32, 26.0, 24.0, 7.0, (255, 255, 255, 16));
        cv.rrect(next_x as f32, nav_y as f32, 26.0, 24.0, 7.0, (255, 255, 255, 16));
        cv.triangle((prev_x + 13) as f32, (nav_y + 12) as f32, 4.0, 1, INK2);
        cv.triangle((next_x + 13) as f32, (nav_y + 12) as f32, 4.0, -1, INK2);
        hits.push(Kind::Static, "modal:cal:prev", prev_x, nav_y, 26, 24);
        hits.push(Kind::Static, "modal:cal:next", next_x, nav_y, 26, 24);
        let wd = ["一", "二", "三", "四", "五", "六", "日"];
        let cell_w = 46i32;
        let gx = ox + (ow - cell_w * 7) / 2;
        let hy = oy + 52;
        for (i, w) in wd.iter().enumerate() {
            let (ww, _) = te.ink(w, f_tag(), 500);
            te.draw(cv, w, gx + i as i32 * cell_w + (cell_w - ww) / 2, hy, f_tag(), 500, INK3);
        }
        let (lead, dim) = m.cal_grid();
        let today = v.d.today.as_str();
        let (sel_y, sel_m, sel_d) = match crate::view::parse_ymd(&m.due) {
            Some((y, mo, d)) => (y, mo, d as u32),
            None => (0, 0, 0),
        };
        let gy = hy + 22;
        let ch = 28i32;
        for d in 1..=dim {
            let idx = lead + d - 1;
            let cxx = gx + (idx % 7) as i32 * cell_w;
            let cyy = gy + (idx / 7) as i32 * ch;
            let is_sel = sel_y == m.cal_y && sel_m == m.cal_m && sel_d == d;
            let td = format!("{:04}-{:02}-{:02}", m.cal_y, m.cal_m, d);
            if is_sel {
                cv.circle((cxx + cell_w / 2) as f32, (cyy + ch / 2) as f32, 11.0, ACC);
            } else if td == today {
                cv.circle_stroke((cxx + cell_w / 2) as f32, (cyy + ch / 2) as f32, 11.0, 1.0, NT_BAR);
            }
            let ds = d.to_string();
            // 真·同心：用墨迹包围盒（left/top/w/h），而不是 advance 或 extent 宽度
            let (bl, bt, bw2, bh2) = te.ink_box(&ds, f_base(), 500);
            let dx0 = cxx + cell_w / 2 - (bl + bw2 / 2);
            let dy0 = cyy + ch / 2 - (bt + bh2 / 2);
            te.draw(cv, &ds, dx0, dy0, f_base(), 500,
                if is_sel { (20, 24, 30, 255) } else { INK2 });
            hits.push(Kind::Static, format!("modal:cal:day:{}", d), cxx, cyy, cell_w, ch);
        }
        // 左下角：直接在日历里清空到期（等价于菜单里的「不设到期」）
        let none_x = ox + 12;
        let none_y = oy + oh - 40;
        cv.rrect(none_x as f32, none_y as f32, 76.0, 26.0, 8.0, (255, 255, 255, 14));
        let (nw2, _) = te.measure(&"不设到期".to_string(), f_sub(), 500);
        te.draw(cv, &"不设到期".to_string(), none_x + (76 - nw2) / 2, none_y + 5, f_sub(), 500, INK2);
        hits.push(Kind::Static, "modal:cal:none", none_x, none_y, 76, 26);
        let back_x = ox + ow - 84;
        let back_y = oy + oh - 40;
        cv.rrect(back_x as f32, back_y as f32, 66.0, 26.0, 8.0, (255, 255, 255, 14));
        let (bw2, _) = te.measure("返回", f_sub(), 500);
        te.draw(cv, "返回", back_x + (66 - bw2) / 2, back_y + 5, f_sub(), 500, INK2);
        hits.push(Kind::Static, "modal:cal:back", back_x, back_y, 66, 26);
    }
}

// ───────────────────────── 托盘右键菜单（独立 layered 小窗） ─────────────────────────

/// 自绘托盘菜单：232 宽，5 行（第 5 行前一条分隔线），深色圆角。
/// 菜单底图（不含悬停高亮）——悬停时直接复用，避免每次重排文字
/// 菜单悬停高亮：只补一层胶囊 + 左侧竖条，不重画文字
fn draw_tabs(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits) {
    cv.rrect(TAB_X as f32, TAB_Y as f32, TAB_W as f32, TAB_H as f32, 13.0, (255, 255, 255, 10));
    cv.rrect_stroke(
        TAB_X as f32,
        TAB_Y as f32,
        TAB_W as f32,
        TAB_H as f32,
        13.0,
        1.0,
        (255, 255, 255, 16),
    );
    let segs: [(Tab, &str, &str, i32); 3] = [
        (Tab::Proj, "项目进度", "tab.proj", TAB_X + 2),
        (Tab::Dept, "部门进度", "tab.dept", TAB_X + 2 + TAB_SEG_W),
        (Tab::Meet, "会议安排", "tab.meet", TAB_X + 2 + TAB_SEG_W * 2),
    ];
    for (t, label, id, x) in segs {
        let sel = v.tab == t;
        if sel {
            cv.rrect(
                (x + 18) as f32,
                (TAB_Y + TAB_H - 4) as f32,
                (TAB_SEG_W - 36) as f32,
                2.4,
                1.2,
                (255, 159, 67, 225),
            );
        }
        band_center(
            cv,
            te,
            label,
            x + TAB_SEG_W / 2,
            TAB_Y + 1,
            TAB_H - 3,
            12,
            500,
            INK2,
        );
        hits.push(Kind::Chip, id, x, TAB_Y, TAB_SEG_W, TAB_H);
    }
}

// —— 标签二：我的待办（部门导入 + 自建，同列表；部门项左侧 3px 灰蓝竖条）——
/// 拖拽排序时的行偏移：按「统一列表序号」（部门项在前、自建项在后）计算。
/// 返回 (y 偏移, 是否是被拖起的那一行)
fn drag_off(drag: Option<(usize, i32, usize)>, ui: usize) -> (i32, bool) {
    if let Some((from, _, to)) = drag {
        if ui == from {
            return (0, true);
        }
        if from < to && ui > from && ui <= to {
            return (-L_ROW_H, false);
        }
        if to < from && ui >= to && ui < from {
            return (L_ROW_H, false);
        }
    }
    (0, false)
}

fn left_card_dept(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits) {
    if v.arc_mode {
        return left_card_archive(cv, te, v, hits);
    }
    // 点日历某天：左卡临时切换成「当天节点」列表，右上角「‹ 返回」
    if !v.day_list.is_empty() || !v.day_label.is_empty() {
        let head = if v.day_label.is_empty() { "当天节点".to_string() } else { v.day_label.clone() };
        let tw = band_text(cv, te, &head, L_X1, L_TITLE_Y, L_TITLE_H, f_title(), 600, INK);
        hits.push(Kind::Text, "title:day", L_X1, L_TITLE_Y, tw, L_TITLE_H);
        let (bw, _) = te.measure("‹ 返回", f_sub(), 400);
        let bx = L_X1 + l_w() - bw;
        te.draw(cv, "‹ 返回", bx, L_TITLE_Y + (L_TITLE_H - 14) / 2, f_sub(), 400, (167, 174, 189, 235));
        hits.push(Kind::Button, "dayback", bx - 6, L_TITLE_Y, bw + 12, L_TITLE_H);
        cv.hline(L_X1, L_DIV_Y, l_w(), (255, 255, 255, 14));
        for (i, r) in v.day_list.iter().take(l_rows()).enumerate() {
            let y = L_LIST_TOP + i as i32 * L_ROW_H;
            let cy = y + L_ROW_H / 2;
            let (pc, pl) = match r.prio.as_str() {
                "P0" => ((255u8, 107u8, 97u8, 255u8), "P0"),
                "P1" => ((255, 194, 71, 255), "P1"),
                "P2" => ((90, 169, 255, 255), "P2"),
                _ => ((95, 102, 116, 160), ""),
            };
            let mut nx = L_X1;
            cv.rrect(nx as f32, (cy - 9) as f32, 3.0, 18.0, 1.5, pc);
            nx += 8;
            if !pl.is_empty() {
                let (pw, ph) = te.measure(pl, f_tag(), 700);
                cv.rrect(nx as f32, (cy - 8) as f32, (pw + 10) as f32, 16.0, 8.0, (pc.0, pc.1, pc.2, 40));
                te.draw(cv, pl, nx + 5, cy - ph / 2, f_tag(), 700, pc);
                nx += pw + 16;
            }
            let body = te.fit(&r.text, (L_X1 + l_w() - nx).max(40), f_base(), 400);
            let (_, bh) = te.measure(&body, f_base(), 400);
            te.draw(cv, &body, nx, cy - bh / 2, f_base(), 400, INK);
        }
        if v.day_list.is_empty() {
            te.draw(cv, "这一天没有节点", L_X1, L_LIST_TOP + 6, f_sub(), 400, (111, 118, 134, 200));
        }
        return;
    }

    let tw = band_text(cv, te, "我的待办", L_X1, L_TITLE_Y, L_TITLE_H, f_title(), 600, INK);
    hits.push(Kind::Text, "title:mine", L_X1, L_TITLE_Y, tw, L_TITLE_H);
    let n = v.dept_rows();
    let cnt = format!("{}", n);
    let cx = L_X1 + tw + 8;
    let cw2 = band_text(cv, te, &cnt, cx, L_TITLE_Y, L_TITLE_H, f_sub(), 500, (111, 118, 134, 200));
    hits.push(Kind::Text, "title:mine:count", cx, L_TITLE_Y, cw2, L_TITLE_H);

    // 「＋」新增（标题行右端；标签二才有）
    let plus_cx = L_X1 + l_w() - 11;
    let plus_cy = L_TITLE_Y + L_TITLE_H / 2;
    cv.circle(plus_cx as f32, plus_cy as f32, 9.0, (255, 255, 255, 14));
    cv.rrect(
        plus_cx as f32 - 4.0,
        plus_cy as f32 - 0.7,
        8.0,
        1.4,
        0.7,
        (255, 255, 255, 95),
    );
    cv.rrect(
        plus_cx as f32 - 0.7,
        plus_cy as f32 - 4.0,
        1.4,
        8.0,
        0.7,
        (255, 255, 255, 95),
    );
    hits.push(Kind::Button, "mine:add", plus_cx - 12, plus_cy - 12, 24, 24);

    // 标题行内、＋ 左侧：归档入口（N=0 灰掉、无箭头，仍可点；点进同区域的归档列表）
    let n_arc = v.archive.len();
    let arc_label = if n_arc == 0 { "已归档 0".to_string() } else { format!("已归档 {} \u{203a}", n_arc) };
    let (aw, ah) = te.measure(&arc_label, f_sub(), 400);
    let ax = plus_cx - 16 - aw;
    te.draw(
        cv,
        &arc_label,
        ax,
        L_TITLE_Y + (L_TITLE_H - ah) / 2,
        f_sub(),
        400,
        if n_arc == 0 { (111, 118, 134, 150) } else { (167, 174, 189, 235) },
    );
    hits.push(Kind::Button, "arc:open", ax - 4, L_TITLE_Y, aw + 8, L_TITLE_H);

    cv.hline(L_X1, L_DIV_Y, l_w(), (255, 255, 255, 14));

    // 新建待办的临时输入行：插在列表顶部（不是弹窗），占 1 行，可见条目相应减 1
    let mut top = L_LIST_TOP;
    let rows = l_rows();
    let max_rows = if v.newtodo.active { rows.saturating_sub(1) } else { rows };
    if v.newtodo.active {
        row_new(cv, te, v, hits, L_X1, top);
        top += L_ROW_H;
    }

    // 空态：不叫「示例」
    if n == 0 {
        band_text(
            cv, te, "暂无部门待办 · 点＋添加", L_X1, L_LIST_TOP, L_ROW_H, f_base(), 400, (139, 147, 164, 235),
        );
    }

    // 单列全宽，固定 9 行（部门导入在前、自建在后；行高与命中矩形不变）
    let mut used = 0usize;
    let drag = v.drag;
    let mut ui = 0usize;
    for (i, r) in v.dept.iter().enumerate().skip(v.scroll) {
        if used >= max_rows {
            break;
        }
        let (off, lifted) = drag_off(drag, ui);
        if !lifted {
            let y = top + used as i32 * L_ROW_H + off;
            row_dept(cv, te, v, hits, L_X1, y, "dept", i, r);
        }
        used += 1;
        ui += 1;
    }
    for (i, r) in v.custom.iter().enumerate().skip(v.scroll.saturating_sub(v.dept.len())) {
        if used >= max_rows {
            break;
        }
        let (off, lifted) = drag_off(drag, ui);
        if !lifted {
            let y = top + used as i32 * L_ROW_H + off;
            row_dept(cv, te, v, hits, L_X1, y, "mine", i, r);
        }
        used += 1;
        ui += 1;
    }

    // 拖起的那一行：原位留空，跟着光标画在最上层（抬起底 + 主色描边），
    // 目标位置画插入线 —— 拖动过程有明确反馈，其他行同步让位。
    if let Some((u, dy, t)) = drag {
        let total = v.dept.len() + v.custom.len();
        let band_lo = 0i32;
        let band_hi = (total as i32 - 1).max(0) * L_ROW_H;
        let dy_c = dy.clamp(band_lo - (u as i32) * L_ROW_H, band_hi - (u as i32) * L_ROW_H);
        let y = top + (u as i32) * L_ROW_H + dy_c;
        if u < v.dept.len() {
            if let Some(r) = v.dept.get(u) {
                cv.rrect((L_X1 - 6) as f32, (y - 2) as f32, (l_w() + 12) as f32, (L_ROW_H + 4) as f32, 8.0, (255, 255, 255, 30));
                cv.rrect_stroke((L_X1 - 6) as f32, (y - 2) as f32, (l_w() + 12) as f32, (L_ROW_H + 4) as f32, 8.0, 1.0, (255, 159, 67, 160));
                let mut scratch = Hits { v: Vec::new() };
                row_dept(cv, te, v, &mut scratch, L_X1, y, "dept", u, r);
            }
        } else if let Some(r) = v.custom.get(u - v.dept.len()) {
            cv.rrect((L_X1 - 6) as f32, (y - 2) as f32, (l_w() + 12) as f32, (L_ROW_H + 4) as f32, 8.0, (255, 255, 255, 30));
            cv.rrect_stroke((L_X1 - 6) as f32, (y - 2) as f32, (l_w() + 12) as f32, (L_ROW_H + 4) as f32, 8.0, 1.0, (255, 159, 67, 160));
            let mut scratch = Hits { v: Vec::new() };
            row_dept(cv, te, v, &mut scratch, L_X1, y, "mine", u - v.dept.len(), r);
        }
        let line_y = top + ((t as i32) * L_ROW_H) + if t > u { L_ROW_H } else { 0 };
        cv.rrect((L_X1 - 4) as f32, (line_y - 1) as f32, (l_w() + 8) as f32, 2.0, 1.0, (255, 159, 67, 235));
        cv.circle((L_X1 - 4) as f32, line_y as f32, 2.4, (255, 159, 67, 255));
    }

    if n > l_rows() {
        band_text(
            cv,
            te,
            &format!("另有 {} 条 · 滚轮查看", n - l_rows()),
            L_X1,
            L_LIST_TOP + l_rows() as i32 * L_ROW_H,
            20,
            f_tag(),
            400,
            (111, 118, 134, 150),
        );
    }

    // 日期面板最后画：右对齐到胶囊右沿，压在行上面，避免溢出左卡
    if v.newtodo.active && v.newtodo.menu_open {
        let chip_x = L_X1 + l_w() - 16 - 8 - NT_CHIP_W;
        let chip_y = L_LIST_TOP + L_ROW_H / 2 - NT_CHIP_H / 2;
        date_menu(cv, te, v, hits, chip_x, chip_y);
    }



    // 未读消息（与项目进度同源）
    unread_block(cv, te, v, hits);

    footer_stamp(cv, te, v, hits);
}

/// 标签二内的归档视图（不加第三个标签）：标题「归档 · N 条」+ 右上「\u{2039} 返回」。
fn left_card_archive(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits) {

    let title = format!("归档 · {} 条", v.archive.len());
    let tw = band_text(cv, te, &title, L_X1, L_TITLE_Y, L_TITLE_H, f_title(), 600, INK);
    hits.push(Kind::Text, "title:archive", L_X1, L_TITLE_Y, tw, L_TITLE_H);

    // 右上「‹ 返回」
    let back = "\u{2039} 返回";
    let (bw, bh) = te.measure(back, f_sub(), 500);
    let bx = L_X1 + l_w() - bw;
    te.draw(cv, back, bx, L_TITLE_Y + (L_TITLE_H - bh) / 2, f_sub(), 500, ACC);
    hits.push(Kind::Button, "arc:back", bx - 6, L_TITLE_Y - 2, bw + 12, L_TITLE_H + 4);

    cv.hline(L_X1, L_DIV_Y, l_w(), (255, 255, 255, 14));

    if v.archive.is_empty() {
        band_text(
            cv, te, "归档是空的", L_X1, L_LIST_TOP, L_ROW_H, f_base(), 400, (139, 147, 164, 235),
        );
    }

    let total = v.archive.len();
    let start = v.arc_scroll.min(total);
    let shown: Vec<(usize, &crate::view::ArcRow)> =
        v.archive.iter().enumerate().skip(start).take(L_ROWS_ARC).collect();

    for (slot, (i, r)) in shown.iter().enumerate() {
        let y = L_LIST_TOP + slot as i32 * L_ROW_H;
        let cy = y + L_ROW_H / 2;
        let id = format!("arc:{}", i);
        hits.push(Kind::Row, id.clone(), L_X1 - 4, y, l_w() + 8, L_ROW_H);

        // 彻底删除 / 二次确认
        let purge_id = format!("arc:{}:purge", i);
        let armed = v.arc_pending.as_deref() == Some(purge_id.as_str());
        if armed {
            let pw = 40;
            let px = L_X1 + l_w() - pw;
            cv.rrect(px as f32, (cy - 9) as f32, pw as f32, 18.0, 9.0, (255, 95, 86, 34));
            let (mw, mh) = te.measure("确定?", f_tag(), 600);
            te.draw(cv, "确定?", px + (pw - mw) / 2, cy - mh / 2, f_tag(), 600, P0C);
            hits.push(Kind::Button, purge_id, px, cy - 10, pw, 20);
        } else {
            let px = L_X1 + l_w() - 16;
            let (xw, xh) = te.measure("×", f_sub(), 400);
            te.draw(cv, "×", px + (16 - xw) / 2, cy - xh / 2, f_sub(), 400, (255, 95, 86, 240));
            hits.push(Kind::Button, purge_id, px, cy - 10, 16, 20);
        }

        // 「恢复」文字按钮（强调色）
        let (rw, rh) = te.measure("恢复", f_sub(), 500);
        let rx = L_X1 + l_w() - 16 - 12 - rw;
        te.draw(cv, "恢复", rx, cy - rh / 2, f_sub(), 500, ACC);
        hits.push(Kind::Button, format!("arc:{}:restore", i), rx - 4, cy - 10, rw + 8, 20);

        // 归档时间（M-D）
        let (aw, ah) = te.measure(&r.at, f_tag(), 400);
        let ax = rx - 12 - aw;
        te.draw(cv, &r.at, ax, cy - ah / 2, f_tag(), 400, (111, 118, 134, 190));

        // 截止日（月-日）
        let due_short = if r.due.len() >= 10 { &r.due[5..] } else { r.due.as_str() };
        let due_r = ax - 10;
        let dw = if due_short.is_empty() {
            0
        } else {
            let (dw, dh) = te.measure(due_short, f_tag(), 400);
            te.draw(cv, due_short, due_r - dw, cy - dh / 2, f_tag(), 400, (111, 118, 134, 190));
            dw
        };

        // 正文（次级色，已完成加删除线）
        let name_r = due_r - if dw > 0 { dw + 10 } else { 0 };
        let body = te.fit(&r.text, (name_r - L_X1).max(24), f_base(), 400);
        let (bw, bh) = te.measure(&body, f_base(), 400);
        te.draw(cv, &body, L_X1, cy - bh / 2, f_base(), 400, (167, 174, 189, 255));
        if r.done {
            cv.hline(L_X1, cy, bw, (111, 118, 134, 170));
        }
        hits.push(Kind::Text, id, L_X1, cy - 9, bw, 18);
    }

    if total > L_ROWS_ARC {
        band_text(
            cv,
            te,
            &format!("{}-{} / {}（滚轮翻动）", start + 1, (start + L_ROWS_ARC).min(total), total),
            L_X1,
            l_stamp_y(),
            22,
            f_tag(),
            400,
            (111, 118, 134, 180),
        );
    }
}

fn attach_glyph(path: &str) -> &'static str {
    let ext = path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp" | "ico" => "图",
        "xls" | "xlsx" | "csv" | "et" => "表",
        "doc" | "docx" | "pdf" | "txt" | "md" | "ppt" | "pptx" => "文",
        _ => "附",
    }
}

/// 按可用宽度截断文本（超出加省略号），避免待办正文与倒计时在宽窗里被拉成一条长线。
#[allow(clippy::too_many_arguments)]
fn row_dept(
    cv: &mut Canvas,
    te: &mut TextEngine,
    v: &View,
    hits: &mut Hits,
    x: i32,
    y: i32,
    prefix: &str,
    idx: usize,
    r: &DeptRow,
) {
    let cy = y + L_ROW_H / 2;
    // 宽窗下不要把正文和倒计时拉到窗口两端：列表内容最大 760px
    let lw_eff = l_w().min(760);
    let key = if prefix == "dept" {
        v.key_of_dept(idx)
    } else {
        v.key_of_custom(idx)
    };
    let done = r.done || v.is_done(&key);
    let id = format!("{}:{}", prefix, idx);
    hits.push(Kind::Row, id.clone(), x - 4, y, lw_eff + 8, L_ROW_H);

    let ccx = x + G_CIRCLE / 2;
    circle_mark(cv, ccx, cy, done);
    hits.push(Kind::Circle, id.clone(), ccx - 10, cy - 10, 20, 20);

    let mut nx = x + G_CIRCLE + G_GAP;
    // 优先级必须一眼可辨：左侧竖条取优先级色，再补一枚 P0/P1/P2 胶囊
    let (ptxt, pcol) = match r.prio.as_str() {
        "P0" => ("P0", (255, 107, 97, 255)),
        "P1" => ("P1", (255, 194, 71, 255)),
        "P2" => ("P2", (90, 169, 255, 255)),
        _ => ("", (0, 0, 0, 0)),
    };
    if pcol.3 > 0 {
        cv.rrect(nx as f32, (cy - 9) as f32, 3.0, 18.0, 1.5, pcol);
        nx += 6;
    }
    if !ptxt.is_empty() {
        let (ptw, pth) = te.measure(ptxt, f_tag(), 700);
        let pcw = ptw + 10;
        cv.rrect(
            nx as f32,
            (cy - 8) as f32,
            pcw as f32,
            16.0,
            8.0,
            (pcol.0, pcol.1, pcol.2, 40),
        );
        te.draw(cv, ptxt, nx + 5, cy - pth / 2, f_tag(), 700, pcol);
        nx += pcw + 6;
    }

    let del_id = format!("{}:{}:del", prefix, idx);
    let pending = v.pending_del.as_deref() == Some(del_id.as_str());
    if pending {
        let bw = 40;
        let bx = x + lw_eff - bw;
        cv.rrect(bx as f32, (cy - 9) as f32, bw as f32, 18.0, 9.0, (255, 95, 86, 34));
        let (tw2, th2) = te.measure("确定?", f_tag(), 600);
        te.draw(cv, "确定?", bx + (bw - tw2) / 2, cy - th2 / 2, f_tag(), 600, P0C);
        hits.push(Kind::Button, del_id, bx, cy - 10, bw, 20);
    } else {
        let del_x = x + lw_eff - 16;
        let (xw, xh) = te.measure("×", f_sub(), 400);
        te.draw(cv, "×", del_x + (16 - xw) / 2, cy - xh / 2, f_sub(), 400, (255, 255, 255, 70));
        hits.push(Kind::Button, del_id, del_x, cy - 10, 16, 20);
    }

    let cd = v.countdown(&r.due);
    let due_right = x + lw_eff - 24;
    let (cdw, cdh) = if cd.is_empty() {
        (0, 0)
    } else {
        te.measure(&cd, f_sub(), 600)
    };
    let mut right_limit = if cd.is_empty() { due_right } else { due_right - cdw - 12 };

    if !r.img.is_empty() {
        let bx = right_limit - 18;
        if bx > nx + 40 {
            cv.rrect(bx as f32, (cy - 9) as f32, 18.0, 18.0, 5.0, (255, 255, 255, 16));
            let g = attach_glyph(&r.img);
            let (gw, gh) = te.measure(g, 9, 600);
            te.draw(cv, g, bx + (18 - gw) / 2, cy - gh / 2, 9, 600, (255, 194, 71, 230));
            hits.push(
                Kind::Static,
                format!("{}:{}:att", prefix, idx),
                bx,
                cy - 9,
                18,
                18,
            );
            right_limit = bx - 8;
        }
    }

    if !cd.is_empty() && !pending {
        let days = v.days_left(&r.due).unwrap_or(99);
        let col = if done {
            (95, 102, 116, 200)
        } else if days < 0 {
            P0C
        } else if days == 0 {
            ACC
        } else if days == 1 {
            NT_BAR
        } else {
            INK3
        };
        te.draw(cv, &cd, due_right - cdw, cy - cdh / 2, f_sub(), 600, col);
        hits.push(
            Kind::Text,
            format!("{}:{}:due", prefix, idx),
            due_right - cdw - 2,
            cy - 9,
            cdw + 4,
            18,
        );
    }

    let name_r = right_limit;
    let body = te.fit(&r.text, (name_r - nx).max(24), f_base(), 400);
    let (bw, bh) = te.measure(&body, f_base(), 400);
    let nc = if done { INK3 } else { INK };
    te.draw(cv, &body, nx, cy - bh / 2, f_base(), 400, nc);
    if done {
        cv.hline(nx, cy, bw, (111, 118, 134, 150));
    }
    hits.push(Kind::Text, id, nx, cy - 9, bw, 18);
}

// —— 新建待办：行内输入行（不是弹窗）+ 日期胶囊 + 胶囊下方日期菜单 ——
const NT_BAR: Rgba = (255, 194, 71, 255); // #FFC247 主色竖条 / 光标
const NT_INK: Rgba = (232, 234, 240, 255); // #E8EAF0 输入后的正文色
const NT_PH: Rgba = (139, 147, 164, 235); // 占位灰
const NT_MENU_BG: Rgba = (27, 31, 40, 246); // #1B1F28
const NT_MENU_BD: Rgba = (42, 48, 64, 255); // #2A3040
const NT_CHIP_W: i32 = 44;
const NT_CHIP_H: i32 = 20;
const NT_MENU_W: i32 = 168;
const NT_MENU_ROW: i32 = 26;
const NT_MENU_PAD: i32 = 8;

/// 12 段点状空心圆：表示「还没建成正式待办」
fn dashed_circle(cv: &mut Canvas, cx: i32, cy: i32, r: f32, c: Rgba) {
    for i in 0..12 {
        let a = i as f32 * std::f32::consts::TAU / 12.0;
        cv.circle(cx as f32 + r * a.cos(), cy as f32 + r * a.sin(), 1.15, c);
    }
}

fn row_new(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits, x: i32, y: i32) {
    let nt = &v.newtodo;
    let cy = y + L_ROW_H / 2;
    // 整行底：聚焦提亮一档（0.06 → 0.10），不聚焦时是普通临时行
    let a: u8 = if nt.focus { 26 } else { 15 };
    cv.rrect((x - 4) as f32, y as f32, (l_w() + 8) as f32, L_ROW_H as f32, 8.0, (255, 255, 255, a));
    if nt.focus {
        cv.rrect((x - 7) as f32, (y + 4) as f32, 2.0, (L_ROW_H - 8) as f32, 1.0, NT_BAR);
    }
    hits.push(Kind::Text, "newtodo.input", x - 4, y, l_w() + 8, L_ROW_H);

    // 左侧虚线圆（未创建）
    dashed_circle(cv, x + G_CIRCLE / 2, cy, 8.0, (255, 255, 255, 110));

    // 日期胶囊（右端，避让删除列）
    let chip_x = x + l_w() - 16 - 8 - NT_CHIP_W;
    let chip_y = cy - NT_CHIP_H / 2;
    cv.rrect(chip_x as f32, chip_y as f32, NT_CHIP_W as f32, NT_CHIP_H as f32, 10.0, (255, 255, 255, 14));
    cv.rrect_stroke(
        chip_x as f32,
        chip_y as f32,
        NT_CHIP_W as f32,
        NT_CHIP_H as f32,
        10.0,
        1.0,
        (255, 255, 255, 22),
    );
    let (lw, lh) = te.measure(&nt.date_label, 12, 400);
    te.draw(
        cv,
        &nt.date_label,
        chip_x + (NT_CHIP_W - lw) / 2,
        cy - lh / 2,
        12,
        400,
        if nt.focus { NT_INK } else { INK2 },
    );
    hits.push(Kind::Button, "newtodo.datechip", chip_x, chip_y, NT_CHIP_W, NT_CHIP_H);

    // 正文 / 占位 + 竖线光标（闪烁帧不画）
    let nx = x + G_CIRCLE + G_GAP;
    let right = chip_x - 10;
    if nt.text.is_empty() {
        let ph = te.fit("新增待办，回车确认", right - nx - 6, f_base(), 400);
        let (pw, phh) = te.measure(&ph, f_base(), 400);
        // 光标在文字左侧（相当于输入位置 0），文字右移 6px 避免被光标压住
        if nt.focus {
            cv.vline(nx, cy - 8, 15, NT_BAR);
        }
        te.draw(cv, &ph, nx + 6, cy - phh / 2, f_base(), 400, NT_PH);
        let _ = pw;
    } else {
        let body = te.fit(&nt.text, right - nx - 2, f_base(), 400);
        let (bw, bh) = te.measure(&body, f_base(), 400);
        te.draw(cv, &body, nx, cy - bh / 2, f_base(), 400, NT_INK);
        if nt.focus {
            cv.vline(nx + bw + 1, cy - 8, 15, NT_BAR);
        }
    }

    // 胶囊下方的日期面板由 left_card_dept 在所有行画完之后再画（保证压在行上面）
}

fn date_menu(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits, chip_x: i32, chip_y: i32) {
    let nt = &v.newtodo;
    let mx = chip_x + NT_CHIP_W - NT_MENU_W;
    let my = chip_y + NT_CHIP_H + 4;
    let h = NT_MENU_PAD * 2 + NT_MENU_ROW * NEWDATE_OPTIONS.len() as i32;
    cv.rrect(mx as f32, my as f32, NT_MENU_W as f32, h as f32, 8.0, NT_MENU_BG);
    cv.rrect_stroke(mx as f32, my as f32, NT_MENU_W as f32, h as f32, 8.0, 1.0, NT_MENU_BD);
    for (i, opt) in NEWDATE_OPTIONS.iter().enumerate() {
        let ry = my + NT_MENU_PAD + i as i32 * NT_MENU_ROW;
        let cy = ry + NT_MENU_ROW / 2;
        if i == nt.menu_sel {
            cv.rrect(
                (mx + 4) as f32,
                ry as f32,
                (NT_MENU_W - 8) as f32,
                NT_MENU_ROW as f32,
                5.0,
                (255, 255, 255, 26),
            );
            cv.rrect((mx + 4) as f32, (ry + 5) as f32, 2.0, (NT_MENU_ROW - 10) as f32, 1.0, NT_BAR);
        }
        if i == nt.date_sel {
            let (_, ch) = te.measure("✓", 12, 400);
            te.draw(cv, "✓", mx + 14, cy - ch / 2, 12, 400, NT_BAR);
        }
        let (_, th) = te.measure(opt, 12, 400);
        let col = if i == nt.menu_sel { NT_INK } else { INK2 };
        te.draw(cv, opt, mx + 34, cy - th / 2, 12, 400, col);
        hits.push(
            Kind::Button,
            format!("newtodo.datemenu.{}", i),
            mx + 4,
            ry,
            NT_MENU_W - 8,
            NT_MENU_ROW,
        );
    }
}

// ───────────────── 备注框：逐字命中 / 光标定位 / 闪烁 ─────────────────
pub static CARET_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

thread_local! {
    static ITEM_CELLS: std::cell::RefCell<Vec<(usize, usize, i32, i32, i32)>> = std::cell::RefCell::new(Vec::new());
}
fn item_cells_clear() {
    ITEM_CELLS.with(|c| c.borrow_mut().clear());
}
fn item_cell_push(a: usize, b: usize, x: i32, y: i32, w: i32) {
    ITEM_CELLS.with(|c| c.borrow_mut().push((a, b, x, y, w)));
}
pub fn item_caret_pos(idx: usize) -> Option<(i32, i32)> {
    ITEM_CELLS.with(|c| {
        let v = c.borrow();
        let mut best: Option<(i32, i32)> = None;
        for (a, b, x, y, w) in v.iter() {
            // idx 正好落在这一格的末尾（idx == b）时必须返回“这一格右边”，
            // 否则光标会画到该字左边，看起来整整早一格。
            if *a == idx {
                return Some((*x, *y));
            }
            if *a < idx && idx <= *b {
                return Some((*x + *w, *y));
            }
            if *b <= idx {
                best = Some((*x + *w, *y));
            }
        }
        best
    })
}
/// 调试：把当前事项框的命中格摘要成一行（用于校准点击→光标映射）
pub fn item_dbg() -> String {
    ITEM_CELLS.with(|c| {
        let v = c.borrow();
        let mut s = format!("cells={}", v.len());
        for (_i, (a, b, x, _y, w)) in v.iter().enumerate() {
            s.push_str(&format!(" [{a}..{b} x={x} w={w}]"));
        }
        s
    })
}
pub fn item_hit(x: i32, _y: i32) -> Option<usize> {
    // 点击超出最近字格中心 ITEM_HIT_MAX_DIST px 就不再强制落光标：远离输入框的空白点击
    // 不该把编辑光标硬拽到最近一格，否则会让用户以为点中了输入框。
    const ITEM_HIT_MAX_DIST: i32 = 100;
    ITEM_CELLS.with(|c| {
        let v = c.borrow();
        let mut best: Option<(i32, usize)> = None;
        for (a, b, cx, _cy, w) in v.iter() {
            let mid = cx + w / 2;
            let d = (x - mid).abs();
            // 右偏规则：点进字内（超过左边 1/5 字宽）就算插到该字之后，避免光标往回跳一格。
            let cand = if x > cx + w / 5 { *b } else { *a };
            if best.map(|(bd, _)| d < bd).unwrap_or(true) {
                best = Some((d, cand));
            }
        }
        best.filter(|(d, _)| *d <= ITEM_HIT_MAX_DIST).map(|(_, i)| i)
    })
}
thread_local! {
    static NOTE_CELLS: std::cell::RefCell<Vec<(usize, usize, i32, i32, i32)>> =
        std::cell::RefCell::new(Vec::new());
    static NOTE_CARET: std::cell::Cell<(i32, i32)> = const { std::cell::Cell::new((0, 0)) };
}

fn note_cells_clear() {
    NOTE_CELLS.with(|c| c.borrow_mut().clear());
    NOTE_CARET.with(|c| c.set((0, 0)));
}

fn note_cell_push(a: usize, b: usize, x: i32, y: i32, w: i32) {
    NOTE_CELLS.with(|c| c.borrow_mut().push((a, b, x, y, w)));
}

fn note_caret_set(x: i32, y: i32) {
    NOTE_CARET.with(|c| c.set((x, y)));
}

/// 光标像素位置（本帧实测出来的；没有则 None）
pub fn note_caret_pos(_idx: usize) -> Option<(i32, i32)> {
    let p = NOTE_CARET.with(|c| c.get());
    if p == (0, 0) {
        None
    } else {
        Some(p)
    }
}


/// 某个字节下标处的光标像素位置（以逐字实测的格子为准）
pub fn note_boundary(idx: usize) -> Option<(i32, i32)> {
    NOTE_CELLS.with(|c| {
        let cells = c.borrow();
        let mut best: Option<(i32, i32)> = None;
        for (a, b, x0, y0, w) in cells.iter() {
            if *a == idx {
                return Some((*x0, *y0));
            }
            if *b == idx {
                best = Some((*x0 + *w, *y0));
            }
        }
        best
    })
}

/// 鼠标位置 → 最近的字节下标（点哪插哪）
pub fn note_hit(x: i32, y: i32) -> Option<usize> {
    NOTE_CELLS.with(|c| {
        let cells = c.borrow();
        let mut best: Option<(i32, usize)> = None;
        for (a, b, x0, y0, w) in cells.iter() {
            if y < *y0 - 2 || y > *y0 + 17 {
                continue;
            }
            let d = if x < *x0 {
                *x0 - x
            } else if x > *x0 + *w {
                x - (*x0 + *w)
            } else {
                0
            };
            // 点在哪一半：左半 → 插到该字前面，右半 → 插到该字后面（符合直觉）
            let mid = *x0 + (*w / 2);
            let idx = if x > mid { *b } else { *a };
            if best.map(|(bd, _)| d < bd).unwrap_or(true) {
                best = Some((d, idx));
            }
        }
        best.filter(|(d, _)| *d <= 10).map(|(_, a)| a)
    })
}

// ───────────────────────── 左卡：会议安排（当日 / 当周 / 当月） ─────────────────────────

/// 距会议开始还有多少分钟（负值=已开始）
fn mins_left(date: &str, start: &str) -> Option<i64> {
    if date.len() < 10 || start.len() < 5 {
        return None;
    }
    let (y, mo, d) = (date[0..4].parse::<i64>().ok()?, date[5..7].parse::<i64>().ok()?, date[8..10].parse::<i64>().ok()?);
    let (sh, smi) = (start[0..2].parse::<i64>().ok()?, start[3..5].parse::<i64>().ok()?);
    let ns = crate::log::now_str();
    let (ny, nmo, nd) = (ns[0..4].parse::<i64>().ok()?, ns[5..7].parse::<i64>().ok()?, ns[8..10].parse::<i64>().ok()?);
    let (nh, nmi) = (ns[11..13].parse::<i64>().ok()?, ns[14..16].parse::<i64>().ok()?);
    let days = |y: i64, m: i64, d: i64| -> i64 {
        let y = if m <= 2 { y - 1 } else { y };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let mp = (m + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146097 + doe - 719468
    };
    Some((days(y, mo, d) - days(ny, nmo, nd)) * 1440 + (sh * 60 + smi) - (nh * 60 + nmi))
}

/// 倒计时文案：X天X小时 / X小时X分 / X分
fn left_text(m: i64) -> String {
    if m <= 0 {
        return String::new();
    }
    if m >= 1440 {
        format!("{}天{}小时", m / 1440, (m % 1440) / 60)
    } else if m >= 60 {
        format!("{}小时{}分", m / 60, m % 60)
    } else {
        format!("{}分", m)
    }
}

fn left_card_meet(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits) {
    let tw = band_text(cv, te, "会议安排", L_X1, L_TITLE_Y, L_TITLE_H, f_title(), 600, INK);
    hits.push(Kind::Text, "title:meet", L_X1, L_TITLE_Y, tw, L_TITLE_H);
    // 右上角三档：当日 / 当周 / 当月
    let spans = [(0u8, "当日"), (1u8, "当周"), (2u8, "当月")];
    let seg = 44;
    let left = L_X1 + l_w() - seg * 3;
    for (i, (n, label)) in spans.iter().enumerate() {
        let sel = v.meet_span == *n;
        let bx = left + (i as i32) * seg;
        if sel {
            cv.rrect(bx as f32, (L_TITLE_Y + 3) as f32, 40.0, 20.0, 10.0, (255, 159, 67, 40));
        }
        let (w2, h2) = te.measure(label, f_tag(), 600);
        te.draw(
            cv,
            label,
            bx + (40 - w2) / 2,
            L_TITLE_Y + 3 + (20 - h2) / 2,
            f_tag(),
            600,
            if sel { (255, 194, 71, 255) } else { (139, 147, 164, 235) },
        );
        hits.push(Kind::Chip, &format!("meet.span.{}", n), bx, L_TITLE_Y + 3, 40, 20);
    }
    cv.hline(L_X1, L_DIV_Y, l_w(), (255, 255, 255, 14));
    let rows = v.meet_rows();
    if rows.is_empty() {
        te.draw(cv, "这一段没有会议", L_X1, L_LIST_TOP + 6, f_sub(), 400, (111, 118, 134, 200));
        unread_block(cv, te, v, hits);
        footer_stamp(cv, te, v, hits);
        return;
    }
    let n = l_rows().min(rows.len());
    for i in 0..n {
        let r = &rows[i];
        let y = L_LIST_TOP + i as i32 * L_ROW_H;
        let cy = y + L_ROW_H / 2;
        // 时间点（左固定列）
        let t = if r.start.is_empty() { "--:--".to_string() } else { r.start.clone() };
        te.draw(cv, &t, L_X1, cy - 7, f_tag(), 600, (255, 194, 71, 235));
        let mut nx = L_X1 + 48;
        // 周/月档位补日期（MM-DD）
        if v.meet_span != 0 {
            let d = if r.date.len() >= 10 { &r.date[5..10] } else { &r.date };
            te.draw(cv, d, nx, cy - 7, f_tag(), 400, (139, 147, 164, 220));
            nx += 44;
        }
        let cd = mins_left(&r.date, &r.start).map(left_text).unwrap_or_default();
        let cdw = if cd.is_empty() { 0 } else { te.measure(&cd, f_tag(), 400).0 + 12 };
        if !cd.is_empty() {
            te.draw(cv, &cd, L_X1 + l_w() - cdw + 6, cy - 7, f_tag(), 400, (255, 194, 71, 210));
        }
        let body = te.fit(&r.name, (L_X1 + l_w() - nx - cdw - 6).max(40), f_base(), 400);
        let (_, bh) = te.measure(&body, f_base(), 400);
        te.draw(cv, &body, nx, cy - bh / 2, f_base(), 400, INK);
        hits.push(Kind::Static, &format!("meet:{}", i), L_X1, y, l_w(), L_ROW_H);
    }
    let bottom = L_LIST_TOP + n as i32 * L_ROW_H + 6;
    if rows.len() > n {
        te.draw(
            cv,
            &format!("另有 {} 条 · 滚轮查看", rows.len() - n),
            L_X1,
            bottom,
            f_sub(),
            400,
            (111, 118, 134, 200),
        );
    }
    footer_stamp(cv, te, v, hits);
    // 未读消息（与项目进度同源）
    unread_block(cv, te, v, hits);
}

// ───────────────────────── 开会前 5 分钟：窗口顶部红色提醒条 ─────────────────────────

fn draw_remind(cv: &mut Canvas, te: &mut TextEngine, v: &View, hits: &mut Hits) {
    let r = match v.remind.as_ref() {
        Some(x) => x,
        None => return,
    };
    let w = 520;
    let h = 62;
    let x = (l_w() - w) / 2 + LEFT_X;
    let y = CARD_Y + 6;
    // 阴影 + 底
    cv.rrect((x + 2) as f32, (y + 4) as f32, w as f32, h as f32, 14.0, (0, 0, 0, 90));
    cv.rrect(x as f32, y as f32, w as f32, h as f32, 14.0, (58, 20, 22, 248));
    cv.rrect_stroke(x as f32, y as f32, w as f32, h as f32, 14.0, 1.2, (255, 99, 90, 190));
    // 左侧红条
    cv.rrect((x + 12) as f32, (y + 12) as f32, 4.0, (h - 24) as f32, 2.0, (255, 107, 97, 255));
    let t1 = format!("{} 分钟后 · {} 开始", r.left.max(0), r.time);
    te.draw(cv, &t1, x + 26, y + 12, f_sub(), 600, (255, 138, 128, 255));
    let name = te.fit(&r.name, w - 150, f_title(), 600);
    te.draw(cv, &name, x + 26, y + 32, f_title(), 600, (255, 236, 233, 255));
    let (cw, _) = te.measure("关闭", f_tag(), 400);
    te.draw(cv, "关闭", x + w - 18 - cw, y + 12, f_tag(), 400, (214, 178, 176, 235));
    hits.push(Kind::Chip, "remind.close", x, y, w, h);
}
