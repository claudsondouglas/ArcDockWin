//! Balão com o nome do app acima do ícone sob o mouse, como no Dock do macOS: pílula escura
//! (clara no tema claro) com uma setinha apontando para o ícone. Janela em camadas que não
//! recebe cliques; o dock chama [`show`] a cada movimento/quadro e [`hide`] ao sair.

use std::cell::RefCell;

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NONE;
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP, IDWriteFontCollection,
};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION, CreateCompatibleDC, DeleteDC, DeleteObject, GetMonitorInfoW, HDC,
    HGDIOBJ, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, w};
use windows_numerics::Vector2;

const CLASS: windows::core::PCWSTR = w!("ArcDockWinTooltip");
const FONT: f32 = 13.0;
const PAD_X: f32 = 10.0;
const BOX_H: f32 = 26.0;
const RADIUS: f32 = 7.0;
const ARROW_W: f32 = 7.0;
const ARROW_H: f32 = 6.0;
/// Espaço entre a ponta da seta e o topo do ícone.
const GAP: f32 = 4.0;

struct Surface {
    memdc: HDC,
    dib: HGDIOBJ,
    w: i32,
    h: i32,
    /// Altura da pílula (sem a seta): é a parte com vidro.
    box_h: i32,
}

struct State {
    hwnd: HWND,
    /// Janela de vidro atrás da pílula (desfoque do DWM); `hwnd` pertence a ela e fica por cima.
    glass: HWND,
    fx: Option<crate::glass::Glass>,
    /// Texto, escala e tema do que está desenhado (redesenha só se mudar).
    drawn: Option<(String, u32, bool)>,
    surface: Option<Surface>,
    pos: POINT,
    visible: bool,
}

thread_local! {
    static ST: RefCell<Option<State>> = const { RefCell::new(None) };
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn ensure() -> Option<HWND> {
    if let Some(h) = ST.with_borrow(|s| s.as_ref().map(|s| s.hwnd)) {
        return Some(h);
    }
    unsafe {
        let module = GetModuleHandleW(None).ok()?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: module.into(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassW(&wc);
        // Classe do vidro registrada pelo dock: sem superfície GDI, o conteúdo vem da composição.
        let glass = CreateWindowExW(
            WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
            w!("ArcDockWinGlass"),
            w!("ArcDockWin Tooltip Glass"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(module.into()),
            None,
        )
        .ok()?;
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            CLASS,
            w!("ArcDockWin Tooltip"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            Some(glass),
            None,
            Some(module.into()),
            None,
        )
        .ok()?;
        let fx = crate::glass::Glass::new(glass, RADIUS).ok();
        ST.with_borrow_mut(|s| {
            *s = Some(State { hwnd, glass, fx, drawn: None, surface: None, pos: POINT::default(), visible: false })
        });
        Some(hwnd)
    }
}

/// Mostra (ou move) o balão com `text`; `anchor` é o ponto da tela logo acima do centro do ícone.
pub fn show(text: &str, anchor: POINT, scale: f32, light: bool) {
    if text.is_empty() {
        return hide();
    }
    let Some(hwnd) = ensure() else { return };
    let key = (text.to_string(), (scale * 100.0).round() as u32, light);
    let stale = ST.with_borrow(|s| s.as_ref().is_some_and(|s| s.drawn.as_ref() != Some(&key)));
    if stale {
        let surface = draw(text, scale, light);
        ST.with_borrow_mut(|s| {
            let Some(s) = s.as_mut() else { return };
            if let Some(old) = s.surface.take() {
                unsafe {
                    let _ = DeleteDC(old.memdc);
                    let _ = DeleteObject(old.dib);
                }
            }
            if let Some(fx) = &s.fx {
                fx.set_radius(RADIUS * scale);
            }
            s.surface = surface;
            s.drawn = Some(key);
            s.visible = false; // força o UpdateLayeredWindow com a superfície nova
        });
    }

    ST.with_borrow_mut(|s| {
        let Some(s) = s.as_mut() else { return };
        let Some(sf) = &s.surface else { return };
        // Ponta da seta em `anchor` (menos o espaço); dentro do monitor.
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        unsafe {
            let _ = GetMonitorInfoW(MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST), &mut mi);
        }
        let m = mi.rcMonitor;
        let mut x = anchor.x - sf.w / 2;
        x = x.clamp(m.left + 4, (m.right - sf.w - 4).max(m.left + 4));
        let y = (anchor.y - (GAP * scale).round() as i32 - sf.h).max(m.top);
        let pos = POINT { x, y };
        if s.visible && pos.x == s.pos.x && pos.y == s.pos.y {
            return;
        }
        unsafe {
            let flags = SWP_NOACTIVATE | if s.visible { SWP_NOZORDER } else { SWP_SHOWWINDOW };
            let _ = SetWindowPos(s.glass, Some(HWND_TOPMOST), x, y, sf.w, sf.box_h, flags);
            if s.visible {
                let _ = SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER);
            } else {
                if let Some(fx) = &s.fx {
                    // O que está atrás mudou desde a última vez: re-amostra o desfoque.
                    fx.resize(sf.w as f32, sf.box_h as f32);
                    fx.refresh();
                }
                let blend = BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: 255,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                };
                let size = SIZE { cx: sf.w, cy: sf.h };
                let _ = UpdateLayeredWindow(
                    hwnd,
                    None,
                    Some(&pos),
                    Some(&size),
                    Some(sf.memdc),
                    Some(&POINT::default()),
                    COLORREF(0),
                    Some(&blend),
                    ULW_ALPHA,
                );
                let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, 0, 0, SWP_NOACTIVATE | SWP_NOSIZE | SWP_SHOWWINDOW);
            }
        }
        s.pos = pos;
        s.visible = true;
    });
}

pub fn hide() {
    let wins = ST.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        std::mem::replace(&mut s.visible, false).then_some((s.hwnd, s.glass))
    });
    if let Some((hwnd, glass)) = wins {
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
            let _ = ShowWindow(glass, SW_HIDE);
        }
    }
}

pub fn shutdown() {
    if let Some(s) = ST.with_borrow_mut(|s| s.take()) {
        unsafe {
            if let Some(sf) = s.surface {
                let _ = DeleteDC(sf.memdc);
                let _ = DeleteObject(sf.dib);
            }
            drop(s.fx);
            let _ = DestroyWindow(s.hwnd);
            let _ = DestroyWindow(s.glass);
        }
    }
}

fn solid([r, g, b, a]: [u8; 4]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(r, g, b, a);
    p.anti_alias = true;
    p
}

/// Pílula + seta embaixo, num só contorno (para a borda passar sem emenda).
fn bubble(w: f32, h: f32, r: f32, aw: f32, ah: f32) -> Option<tiny_skia::Path> {
    let (x, y) = (0.5, 0.5);
    let (w, bh) = (w - 1.0, h - ah - 1.0);
    let k = r * 0.5523;
    let cx = x + w / 2.0;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + bh - r);
    pb.cubic_to(x + w, y + bh - r + k, x + w - r + k, y + bh, x + w - r, y + bh);
    pb.line_to(cx + aw, y + bh);
    pb.line_to(cx, y + bh + ah);
    pb.line_to(cx - aw, y + bh);
    pb.line_to(x + r, y + bh);
    pb.cubic_to(x + r - k, y + bh, x, y + bh - r + k, x, y + bh - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

fn draw(text: &str, scale: f32, light: bool) -> Option<Surface> {
    let (target, dwrite) = crate::topbar::shared_d2d().ok()?;
    let wide: Vec<u16> = text.encode_utf16().collect();
    unsafe {
        let locale = HSTRING::from(crate::i18n::text_locale());
        let fmt = dwrite
            .CreateTextFormat(
                w!("Segoe UI"),
                None::<&IDWriteFontCollection>,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                FONT * scale,
                windows::core::PCWSTR::from_raw(locale.as_ptr()),
            )
            .ok()?;
        let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        let _ = fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        let _ = fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
        let probe = dwrite.CreateTextLayout(&wide, &fmt, 4000.0, 100.0).ok()?;
        let mut m = DWRITE_TEXT_METRICS::default();
        probe.GetMetrics(&mut m).ok()?;

        let box_h = (BOX_H * scale).round();
        let arrow_h = (ARROW_H * scale).round();
        let w = (m.width + 2.0 * PAD_X * scale).ceil().min(600.0 * scale) as i32;
        let h = (box_h + arrow_h) as i32;
        let mut pm = Pixmap::new(w as u32, h as u32)?;
        let (fill, border, ink) = if light {
            ([250, 250, 252, 150], [0, 0, 0, 40], D2D1_COLOR_F { r: 0.1, g: 0.1, b: 0.12, a: 1.0 })
        } else {
            ([40, 40, 44, 150], [255, 255, 255, 58], D2D1_COLOR_F { r: 0.96, g: 0.96, b: 0.97, a: 1.0 })
        };
        let path = bubble(w as f32, h as f32, RADIUS * scale, ARROW_W * scale, arrow_h)?;
        pm.fill(Color::TRANSPARENT);
        pm.fill_path(&path, &solid(fill), FillRule::Winding, Transform::identity(), None);
        let stroke = Stroke { width: scale.max(1.0), ..Default::default() };
        pm.stroke_path(&path, &solid(border), &stroke, Transform::identity(), None);

        let (dib, bits) = crate::dib(w, h);
        let memdc = CreateCompatibleDC(None);
        SelectObject(memdc, dib);
        // RGBA (tiny-skia) -> BGRA (GDI), ambos pré-multiplicados.
        let src = pm.data();
        let dst = std::slice::from_raw_parts_mut(bits, src.len());
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            d.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
        }
        if target.BindDC(memdc, &RECT { left: 0, top: 0, right: w, bottom: h }).is_ok()
            && let Ok(layout) = dwrite.CreateTextLayout(&wide, &fmt, w as f32, box_h)
            && let Ok(brush) = target.CreateSolidColorBrush(&ink, None)
        {
            target.BeginDraw();
            target.DrawTextLayout(Vector2 { X: 0.0, Y: 0.0 }, &layout, &brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
            let _ = target.EndDraw(None, None);
        }
        Some(Surface { memdc, dib, w, h, box_h: box_h as i32 })
    }
}

