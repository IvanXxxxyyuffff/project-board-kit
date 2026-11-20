//! 附件缩略图：GDI+ 解码图片（png/jpg/bmp/gif/webp 由系统编解码器决定），
//! 转成 32bpp 顶行 BGRA，供 Canvas 直接 blit。表格类文件不做栅格化，
//! 由调用方退回「类型徽标 + 文件名」卡片。

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::OnceLock;

#[repr(C)]
struct StartupInput {
    version: u32,
    callback: *const c_void,
    callback_data: *mut c_void,
    suppress_bg: i32,
    suppress_ext: i32,
}

#[repr(C)]
struct BitmapInfoHeader {
    bi_size: u32,
    bi_width: i32,
    bi_height: i32,
    bi_planes: u16,
    bi_bit_count: u16,
    bi_compression: u32,
    bi_size_image: u32,
    bi_xppm: i32,
    bi_yppm: i32,
    bi_clr_used: u32,
    bi_clr_important: u32,
}

#[repr(C)]
struct BitmapInfo {
    header: BitmapInfoHeader,
    colors: [u32; 1],
}

#[link(name = "gdiplus")]
extern "system" {
    fn GdiplusStartup(token: *mut usize, input: *const StartupInput, output: *mut c_void) -> i32;
    fn GdipCreateBitmapFromFile(filename: *const u16, bitmap: *mut *mut c_void) -> i32;
    fn GdipGetImageWidth(image: *mut c_void, width: *mut u32) -> i32;
    fn GdipGetImageHeight(image: *mut c_void, height: *mut u32) -> i32;
    fn GdipCreateHBITMAPFromBitmap(bitmap: *mut c_void, hbm: *mut isize, background: u32) -> i32;
    fn GdipDisposeImage(image: *mut c_void) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    fn CreateCompatibleDC(hdc: isize) -> isize;
    fn DeleteDC(hdc: isize) -> i32;
    fn GetDIBits(
        hdc: isize,
        hbm: isize,
        start: u32,
        clines: u32,
        bits: *mut c_void,
        bmi: *mut BitmapInfo,
        usage: u32,
    ) -> i32;
}

#[link(name = "user32")]
extern "system" {
    fn GetDC(hwnd: isize) -> isize;
    fn ReleaseDC(hwnd: isize, hdc: isize) -> i32;
    fn DeleteObject(hobj: isize) -> i32;
}

static TOKEN: OnceLock<usize> = OnceLock::new();

fn ensure() {
    TOKEN.get_or_init(|| unsafe {
        let mut t = 0usize;
        let inp = StartupInput {
            version: 1,
            callback: std::ptr::null(),
            callback_data: std::ptr::null_mut(),
            suppress_bg: 0,
            suppress_ext: 0,
        };
        let _ = GdiplusStartup(&mut t, &inp, std::ptr::null_mut());
        t
    });
}

pub struct Thumb {
    pub w: i32,
    pub h: i32,
    /// 0xAARRGGBB，非预乘（已用白底合成，alpha 恒 255）
    pub px: Vec<u32>,
}

thread_local! {
    static CACHE: RefCell<HashMap<String, Option<Rc<Thumb>>>> = RefCell::new(HashMap::new());
}

/// 加载并缓存一张缩略图；`want` 是期望的最长边（GDI+ 按原始尺寸解码，缩放交给 blit）。
pub fn load(path: &str, want: i32) -> Option<Rc<Thumb>> {
    let key = format!("{}|{}", path, want);
    if let Some(v) = CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return v;
    }
    let got = unsafe { decode(path) };
    let got = got.map(Rc::new);
    CACHE.with(|c| {
        c.borrow_mut().insert(key, got.clone());
    });
    got
}

unsafe fn decode(path: &str) -> Option<Thumb> {
    ensure();
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut img: *mut c_void = std::ptr::null_mut();
    if GdipCreateBitmapFromFile(wide.as_ptr(), &mut img) != 0 || img.is_null() {
        return None;
    }
    let (mut w, mut h) = (0u32, 0u32);
    let _ = GdipGetImageWidth(img, &mut w);
    let _ = GdipGetImageHeight(img, &mut h);
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        let _ = GdipDisposeImage(img);
        return None;
    }
    let mut hbm: isize = 0;
    // 白底合成：截图/表格缩略图没有 alpha 通道，避免透明处变黑
    if GdipCreateHBITMAPFromBitmap(img, &mut hbm, 0xFFFF_FFFF) != 0 || hbm == 0 {
        let _ = GdipDisposeImage(img);
        return None;
    }
    let _ = GdipDisposeImage(img);

    let mut bmi = BitmapInfo {
        header: BitmapInfoHeader {
            bi_size: std::mem::size_of::<BitmapInfoHeader>() as u32,
            bi_width: w as i32,
            bi_height: -(h as i32), // 负数 = 顶行优先
            bi_planes: 1,
            bi_bit_count: 32,
            bi_compression: 0,
            bi_size_image: 0,
            bi_xppm: 0,
            bi_yppm: 0,
            bi_clr_used: 0,
            bi_clr_important: 0,
        },
        colors: [0],
    };
    let mut px = vec![0u32; (w * h) as usize];
    let screen = GetDC(0);
    let dc = CreateCompatibleDC(screen);
    let lines = GetDIBits(
        dc,
        hbm,
        0,
        h,
        px.as_mut_ptr() as *mut c_void,
        &mut bmi,
        0, // DIB_RGB_COLORS
    );
    let _ = DeleteDC(dc);
    let _ = ReleaseDC(0, screen);
    let _ = DeleteObject(hbm);
    if lines == 0 {
        return None;
    }
    Some(Thumb { w: w as i32, h: h as i32, px })
}

/// 文件扩展名 → 是否当作图片处理
pub fn is_image(path: &str) -> bool {
    let ext = path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp" | "ico")
}
