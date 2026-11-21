//! Dock-directed window minimize animations and their user-selectable styles.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};
use windows::Win32::Foundation::{
    COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, BitBlt,
    CAPTUREBLT, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject,
    GetWindowDC, HBITMAP, HDC, HGDIOBJ, ROP_CODE, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

const DURATION: Duration = Duration::from_millis(420);
const BANDS: usize = 12;
static SELECTED: AtomicU8 = AtomicU8::new(u8::MAX);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Effect {
    Genie = 0,
    Scale = 1,
    Suck = 2,
}

impl Effect {
    pub fn label(self) -> &'static str {
        match self {
            Self::Genie => "Efeito Gênio",
            Self::Scale => "Efeito Escala",
            Self::Suck => "Efeito Sucção",
        }
    }

    fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "scale" => Self::Scale,
            "suck" => Self::Suck,
            _ => Self::Genie,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Genie => "genie",
            Self::Scale => "scale",
            Self::Suck => "suck",
        }
    }
}

fn settings_path() -> std::path::PathBuf {
    crate::apps::config_path().with_file_name("minimize_effect.txt")
}

pub fn effect() -> Effect {
    match SELECTED.load(Ordering::Acquire) {
        0 => Effect::Genie,
        1 => Effect::Scale,
        2 => Effect::Suck,
        _ => {
            let effect = std::fs::read_to_string(settings_path())
                .map(|value| Effect::parse(&value))
                .unwrap_or(Effect::Genie);
            SELECTED.store(effect as u8, Ordering::Release);
            effect
        }
    }
}

pub fn set_effect(effect: Effect) {
    SELECTED.store(effect as u8, Ordering::Release);
    if let Some(parent) = settings_path().parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(settings_path(), effect.name());
}

pub struct Animator {
    animations: Vec<WindowAnimation>,
    overlay: Option<Overlay>,
}

impl Animator {
    pub fn new() -> Self {
        Self {
            animations: Vec::new(),
            overlay: None,
        }
    }

    pub fn is_active(&self) -> bool {
        !self.animations.is_empty()
    }

    pub fn has_window(&self, hwnd: HWND) -> bool {
        self.animations
            .iter()
            .any(|animation| animation.hwnd == hwnd)
    }

    pub fn add(&mut self, hwnd: HWND, image: Pixmap, source: RECT, target: RECT, effect: Effect) {
        if self.has_window(hwnd) || source.right <= source.left || source.bottom <= source.top {
            return;
        }
        self.animations
            .push(WindowAnimation::new(hwnd, image, source, target, effect));
    }

    /// Redraws all active previews into one click-through overlay window.
    pub fn step(&mut self) -> bool {
        let now = Instant::now();
        self.animations
            .retain(|animation| animation.progress(now) < 1.0);
        if self.animations.is_empty() {
            if let Some(overlay) = &self.overlay {
                overlay.hide();
            }
            return false;
        }

        let frames: Vec<Frame<'_>> = self
            .animations
            .iter()
            .filter_map(|a| a.frame(now))
            .collect();
        if frames.is_empty() {
            self.animations.clear();
            if let Some(overlay) = &self.overlay {
                overlay.hide();
            }
            return false;
        }
        if self.overlay.is_none() {
            self.overlay = Overlay::new();
        }
        if let Some(overlay) = &mut self.overlay {
            overlay.draw(&frames);
        } else {
            self.animations.clear();
        }
        !self.animations.is_empty()
    }
}

/// Captures a visible window before Windows hides it. The caller supplies the dock icon target.
pub fn capture(hwnd: HWND) -> Option<(Pixmap, RECT)> {
    unsafe {
        if hwnd.is_invalid() || IsIconic(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() {
            return None;
        }
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).ok()?;
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        if !(96..=8192).contains(&w)
            || !(72..=8192).contains(&h)
            || (w as i64) * (h as i64) > 16_777_216
        {
            return None;
        }

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let dst = CreateCompatibleDC(None);
        if dst.0.is_null() {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            return None;
        }
        let old = SelectObject(dst, HGDIOBJ(bitmap.0));
        let src = GetWindowDC(Some(hwnd));
        let copied = if src.0.is_null() {
            false
        } else {
            BitBlt(
                dst,
                0,
                0,
                w,
                h,
                Some(src),
                0,
                0,
                ROP_CODE(SRCCOPY.0 | CAPTUREBLT.0),
            )
            .is_ok()
        };
        if !src.0.is_null() {
            let _ = ReleaseDC(Some(hwnd), src);
        }
        let _ = SelectObject(dst, old);
        let _ = DeleteDC(dst);

        if !copied || bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            return None;
        }
        let count = (w as usize).checked_mul(h as usize)?;
        let bgra = std::slice::from_raw_parts(bits as *const u8, count * 4);
        let mut rgba = vec![0u8; count * 4];
        for (src, dst) in bgra.chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
            dst.copy_from_slice(&[src[2], src[1], src[0], 255]);
        }
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        Some((
            Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(w as u32, h as u32)?)?,
            rect,
        ))
    }
}

struct WindowAnimation {
    hwnd: HWND,
    source: RECT,
    target: RECT,
    started: Instant,
    duration: Duration,
    parts: Parts,
}

enum Parts {
    Full(Pixmap),
    Vertical(Vec<Segment>),
    Horizontal(Vec<Segment>),
}

struct Segment {
    image: Pixmap,
    offset: f32,
    fraction: f32,
}

impl WindowAnimation {
    fn new(hwnd: HWND, image: Pixmap, source: RECT, target: RECT, effect: Effect) -> Self {
        let parts = match effect {
            Effect::Scale => Parts::Full(image),
            Effect::Genie => Parts::Vertical(split(&image, BANDS, true)),
            Effect::Suck => Parts::Horizontal(split(&image, BANDS, false)),
        };
        let duration = match effect {
            Effect::Genie => DURATION + Duration::from_millis(80),
            Effect::Scale => DURATION,
            Effect::Suck => DURATION - Duration::from_millis(40),
        };
        Self {
            hwnd,
            source,
            target,
            started: Instant::now(),
            duration,
            parts,
        }
    }

    fn progress(&self, now: Instant) -> f32 {
        (now.saturating_duration_since(self.started).as_secs_f32() / self.duration.as_secs_f32())
            .clamp(0.0, 1.0)
    }

    fn frame(&self, now: Instant) -> Option<Frame<'_>> {
        let t = self.progress(now);
        let sw = (self.source.right - self.source.left) as f32;
        let sh = (self.source.bottom - self.source.top) as f32;
        let tw = (self.target.right - self.target.left) as f32;
        let th = (self.target.bottom - self.target.top) as f32;
        let sx = self.source.left as f32;
        let sy = self.source.top as f32;
        let tx = self.target.left as f32;
        let ty = self.target.top as f32;
        let mut pieces = Vec::new();

        match &self.parts {
            Parts::Full(image) => {
                let p = ease_out_cubic(t);
                let (w, h) = (lerp(sw, tw, p), lerp(sh, th, p));
                let rect = FRect {
                    x: lerp(sx, tx + (tw - w) / 2.0, p),
                    y: lerp(sy, ty + (th - h) / 2.0, p),
                    w,
                    h,
                };
                pieces.push(Piece {
                    image,
                    rect,
                    opacity: 1.0,
                });
            }
            Parts::Vertical(segments) => {
                let p = ease_out_cubic(t);
                for segment in segments {
                    let part_w = segment.fraction * sw;
                    let w = lerp(part_w, tw * segment.fraction, p);
                    let h = lerp(sh, th, p);
                    let u = segment.offset + segment.fraction / 2.0;
                    let curve =
                        (u * std::f32::consts::PI).sin() * (t * std::f32::consts::PI).sin() * 54.0;
                    let start_x = sx + segment.offset * sw;
                    let target_center = tx + u * tw;
                    let rect = FRect {
                        x: lerp(start_x, target_center - w / 2.0, p),
                        y: lerp(sy, ty, p) + curve,
                        w,
                        h,
                    };
                    pieces.push(Piece {
                        image: &segment.image,
                        rect,
                        opacity: 1.0,
                    });
                }
            }
            Parts::Horizontal(segments) => {
                // A late acceleration and a curved band-by-band pull distinguish the suction path.
                let p = ease_in_cubic(t);
                for segment in segments {
                    let band_h = segment.fraction * sh;
                    let w = lerp(sw, tw, p);
                    let h = lerp(band_h, th * segment.fraction, p);
                    let u = segment.offset + segment.fraction / 2.0;
                    let curve =
                        (u * std::f32::consts::PI).sin() * (t * std::f32::consts::PI).sin() * 64.0;
                    let rect = FRect {
                        x: lerp(sx, tx, p) + curve,
                        y: lerp(sy + segment.offset * sh, ty + u * th - h / 2.0, p),
                        w,
                        h,
                    };
                    let fade = 1.0 - ((t - 0.88).max(0.0) / 0.12).clamp(0.0, 1.0) * 0.14;
                    pieces.push(Piece {
                        image: &segment.image,
                        rect,
                        opacity: fade,
                    });
                }
            }
        }

        let bounds = pieces.iter().map(|piece| piece.rect).reduce(FRect::union)?;
        Some(Frame {
            bounds: bounds.to_rect()?,
            pieces,
        })
    }
}

fn split(image: &Pixmap, count: usize, vertical: bool) -> Vec<Segment> {
    let axis = (if vertical {
        image.width()
    } else {
        image.height()
    }) as usize;
    (0..count)
        .filter_map(|i| {
            let start = i * axis / count;
            let end = (i + 1) * axis / count;
            if end <= start {
                return None;
            }
            let rect = if vertical {
                tiny_skia::IntRect::from_xywh(
                    start as i32,
                    0,
                    (end - start) as u32,
                    image.height(),
                )?
            } else {
                tiny_skia::IntRect::from_xywh(0, start as i32, image.width(), (end - start) as u32)?
            };
            Some(Segment {
                image: image.clone_rect(rect)?,
                offset: start as f32 / axis as f32,
                fraction: (end - start) as f32 / axis as f32,
            })
        })
        .collect()
}

#[derive(Clone, Copy)]
struct FRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl FRect {
    fn union(self, other: Self) -> Self {
        let l = self.x.min(other.x);
        let t = self.y.min(other.y);
        let r = (self.x + self.w).max(other.x + other.w);
        let b = (self.y + self.h).max(other.y + other.h);
        Self {
            x: l,
            y: t,
            w: r - l,
            h: b - t,
        }
    }

    fn to_rect(self) -> Option<RECT> {
        let left = self.x.floor() as i32;
        let top = self.y.floor() as i32;
        let right = (self.x + self.w).ceil() as i32;
        let bottom = (self.y + self.h).ceil() as i32;
        (right > left && bottom > top).then_some(RECT {
            left,
            top,
            right,
            bottom,
        })
    }
}

struct Piece<'a> {
    image: &'a Pixmap,
    rect: FRect,
    opacity: f32,
}

struct Frame<'a> {
    bounds: RECT,
    pieces: Vec<Piece<'a>>,
}

struct Overlay {
    hwnd: HWND,
    dc: HDC,
    bitmap: Option<HBITMAP>,
    bits: *mut u8,
    width: u32,
    height: u32,
    pixmap: Option<Pixmap>,
}

impl Overlay {
    fn new() -> Option<Self> {
        unsafe {
            let instance: HINSTANCE = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
                .ok()?
                .into();
            let class = w!("ArcDockWinMinimizeOverlay");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(overlay_proc),
                hInstance: instance,
                lpszClassName: class,
                ..Default::default()
            };
            let _ = RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST
                    | WS_EX_NOACTIVATE,
                class,
                w!("ArcDockWinMinimizeOverlay"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance),
                None,
            )
            .ok()?;
            let dc = CreateCompatibleDC(None);
            if dc.0.is_null() {
                let _ = DestroyWindow(hwnd);
                return None;
            }
            Some(Self {
                hwnd,
                dc,
                bitmap: None,
                bits: std::ptr::null_mut(),
                width: 0,
                height: 0,
                pixmap: None,
            })
        }
    }

    fn ensure_size(&mut self, w: u32, h: u32) -> bool {
        if w == 0 || h == 0 || (w <= self.width && h <= self.height) {
            return self.pixmap.is_some();
        }
        let (w, h) = (w.max(self.width), h.max(self.height));
        let Some(pixmap) = Pixmap::new(w, h) else {
            return false;
        };
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let bitmap = unsafe { CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) };
        let Ok(bitmap) = bitmap else { return false };
        let old = unsafe { SelectObject(self.dc, HGDIOBJ(bitmap.0)) };
        if let Some(previous) = self.bitmap.replace(bitmap) {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(previous.0));
            }
        } else if !old.0.is_null() {
            // The first selected object is the stock bitmap owned by GDI; it must not be deleted.
        }
        self.bits = bits as *mut u8;
        self.width = w;
        self.height = h;
        self.pixmap = Some(pixmap);
        true
    }

    fn draw(&mut self, frames: &[Frame<'_>]) {
        let Some(bounds) = frames.iter().map(|f| f.bounds).reduce(union_rect) else {
            return;
        };
        let (w, h) = (
            (bounds.right - bounds.left) as u32,
            (bounds.bottom - bounds.top) as u32,
        );
        if !self.ensure_size(w, h) || self.bits.is_null() {
            return;
        }
        let Some(pixmap) = &mut self.pixmap else {
            return;
        };
        let stride = self.width as usize * 4;
        let row_bytes = w as usize * 4;
        for row in pixmap.data_mut().chunks_exact_mut(stride).take(h as usize) {
            row[..row_bytes].fill(0);
        }
        for frame in frames {
            for piece in &frame.pieces {
                let rect = piece.rect;
                let sx = rect.w / piece.image.width() as f32;
                let sy = rect.h / piece.image.height() as f32;
                let transform = Transform::from_row(
                    sx,
                    0.0,
                    0.0,
                    sy,
                    rect.x - bounds.left as f32,
                    rect.y - bounds.top as f32,
                );
                let paint = PixmapPaint {
                    opacity: piece.opacity,
                    quality: FilterQuality::Bicubic,
                    ..Default::default()
                };
                pixmap.draw_pixmap(0, 0, piece.image.as_ref(), &paint, transform, None);
            }
        }

        let rgba = pixmap.data();
        let dest = unsafe {
            std::slice::from_raw_parts_mut(
                self.bits,
                self.width as usize * self.height as usize * 4,
            )
        };
        for y in 0..h as usize {
            let src_row = &rgba[y * stride..y * stride + row_bytes];
            let dst_row = &mut dest[y * stride..y * stride + row_bytes];
            for (src, dst) in src_row.chunks_exact(4).zip(dst_row.chunks_exact_mut(4)) {
                dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]);
            }
        }

        let size = SIZE {
            cx: w as i32,
            cy: h as i32,
        };
        let position = POINT {
            x: bounds.left,
            y: bounds.top,
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            let _ = UpdateLayeredWindow(
                self.hwnd,
                None,
                Some(&position),
                Some(&size),
                Some(self.dc),
                Some(&POINT::default()),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
    }

    fn hide(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            let _ = DestroyWindow(self.hwnd);
            let _ = DeleteDC(self.dc);
            if let Some(bitmap) = self.bitmap.take() {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
            }
        }
    }
}

unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn union_rect(a: RECT, b: RECT) -> RECT {
    RECT {
        left: a.left.min(b.left),
        top: a.top.min(b.top),
        right: a.right.max(b.right),
        bottom: a.bottom.max(b.bottom),
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn ease_in_cubic(t: f32) -> f32 {
    t.powi(3)
}
