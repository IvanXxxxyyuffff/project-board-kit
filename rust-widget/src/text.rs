//! 文本栅格化：GDI 灰度抗锯齿（ANTIALIASED_QUALITY，无 ClearType 彩边）→ 亮度当 alpha → 预乘贴图。
//! 位图按 (文本, 字号, 粗细, 颜色, alpha) 做哈希缓存；清缓存只发生在数据/月份变化（bump()）。

use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::SIZE;
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, GetDC,
    GetCharWidth32W, GetGlyphIndicesW, GetTextExtentPoint32W, GetTextFaceW, ReleaseDC, SelectObject, SetBkMode, SetTextColor,
    TextOutW, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, GGI_MARK_NONEXISTING_GLYPHS, HDC, HFONT, TRANSPARENT,
};

use crate::canvas::{Canvas, Rgba};

const SCRATCH_W: i32 = 1100;
const SCRATCH_H: i32 = 80;

const CHARSET_DEFAULT: u32 = 1; // DEFAULT_CHARSET
const OUT_TT_PRECIS: u32 = 4;
const CLIP_DEFAULT_PRECIS: u32 = 0;
const ANTIALIASED_QUALITY: u32 = 4;
const DEFAULT_PITCH: u32 = 0;
const FF_DONTCARE: u32 = 0;

/// 依次尝试的字体。YaHei UI 是纯拉丁子集：GDI 会把中文链到 SimSun（中文变宋体），
/// 所以用「自带中文字形」这个能力判定来选，而不是比字面名（GDI 会把名字本地化成「微软雅黑」）。
const FACES: [&str; 3] = ["Microsoft YaHei", "Microsoft YaHei UI", "SimSun"];

#[derive(Hash, PartialEq, Eq, Clone)]
struct Key {
    text: String,
    px: i32,
    weight: i32,
    color: (u8, u8, u8),
    alpha: u8,
}

const CACHE_MAX: usize = 3000;

pub struct Run {
    pub w: i32,
    pub h: i32,
    pub mask: Vec<u8>,
}

pub struct TextEngine {
    screen: HDC,
    dc: HDC,
    bmp: isize,
    bits: *mut u32,
    fonts: HashMap<(i32, i32), isize>,
    cache: HashMap<Key, Run>,
    pub hits: u64,
    pub misses: u64,
}

unsafe impl Send for TextEngine {}

fn wide(s: &str) -> Vec<u16> {
    // 必须带 NUL：CreateFontW 要的是以 0 结尾的宽字符串，缺了会静默退到系统默认字体（仿宋/宋体）
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 该字体是否自带中文字形（不依赖 GDI 字体链接）。
fn has_cjk(dc: HDC, f: HFONT) -> bool {
    unsafe {
        let old = SelectObject(dc, f);
        let probe: Vec<u16> = "中".encode_utf16().collect();
        let mut idx = [0u16; 2];
        let r = GetGlyphIndicesW(dc, probe.as_ptr(), 1, idx.as_mut_ptr(), GGI_MARK_NONEXISTING_GLYPHS);
        SelectObject(dc, old);
        r != u32::MAX && r > 0 && idx[0] != 0xFFFF
    }
}

/// 取 DC 当前选中字体的实际字面名（用于校验 CreateFontW 是否真的命中了目标字体）。
fn face_of(dc: HDC, f: HFONT) -> String {
    unsafe {
        let old = SelectObject(dc, f);
        let mut buf = [0u16; 64];
        let n = GetTextFaceW(dc, 64, buf.as_mut_ptr());
        SelectObject(dc, old);
        if n <= 0 {
            return String::new();
        }
        let len = ((n as usize) - 1).min(63);
        String::from_utf16_lossy(&buf[..len])
    }
}

impl TextEngine {
    pub fn new() -> Self {
        unsafe {
            let screen = GetDC(null_mut());
            let dc = CreateCompatibleDC(screen);
            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: SCRATCH_W,
                biHeight: -SCRATCH_H,
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
            SetBkMode(dc, TRANSPARENT as i32);
            Self {
                screen,
                dc,
                bmp: bmp as isize,
                bits: bits as *mut u32,
                fonts: HashMap::new(),
                cache: HashMap::new(),
                hits: 0,
                misses: 0,
            }
        }
    }

    fn font(&mut self, px: i32, weight: i32) -> HFONT {
        // 小字号禁用 GDI 合成粗体：Regular 字体在 12-15px 按 600/700 请求时 GDI 用描边加重
        // 模拟粗体，灰度抗锯齿下就是「发糊、像重影」。统一封顶 500。
        let weight = if px <= 15 { weight.min(500) } else { weight };
        if let Some(h) = self.fonts.get(&(px, weight)) {
            return *h as HFONT;
        }
        let mut chosen: HFONT = std::ptr::null_mut();
        for name in FACES {
            let f = Self::make_font(px, weight, name);
            if f.is_null() {
                continue;
            }
            // CreateFontW 对不存在的字面名也返回非 NULL，只能靠字形能力判断
            if has_cjk(self.dc, f) {
                if std::env::var("DBG_FACE").is_ok() {
                    eprintln!("[font] px={} w={} 请求={:?} 实际={:?}", px, weight, name, face_of(self.dc, f));
                }
                chosen = f;
                break;
            }
            unsafe { DeleteObject(f) };
        }
        self.fonts.insert((px, weight), chosen as isize);
        chosen
    }

    fn make_font(px: i32, weight: i32, name: &str) -> HFONT {
        let face = wide(name);
        unsafe {
            CreateFontW(
                -px,
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                CHARSET_DEFAULT,
                OUT_TT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                ANTIALIASED_QUALITY,
                DEFAULT_PITCH | FF_DONTCARE,
                face.as_ptr(),
            )
        }
    }



    /// 单字推进宽度（A+B+C），与 GDI 排版一致；CJK 的 extent 只给墨迹宽，不能用。
    pub fn advance(&mut self, ch: char, px: i32, weight: i32) -> i32 {
        let f = self.font(px, weight);
        let cp = ch as u32;
        if cp > 0xFFFF {
            return self.measure(&ch.to_string(), px, weight).0.max(1);
        }
        unsafe {
            let old = SelectObject(self.dc, f);
            let mut w: i32 = 0;
            let ok = GetCharWidth32W(self.dc, cp, cp, &mut w);
            SelectObject(self.dc, old);
            if ok == 0 || w <= 0 {
                self.measure(&ch.to_string(), px, weight).0.max(1)
            } else {
                w
            }
        }
    }
    /// 墨迹包围盒 (left, top, w, h)：字形在 run 位图里的真实非透明范围。
    /// 用于把数字/图标摆到几何中心——按 advance 或 extent 居中都会偏。
    pub fn ink_box(&mut self, text: &str, px: i32, weight: i32) -> (i32, i32, i32, i32) {
        let run = self.run(text, px, weight, (255, 255, 255, 255));
        let (w, h) = (run.w, run.h);
        let (mut l, mut t, mut r, mut b) = (w, h, -1i32, -1i32);
        for y in 0..h {
            for x in 0..w {
                if run.mask[(y * w + x) as usize] > 8 {
                    if x < l { l = x; }
                    if x > r { r = x; }
                    if y < t { t = y; }
                    if y > b { b = y; }
                }
            }
        }
        if r < 0 { return (0, 0, w, h); }
        (l, t, r - l + 1, b - t + 1)
    }
    /// 墨迹尺寸（与 draw 同源：run() 的紧致位图宽高）。用于把字真正摆到格子中心。
    pub fn ink(&mut self, text: &str, px: i32, weight: i32) -> (i32, i32) {
        if text.is_empty() { return (0, 0); }
        let run = self.run(text, px, weight, (255, 255, 255, 255));
        (run.w, run.h)
    }
    /// 与 draw 完全同源的文本游标宽度（走 run 的栅格遮罩，保证与实际绘制一致）
    /// 文本像素宽高（与绘制同字体同字号）。
    pub fn measure(&mut self, text: &str, px: i32, weight: i32) -> (i32, i32) {
        if text.is_empty() {
            return (0, px);
        }
        let f = self.font(px, weight);
        unsafe {
            let old = SelectObject(self.dc, f);
            let w = wide(text);
            let mut sz: SIZE = std::mem::zeroed();
            let ok = GetTextExtentPoint32W(self.dc, w.as_ptr(), w.len() as i32, &mut sz);
            SelectObject(self.dc, old);
            if ok == 0 {
                (text.chars().count() as i32 * px, px)
            } else {
                (sz.cx, sz.cy)
            }
        }
    }

    /// 按最大宽度截断（尾部 …），返回可绘制的字符串。
    pub fn fit(&mut self, text: &str, max_w: i32, px: i32, weight: i32) -> String {
        if self.measure(text, px, weight).0 <= max_w {
            return text.to_string();
        }
        let chars: Vec<char> = text.chars().collect();
        let mut lo = 0usize;
        let mut hi = chars.len();
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let mut s: String = chars[..mid].iter().collect();
            s.push('…');
            if self.measure(&s, px, weight).0 <= max_w {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        let mut s: String = chars[..lo].iter().collect();
        if lo < chars.len() {
            s.push('…');
        }
        s
    }

    fn run(&mut self, text: &str, px: i32, weight: i32, color: Rgba) -> &Run {
        // 先取尺寸，避免借用冲突
        let key = Key {
            text: text.to_string(),
            px,
            weight,
            color: (color.0, color.1, color.2),
            alpha: color.3,
        };
        if self.cache.contains_key(&key) {
            self.hits += 1;
            return self.cache.get(&key).unwrap();
        }
        self.misses += 1;
        let (tw, th) = self.measure(text, px, weight);
        let w = tw.clamp(1, SCRATCH_W);
        let h = th.clamp(px, SCRATCH_H);
        let f = self.font(px, weight);
        unsafe {
            // 清空 scratch 区域（黑底）
            for y in 0..h {
                let row = self.bits.add((y * SCRATCH_W) as usize);
                for x in 0..(w + 2).min(SCRATCH_W) {
                    *row.add(x as usize) = 0;
                }
            }
            let old = SelectObject(self.dc, f);
            SetTextColor(self.dc, 0x00FF_FFFF);
            SetBkMode(self.dc, TRANSPARENT as i32);
            let wbuf = wide(text);
            TextOutW(self.dc, 0, 0, wbuf.as_ptr(), wbuf.len() as i32);
            SelectObject(self.dc, old);
        }
        let mut mask = vec![0u8; (w * h) as usize];
        unsafe {
            for y in 0..h {
                let row = self.bits.add((y * SCRATCH_W) as usize);
                for x in 0..w {
                    let v = *row.add(x as usize);
                    let r = (v >> 16) & 0xFF;
                    let g = (v >> 8) & 0xFF;
                    let b = v & 0xFF;
                    mask[(y * w + x) as usize] = r.max(g).max(b) as u8;
                }
            }
        }
        if self.cache.len() >= CACHE_MAX {
            self.cache.clear();
        }
        let lookup = key.clone();
        self.cache.insert(key, Run { w, h, mask });
        self.cache.get(&lookup).unwrap()
    }

    /// 画一行文字，(x,y) 是文本盒左上角。返回宽度。
#[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        cv: &mut Canvas,
        text: &str,
        x: i32,
        y: i32,
        px: i32,
        weight: i32,
        color: Rgba,
    ) -> i32 {
        if text.is_empty() {
            return 0;
        }
        // 画布可能被放大到窗口实际尺寸（sx/sy≠1）：逻辑坐标在这里统一换算成像素，
        // 这样布局代码完全不用动，字号也不会被位图拉伸糊掉。
        let (sx, sy) = (cv.sx.max(0.01), cv.sy.max(0.01));
        let px_s = ((px as f32 * sx).round() as i32).max(1);
        let (x, y) = (
            ((x as f32) * sx + cv.ox).round() as i32,
            ((y as f32) * sy + cv.oy).round() as i32,
        );
        let run_w;
        {
            let run = self.run(text, px_s, weight, color);
            run_w = ((run.w as f32) / sx).round() as i32;
            let (r, g, b, a) = color;
            for yy in 0..run.h {
                let cy = y + yy;
                if cy < 0 || cy >= cv.h {
                    continue;
                }
                for xx in 0..run.w {
                    let cx = x + xx;
                    if cx < 0 || cx >= cv.w {
                        continue;
                    }
                    let m = run.mask[(yy * run.w + xx) as usize];
                    if m == 0 {
                        continue;
                    }
                    let cov = (m as u32 * a as u32 / 255) as u8;
                    if cov == 0 {
                        continue;
                    }
                    cv.blend_at(cx, cy, cov, r, g, b);
                }
            }
        }
        run_w
    }

    /// 右对齐绘制：返回左边界 x。
    #[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
    pub fn draw_right(
        &mut self,
        cv: &mut Canvas,
        text: &str,
        right: i32,
        y: i32,
        px: i32,
        weight: i32,
        color: Rgba,
    ) -> i32 {
        let w = self.measure(text, px, weight).0;
        self.draw(cv, text, right - w, y, px, weight, color);
        right - w
    }

    /// 居中绘制（给定中心 x）。
    #[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
    pub fn draw_center(
        &mut self,
        cv: &mut Canvas,
        text: &str,
        cx: i32,
        y: i32,
        px: i32,
        weight: i32,
        color: Rgba,
    ) -> i32 {
        let w = self.ink(text, px, weight).0;
        self.draw(cv, text, cx - w / 2, y, px, weight, color);
        w
    }

    #[allow(dead_code)]
    pub fn bump(&mut self) {
        self.cache.clear();
    }

    /// 数据热重载/翻月后清空位图缓存（key 含文本内容，旧文本不会复现）。
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    #[allow(dead_code)]
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }
}

impl Drop for TextEngine {
    fn drop(&mut self) {
        unsafe {
            if self.bmp != 0 {
                DeleteObject(self.bmp as *mut c_void);
            }
            if !self.dc.is_null() {
                DeleteDC(self.dc);
            }
            if !self.screen.is_null() {
                ReleaseDC(null_mut(), self.screen);
            }
        }
    }
}
