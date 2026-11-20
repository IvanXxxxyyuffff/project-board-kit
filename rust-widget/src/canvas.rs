//! 预乘 ARGB8888 画布 + CPU 软光栅原语（无 GPU、无 GDI 绘图、无模糊）。
//! 所有坐标是 980×566 逻辑像素；抗锯齿靠 SDF 覆盖率，1px 边缘不发虚。

#[derive(Clone)]
pub struct Canvas {
    pub w: i32,
    pub h: i32,
    /// 预乘 ARGB，内存序 0xAARRGGBB
    pub px: Vec<u32>,
    /// 脏区裁剪（x,y,w,h）：翻月只重画右卡时开，其他时候 None
    pub clip: Option<(i32, i32, i32, i32)>,
    /// 设计尺寸 → 像素尺寸的缩放比（布局一律按 980×566 逻辑坐标书写，这里统一放大）
    pub sx: f32,
    pub sy: f32,
    /// 等比缩放后在窗口里居中的像素偏移
    pub ox: f32,
    pub oy: f32,
}

pub type Rgba = (u8, u8, u8, u8);

impl Canvas {
    #[inline]
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgba) {
        let (x, y, w, h) = (self.sip(x), self.sipy(y), self.sip(w), (h as f32 * self.sy).round() as i32);
        self.rect_raw(x, y, w, h, c)
    }
    #[inline]
    pub fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, rad: f32, c: Rgba) {
        let (x, y, w, h, rad) = (self.sfx(x), self.sfy(y), self.sfx(w), self.sfy(h), self.sfx(rad));
        self.rrect_raw(x, y, w, h, rad, c)
    }
    #[inline]
#[allow(clippy::too_many_arguments)]
    pub fn rrect_stroke(&mut self, x: f32, y: f32, w: f32, h: f32, rad: f32, th: f32, c: Rgba) {
        let (x, y, w, h, rad, th) = (self.sfx(x), self.sfy(y), self.sfx(w), self.sfy(h), self.sfx(rad), self.sfx(th));
        self.rrect_stroke_raw(x, y, w, h, rad, th, c)
    }
    #[inline]
    pub fn circle(&mut self, cx: f32, cy: f32, rad: f32, c: Rgba) {
        let (cx, cy, rad) = (self.sfx(cx), self.sfy(cy), self.sfx(rad));
        self.circle_raw(cx, cy, rad, c)
    }
    #[inline]
    pub fn circle_stroke(&mut self, cx: f32, cy: f32, rad: f32, th: f32, c: Rgba) {
        let (cx, cy, rad, th) = (self.sfx(cx), self.sfy(cy), self.sfx(rad), self.sfx(th));
        self.circle_stroke_raw(cx, cy, rad, th, c)
    }
    #[inline]
    pub fn triangle(&mut self, cx: f32, cy: f32, size: f32, dir: i32, c: Rgba) {
        let (cx, cy, size) = (self.sfx(cx), self.sfy(cy), self.sfx(size));
        self.triangle_raw(cx, cy, size, dir, c)
    }
    #[inline]
    pub fn hline(&mut self, x: i32, y: i32, w: i32, c: Rgba) {
        let (x, y, w) = (self.sip(x), self.sip(y), self.sip(w));
        self.hline_raw(x, y, w, c)
    }
    #[inline]
    pub fn vline(&mut self, x: i32, y: i32, h: i32, c: Rgba) {
        let (x, y, h) = (self.sip(x), self.sipy(y), (h as f32 * self.sy).round() as i32);
        self.vline_raw(x, y, h, c)
    }
    #[inline]
    fn sip(&self, v: i32) -> i32 { (v as f32 * self.sx + self.ox).round() as i32 }
    #[inline]
    fn sipy(&self, v: i32) -> i32 { (v as f32 * self.sy + self.oy).round() as i32 }
    #[inline]
    fn sfx(&self, v: f32) -> f32 { v * self.sx + self.ox }
    #[inline]
    fn sfy(&self, v: f32) -> f32 { v * self.sy + self.oy }
    #[inline]
    pub fn new(w: i32, h: i32) -> Self {
        Self { w, h, px: vec![0; (w * h) as usize], clip: None, sx: 1.0, sy: 1.0, ox: 0.0, oy: 0.0 }
    }

    #[allow(dead_code)]
    pub fn set_clip(&mut self, r: Option<(i32, i32, i32, i32)>) {
        self.clip = r;
    }

    /// 脏区清零（透明），供翻月/切标签的局部重绘使用。
    #[allow(dead_code)]
    pub fn clear_rect_raw(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.w);
        let y1 = (y + h).min(self.h);
        for yy in y0..y1 {
            let row = (yy * self.w) as usize;
            for xx in x0..x1 {
                self.px[row + xx as usize] = 0;
            }
        }
    }


    /// 直接覆盖为不透明/半透明的预乘色（不参与混合）：用于局部脏区回填面板底色。
    #[allow(dead_code)]
    pub fn fill_rect_raw(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgba) {
        let (r, g, b, a) = c;
        let pa = a as u32;
        let px = (pa << 24) | ((r as u32 * pa / 255) << 16) | ((g as u32 * pa / 255) << 8) | (b as u32 * pa / 255);
        self.fill_rect_px(x, y, w, h, px);
    }

    /// 最近邻缩放 blit 一张不透明 BGRA/ARGB32 位图（0xAARRGGBB），带裁剪。
#[allow(clippy::too_many_arguments)]
    pub fn blit(&mut self, dx: i32, dy: i32, dw: i32, dh: i32, src: &[u32], sw: i32, sh: i32) {
        if sw <= 0 || sh <= 0 || dw <= 0 || dh <= 0 || src.len() < (sw * sh) as usize {
            return;
        }
        let x0 = dx.max(0);
        let y0 = dy.max(0);
        let x1 = (dx + dw).min(self.w);
        let y1 = (dy + dh).min(self.h);
        for yy in y0..y1 {
            let sy = ((yy - dy) as i64 * sh as i64 / dh as i64) as i32;
            let srow = (sy * sw) as usize;
            let drow = (yy * self.w) as usize;
            for xx in x0..x1 {
                let sx = ((xx - dx) as i64 * sw as i64 / dw as i64) as i32;
                let s = src[srow + sx as usize];
                let a = (s >> 24) & 0xFF;
                if a == 0 {
                    continue;
                }
                let d = self.px[drow + xx as usize];
                if a == 255 {
                    self.px[drow + xx as usize] = s;
                    continue;
                }
                let blend = |sp: u32, dp: u32| -> u32 {
                    (sp * a + dp * (255 - a)) / 255
                };
                let r = blend((s >> 16) & 0xFF, (d >> 16) & 0xFF);
                let g = blend((s >> 8) & 0xFF, (d >> 8) & 0xFF);
                let b = blend(s & 0xFF, d & 0xFF);
                let out_a = a + (((d >> 24) & 0xFF) * (255 - a)) / 255;
                self.px[drow + xx as usize] = (out_a << 24) | (r << 16) | (g << 8) | b;
            }
        }
    }

    fn fill_rect_px(&mut self, x: i32, y: i32, w: i32, h: i32, px: u32) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.w);
        let y1 = (y + h).min(self.h);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for yy in y0..y1 {
            let row = (yy * self.w) as usize;
            for xx in x0..x1 {
                self.px[row + xx as usize] = px;
            }
        }
    }

    /// 从另一块同尺寸画布拷回一个矩形（脏区回填，走 memcpy 级别的行拷贝）。
    #[allow(dead_code)]
    pub fn copy_rect_from(&mut self, src: &Canvas, x: i32, y: i32, w: i32, h: i32) {
        if src.w != self.w || src.h != self.h {
            return;
        }
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.w);
        let y1 = (y + h).min(self.h);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for yy in y0..y1 {
            let a = (yy * self.w) as usize;
            let b = (yy * src.w) as usize;
            self.px[a + x0 as usize..a + x1 as usize]
                .copy_from_slice(&src.px[b + x0 as usize..b + x1 as usize]);
        }
    }

    pub fn clear(&mut self) {
        for p in self.px.iter_mut() {
            *p = 0;
        }
    }

    #[inline]
    pub(crate) fn blend_at(&mut self, x: i32, y: i32, a: u8, r: u8, g: u8, b: u8) {
        if a == 0 || x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        if let Some((cx, cy, cw, ch)) = self.clip {
            if x < cx || y < cy || x >= cx + cw || y >= cy + ch {
                return;
            }
        }
        let i = (y * self.w + x) as usize;
        let sa = a as u32;
        if sa == 255 {
            self.px[i] = 0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
            return;
        }
        let d = self.px[i];
        let inv = 255 - sa;
        let da = (d >> 24) & 0xFF;
        // 预乘 alpha 的 over：out = src*sa + dst*(1-sa)
        let dr = (((d >> 16) & 0xFF) * inv) / 255 + (r as u32 * sa) / 255;
        let dg = (((d >> 8) & 0xFF) * inv) / 255 + (g as u32 * sa) / 255;
        let db = ((d & 0xFF) * inv) / 255 + (b as u32 * sa) / 255;
        let oa = (da * inv) / 255 + sa;
        self.px[i] = (oa.min(255) << 24) | (dr.min(255) << 16) | (dg.min(255) << 8) | db.min(255);
    }

    /// 轴对齐实心矩形（整数边界，无 AA）。
    pub fn rect_raw(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgba) {
        let (r, g, b, a) = c;
        if a == 0 || w <= 0 || h <= 0 {
            return;
        }
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.w);
        let y1 = (y + h).min(self.h);
        for yy in y0..y1 {
            for xx in x0..x1 {
                self.blend_at(xx, yy, a, r, g, b);
            }
        }
    }

    /// 圆角矩形（SDF + 1px 覆盖率）。
    pub fn rrect_raw(&mut self, x: f32, y: f32, w: f32, h: f32, rad: f32, c: Rgba) {
        let (r, g, b, a) = c;
        if a == 0 || w <= 0.0 || h <= 0.0 {
            return;
        }
        let rad = rad.min(w * 0.5).min(h * 0.5).max(0.0);
        let cx = x + w * 0.5;
        let cy = y + h * 0.5;
        let hx = w * 0.5 - rad;
        let hy = h * 0.5 - rad;
        let x0 = x.floor() as i32;
        let y0 = y.floor() as i32;
        let x1 = (x + w).ceil() as i32;
        let y1 = (y + h).ceil() as i32;
        for yy in y0..y1 {
            for xx in x0..x1 {
                let px = xx as f32 + 0.5 - cx;
                let py = yy as f32 + 0.5 - cy;
                let qx = px.abs() - hx;
                let qy = py.abs() - hy;
                let outside = (qx.max(0.0) * qx.max(0.0) + qy.max(0.0) * qy.max(0.0)).sqrt();
                let inside = qx.max(qy).min(0.0);
                let sd = outside + inside - rad;
                let cov = (0.5 - sd).clamp(0.0, 1.0);
                if cov > 0.0 {
                    let aa = (a as f32 * cov).round() as u8;
                    self.blend_at(xx, yy, aa, r, g, b);
                }
            }
        }
    }

    /// 圆角矩形描边（环带 SDF）。
#[allow(clippy::too_many_arguments)]
    pub fn rrect_stroke_raw(&mut self, x: f32, y: f32, w: f32, h: f32, rad: f32, th: f32, c: Rgba) {
        let (r, g, b, a) = c;
        if a == 0 || w <= 0.0 || h <= 0.0 {
            return;
        }
        let rad = rad.min(w * 0.5).min(h * 0.5).max(0.0);
        let cx = x + w * 0.5;
        let cy = y + h * 0.5;
        let hx = w * 0.5 - rad;
        let hy = h * 0.5 - rad;
        let half = th * 0.5;
        for yy in (y - th).floor() as i32..(y + h + th).ceil() as i32 {
            for xx in (x - th).floor() as i32..(x + w + th).ceil() as i32 {
                let px = xx as f32 + 0.5 - cx;
                let py = yy as f32 + 0.5 - cy;
                let qx = px.abs() - hx;
                let qy = py.abs() - hy;
                let outside = (qx.max(0.0) * qx.max(0.0) + qy.max(0.0) * qy.max(0.0)).sqrt();
                let inside = qx.max(qy).min(0.0);
                let sd = (outside + inside - rad).abs() - half;
                let cov = (0.5 - sd).clamp(0.0, 1.0);
                if cov > 0.0 {
                    let aa = (a as f32 * cov).round() as u8;
                    self.blend_at(xx, yy, aa, r, g, b);
                }
            }
        }
    }

    pub fn circle_raw(&mut self, cx: f32, cy: f32, rad: f32, c: Rgba) {
        let (r, g, b, a) = c;
        if a == 0 || rad <= 0.0 {
            return;
        }
        for yy in (cy - rad - 1.0) as i32..(cy + rad + 1.0) as i32 {
            for xx in (cx - rad - 1.0) as i32..(cx + rad + 1.0) as i32 {
                let dx = xx as f32 + 0.5 - cx;
                let dy = yy as f32 + 0.5 - cy;
                let sd = (dx * dx + dy * dy).sqrt() - rad;
                let cov = (0.5 - sd).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend_at(xx, yy, (a as f32 * cov).round() as u8, r, g, b);
                }
            }
        }
    }

    pub fn circle_stroke_raw(&mut self, cx: f32, cy: f32, rad: f32, th: f32, c: Rgba) {
        let (r, g, b, a) = c;
        if a == 0 || rad <= 0.0 {
            return;
        }
        let half = th * 0.5;
        for yy in (cy - rad - th) as i32..(cy + rad + th) as i32 {
            for xx in (cx - rad - th) as i32..(cx + rad + th) as i32 {
                let dx = xx as f32 + 0.5 - cx;
                let dy = yy as f32 + 0.5 - cy;
                let sd = ((dx * dx + dy * dy).sqrt() - rad).abs() - half;
                let cov = (0.5 - sd).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend_at(xx, yy, (a as f32 * cov).round() as u8, r, g, b);
                }
            }
        }
    }

    /// 水平/垂直线（0.5px 对齐，避免发灰）。
    pub fn hline_raw(&mut self, x: i32, y: i32, w: i32, c: Rgba) {
        self.rect(x, y, w, 1, c);
    }
    pub fn vline_raw(&mut self, x: i32, y: i32, h: i32, c: Rgba) {
        self.rect(x, y, 1, h, c);
    }

    /// 三角形（月历翻页箭头）。dir: -1 左, 1 右。
    pub fn triangle_raw(&mut self, cx: f32, cy: f32, size: f32, dir: i32, c: Rgba) {
        let (r, g, b, a) = c;
        if a == 0 {
            return;
        }
        let h = size * 0.5;
        for yy in (cy - h).floor() as i32..(cy + h).ceil() as i32 {
            for xx in (cx - h).floor() as i32..(cx + h).ceil() as i32 {
                let px = xx as f32 + 0.5 - cx;
                let py = yy as f32 + 0.5 - cy;
                // 沿 dir 方向的半平面与斜边
                let t = if dir < 0 { px + h } else { h - px }; // 0..2h
                if t < 0.0 {
                    continue;
                }
                let half_w = h * (1.0 - t / (2.0 * h)) * 2.0; // 三角形底边在 t=0
                let inside = py.abs() <= half_w * 0.5;
                if inside {
                    self.blend_at(xx, yy, a, r, g, b);
                }
            }
        }
    }
}
