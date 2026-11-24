//! Prévia das janelas de um app no dock: ao parar o mouse sobre o ícone de um app com duas ou
//! mais janelas (ex.: dois Explorers, um em Imagens e outro em Downloads), abre uma faixa acima
//! do ícone com uma miniatura ao vivo (DWM) e o título de cada janela. Clicar numa miniatura
//! traz aquela janela para frente.
//!
//! O dock avisa onde o mouse está ([`hover`], [`dock_left`], [`dismiss`]); a janela da prévia
//! tem um timer próprio que decide quando abrir (depois de uma pequena espera) e quando fechar
//! (mouse fora da prévia e fora do ícone).

use std::cell::RefCell;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION, DWM_TNP_VISIBLE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND, DwmQueryThumbnailSourceSize, DwmRegisterThumbnail, DwmSetWindowAttribute, DwmUnregisterThumbnail,
    DwmUpdateThumbnailProperties,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW,
    CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER,
    DeleteDC, DeleteObject, DrawTextW, EndPaint, FF_DONTCARE, FW_NORMAL, FillRect, GetMonitorInfoW, HBRUSH, HGDIOBJ,
    InvalidateRect, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, NULL_PEN, OUT_DEFAULT_PRECIS, PAINTSTRUCT,
    RoundRect, SRCCOPY, SelectObject, SetBkMode, SetTextColor, TRANSPARENT, BitBlt, GetStockObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DrawIconEx, GCLP_HICON, GCLP_HICONSM, GetClassLongPtrW,
    GetCursorPos, GetWindowRect, GetWindowTextW, HICON, HWND_TOPMOST, ICON_BIG, ICON_SMALL2, IDC_ARROW, IsWindow,
    KillTimer, LoadCursorW, MA_NOACTIVATE, RegisterClassW, SMTO_ABORTIFHUNG, SWP_NOACTIVATE, SWP_SHOWWINDOW,
    SendMessageTimeoutW, SetTimer, SetWindowPos, ShowWindow, SW_HIDE, WM_ERASEBKGND, WM_GETICON, WM_LBUTTONUP,
    WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP, DI_NORMAL,
};
use windows::core::w;

const CLASS: windows::core::PCWSTR = w!("ArcDockWinPreview");
const WM_MOUSELEAVE: u32 = 0x02A3;
const TIMER: usize = 1;
const TICK_MS: u32 = 60;

/// Mouse parado no ícone por esse tempo antes de a prévia abrir.
const OPEN_DELAY: Duration = Duration::from_millis(350);
/// Com a prévia aberta, mouse fora dela e do ícone por esse tempo fecha.
const CLOSE_DELAY: Duration = Duration::from_millis(280);

// Medidas em px a 96 dpi.
const PAD: i32 = 12;
const GAP: i32 = 8;
const TILE_W: i32 = 216;
const TITLE_H: i32 = 30;
const THUMB_H: i32 = 126;
const TILE_PAD: i32 = 8;
const RADIUS: i32 = 10;
const ABOVE_ICON: i32 = 6;

/// O que o dock está apontando: um app com 2+ janelas.
#[derive(Clone)]
pub struct Target {
    pub key: String,
    pub windows: Vec<HWND>,
    /// Ponto acima do centro do ícone (o mesmo onde o menu de contexto se apoia).
    pub anchor: POINT,
}

struct Tile {
    win: HWND,
    /// Em coordenadas do cliente.
    rect: RECT,
    thumb: isize,
}

struct State {
    hwnd: HWND,
    target: Option<Target>,
    /// Desde quando o dock aponta para o `target` atual.
    since: Instant,
    dock_hover: bool,
    /// Depois de um clique a prévia não reabre até o mouse trocar de ícone ou sair do dock.
    suppressed: bool,
    open: bool,
    key: String,
    anchor: POINT,
    tiles: Vec<Tile>,
    hot: Option<usize>,
    outside_since: Option<Instant>,
    tracking: bool,
    light: bool,
    scale: f32,
}

thread_local! {
    static ST: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    // try_borrow_mut: criar/mover janelas faz o Windows reentrar no wndproc.
    ST.with(|st| st.try_borrow_mut().ok().and_then(|mut st| st.as_mut().map(f)))
}

pub fn is_open() -> bool {
    with(|s| s.open).unwrap_or(false)
}

/// O mouse está sobre o ícone de um app (`Some` se ele tem 2+ janelas) ou sobre outra parte do dock.
pub fn hover(target: Option<Target>) {
    let target = target.filter(|_| crate::config::window_previews());
    if target.is_none() && ST.with(|st| st.try_borrow().map(|s| s.is_none()).unwrap_or(true)) {
        return; // nada aberto e nada a fechar: não cria a janela à toa
    }
    ensure();
    with(|s| {
        s.dock_hover = true;
        let changed = match (&s.target, &target) {
            (Some(a), Some(b)) => a.key != b.key,
            (None, None) => false,
            _ => true,
        };
        if changed {
            s.since = Instant::now();
            s.suppressed = false;
        }
        if let (Some(old), Some(new)) = (&mut s.target, &target) {
            // Mesmo app: mantém lista de janelas e posição em dia.
            if old.key == new.key {
                *old = new.clone();
                return;
            }
        }
        s.target = target;
    });
}

/// O mouse saiu da janela do dock (talvez para dentro da prévia, que decide sozinha).
pub fn dock_left() {
    with(|s| {
        s.dock_hover = false;
        s.target = None;
        s.suppressed = false;
    });
}

/// Clique ou rolagem no dock: fecha a prévia e não reabre até o mouse mudar de ícone.
pub fn dismiss() {
    with(|s| s.suppressed = true);
    hide();
}

pub fn shutdown() {
    hide();
    let hwnd = ST.with(|st| st.try_borrow_mut().ok().and_then(|mut st| st.take()).map(|s| s.hwnd));
    if let Some(hwnd) = hwnd {
        unsafe {
            let _ = KillTimer(Some(hwnd), TIMER);
            let _ = DestroyWindow(hwnd);
        }
    }
}

fn ensure() {
    if ST.with(|st| st.try_borrow().map(|s| s.is_some()).unwrap_or(true)) {
        return;
    }
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else { return };
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: module.into(),
            lpszClassName: CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            CLASS,
            w!("ArcDockWin Preview"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(module.into()),
            None,
        ) else {
            return;
        };
        let round = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &round as *const _ as *const std::ffi::c_void,
            size_of_val(&round) as u32,
        );
        let _ = SetTimer(Some(hwnd), TIMER, TICK_MS, None);
        ST.with(|st| {
            if let Ok(mut st) = st.try_borrow_mut() {
                *st = Some(State {
                    hwnd,
                    target: None,
                    since: Instant::now(),
                    dock_hover: false,
                    suppressed: false,
                    open: false,
                    key: String::new(),
                    anchor: POINT::default(),
                    tiles: Vec::new(),
                    hot: None,
                    outside_since: None,
                    tracking: false,
                    light: false,
                    scale: 1.0,
                });
            }
        });
    }
}

enum Action {
    Show(Target),
    Hide,
}

fn tick() {
    let mut cursor = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut cursor);
    }
    let action = with(|s| {
        let now = Instant::now();
        if !s.open {
            let t = s.target.as_ref()?;
            return (s.dock_hover && !s.suppressed && now - s.since >= OPEN_DELAY).then(|| Action::Show(t.clone()));
        }
        // Aberta: dentro da prévia (e da ponte até o ícone) ou sobre o ícone do mesmo app, fica.
        let mut rc = RECT::default();
        unsafe {
            let _ = GetWindowRect(s.hwnd, &mut rc);
        }
        rc.bottom = rc.bottom.max(s.anchor.y + (12.0 * s.scale) as i32);
        let inside = cursor.x >= rc.left && cursor.x < rc.right && cursor.y >= rc.top && cursor.y < rc.bottom;
        match &s.target {
            Some(t) if s.dock_hover && t.key == s.key => {
                s.outside_since = None;
                return None;
            }
            // Outro app com 2+ janelas: troca a prévia (com um instante de espera).
            Some(t) if s.dock_hover && now - s.since >= Duration::from_millis(150) && !s.suppressed => {
                return Some(Action::Show(t.clone()));
            }
            _ => {}
        }
        if inside {
            s.outside_since = None;
            return None;
        }
        let since = *s.outside_since.get_or_insert(now);
        (now - since >= CLOSE_DELAY).then_some(Action::Hide)
    })
    .flatten();
    match action {
        Some(Action::Show(t)) => show(&t),
        Some(Action::Hide) => hide(),
        None => {
            // Janelas fechadas com a prévia aberta: refaz a lista.
            let stale = with(|s| s.open && s.tiles.iter().any(|t| !unsafe { IsWindow(Some(t.win)) }.as_bool()))
                .unwrap_or(false);
            if stale {
                let wins: Vec<HWND> = with(|s| {
                    s.tiles.iter().map(|t| t.win).filter(|&w| unsafe { IsWindow(Some(w)) }.as_bool()).collect()
                })
                .unwrap_or_default();
                let anchor = with(|s| s.anchor).unwrap_or_default();
                let key = with(|s| s.key.clone()).unwrap_or_default();
                if wins.len() >= 2 {
                    show(&Target { key, windows: wins, anchor });
                } else {
                    hide();
                }
            }
        }
    }
}

fn unregister_thumbs() {
    let thumbs: Vec<isize> = with(|s| s.tiles.iter().map(|t| t.thumb).filter(|&t| t != 0).collect()).unwrap_or_default();
    for t in thumbs {
        unsafe {
            let _ = DwmUnregisterThumbnail(t);
        }
    }
}

fn hide() {
    let Some(hwnd) = with(|s| s.hwnd) else { return };
    unregister_thumbs();
    with(|s| {
        s.open = false;
        s.tiles.clear();
        s.hot = None;
        s.outside_since = None;
        s.since = Instant::now();
    });
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

fn show(t: &Target) {
    let wins: Vec<HWND> = t.windows.iter().copied().filter(|&w| unsafe { IsWindow(Some(w)) }.as_bool()).collect();
    if wins.len() < 2 {
        return;
    }
    let Some(hwnd) = with(|s| s.hwnd) else { return };
    unregister_thumbs();
    crate::tooltip::hide();

    unsafe {
        let monitor = MonitorFromPoint(t.anchor, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(monitor, &mut mi);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let scale = dx as f32 / 96.0;
        let px = |v: i32| (v as f32 * scale).round() as i32;

        let mon = mi.rcMonitor;
        let n = wins.len() as i32;
        let max_cols = ((mon.right - mon.left - 2 * px(PAD) - px(16)) / (px(TILE_W) + px(GAP))).clamp(1, 5);
        let cols = n.min(max_cols);
        let rows = (n + cols - 1) / cols;
        let tile_h = px(TITLE_H) + px(THUMB_H) + px(TILE_PAD);
        let width = 2 * px(PAD) + cols * px(TILE_W) + (cols - 1) * px(GAP);
        let height = 2 * px(PAD) + rows * tile_h + (rows - 1) * px(GAP);

        let mut x = t.anchor.x - width / 2;
        x = x.clamp(mon.left + px(8), (mon.right - width - px(8)).max(mon.left + px(8)));
        let y = (t.anchor.y - height - px(ABOVE_ICON)).max(mon.top + px(8));

        let mut tiles = Vec::with_capacity(wins.len());
        for (i, &win) in wins.iter().enumerate() {
            let (c, r) = (i as i32 % cols, i as i32 / cols);
            let left = px(PAD) + c * (px(TILE_W) + px(GAP));
            let top = px(PAD) + r * (tile_h + px(GAP));
            tiles.push(Tile { win, rect: RECT { left, top, right: left + px(TILE_W), bottom: top + tile_h }, thumb: 0 });
        }

        with(|s| {
            s.key = t.key.clone();
            s.anchor = t.anchor;
            s.scale = scale;
            s.light = crate::light_theme();
            s.hot = None;
            s.outside_since = None;
        });

        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);

        // Miniaturas ao vivo: cabem na área da miniatura mantendo a proporção da janela.
        for tile in &mut tiles {
            let Ok(thumb) = DwmRegisterThumbnail(hwnd, tile.win) else { continue };
            let src = DwmQueryThumbnailSourceSize(thumb).unwrap_or_default();
            if src.cx <= 0 || src.cy <= 0 {
                let _ = DwmUnregisterThumbnail(thumb);
                continue;
            }
            let area = thumb_area(&tile.rect, scale);
            let (aw, ah) = ((area.right - area.left) as f32, (area.bottom - area.top) as f32);
            let k = (aw / src.cx as f32).min(ah / src.cy as f32).min(1.0);
            let (w, h) = ((src.cx as f32 * k).round() as i32, (src.cy as f32 * k).round() as i32);
            let l = area.left + ((area.right - area.left) - w) / 2;
            let tp = area.top + ((area.bottom - area.top) - h) / 2;
            let props = DWM_THUMBNAIL_PROPERTIES {
                dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY,
                rcDestination: RECT { left: l, top: tp, right: l + w, bottom: tp + h },
                fVisible: true.into(),
                opacity: 255,
                ..Default::default()
            };
            if DwmUpdateThumbnailProperties(thumb, &props).is_ok() {
                tile.thumb = thumb;
            } else {
                let _ = DwmUnregisterThumbnail(thumb);
            }
        }

        with(|s| {
            s.tiles = tiles;
            s.open = true;
        });
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn thumb_area(tile: &RECT, scale: f32) -> RECT {
    let px = |v: i32| (v as f32 * scale).round() as i32;
    RECT {
        left: tile.left + px(TILE_PAD),
        top: tile.top + px(TITLE_H),
        right: tile.right - px(TILE_PAD),
        bottom: tile.top + px(TITLE_H) + px(THUMB_H),
    }
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

fn window_icon(win: HWND) -> Option<HICON> {
    unsafe {
        for kind in [ICON_SMALL2, ICON_BIG] {
            let mut out = 0usize;
            let ok = SendMessageTimeoutW(win, WM_GETICON, WPARAM(kind as usize), LPARAM(0), SMTO_ABORTIFHUNG, 30, Some(&mut out));
            if ok.0 != 0 && out != 0 {
                return Some(HICON(out as *mut _));
            }
        }
        for idx in [GCLP_HICONSM, GCLP_HICON] {
            let h = GetClassLongPtrW(win, idx);
            if h != 0 {
                return Some(HICON(h as *mut _));
            }
        }
    }
    None
}

fn title_of(win: HWND) -> Vec<u16> {
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(win, &mut buf) }.max(0) as usize;
    buf[..len].to_vec()
}

unsafe fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rc);
        let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);

        let mem = CreateCompatibleDC(Some(hdc));
        let bmp = CreateCompatibleBitmap(hdc, w, h);
        let old_bmp = SelectObject(mem, HGDIOBJ(bmp.0));

        ST.with(|st| {
            let Ok(st) = st.try_borrow() else { return };
            let Some(s) = st.as_ref() else { return };
            let px = |v: i32| (v as f32 * s.scale).round() as i32;
            let (bg, text, hot, slot) = if s.light {
                (rgb(243, 243, 245), rgb(28, 28, 30), rgb(222, 222, 228), rgb(228, 228, 233))
            } else {
                (rgb(38, 38, 42), rgb(240, 240, 242), rgb(66, 66, 76), rgb(26, 26, 29))
            };

            let brush_bg = CreateSolidBrush(bg);
            FillRect(mem, &rc, brush_bg);
            let _ = DeleteObject(HGDIOBJ(brush_bg.0));

            let font = CreateFontW(
                -px(13), 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY, (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32, w!("Segoe UI"),
            );
            let old_font = SelectObject(mem, HGDIOBJ(font.0));
            SetBkMode(mem, TRANSPARENT);
            SetTextColor(mem, text);
            let old_pen = SelectObject(mem, GetStockObject(NULL_PEN));

            let round = |color: COLORREF, r: &RECT, radius: i32| {
                let b: HBRUSH = CreateSolidBrush(color);
                let old = SelectObject(mem, HGDIOBJ(b.0));
                let _ = RoundRect(mem, r.left, r.top, r.right + 1, r.bottom + 1, radius * 2, radius * 2);
                SelectObject(mem, old);
                let _ = DeleteObject(HGDIOBJ(b.0));
            };

            for (i, tile) in s.tiles.iter().enumerate() {
                if s.hot == Some(i) {
                    round(hot, &tile.rect, px(RADIUS));
                }
                round(slot, &thumb_area(&tile.rect, s.scale), px(6));

                // Ícone do app + título.
                let icon_sz = px(16);
                let icon_x = tile.rect.left + px(TILE_PAD);
                let icon_y = tile.rect.top + (px(TITLE_H) - icon_sz) / 2;
                if let Some(icon) = window_icon(tile.win) {
                    let _ = DrawIconEx(mem, icon_x, icon_y, icon, icon_sz, icon_sz, 0, None, DI_NORMAL);
                }
                let mut title = title_of(tile.win);
                let mut tr = RECT {
                    left: icon_x + icon_sz + px(8),
                    top: tile.rect.top,
                    right: tile.rect.right - px(TILE_PAD),
                    bottom: tile.rect.top + px(TITLE_H),
                };
                DrawTextW(mem, &mut title, &mut tr, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX);
            }

            SelectObject(mem, old_pen);
            SelectObject(mem, old_font);
            let _ = DeleteObject(HGDIOBJ(font.0));
        });

        let _ = BitBlt(hdc, 0, 0, w, h, Some(mem), 0, 0, SRCCOPY);
        SelectObject(mem, old_bmp);
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        let _ = DeleteDC(mem);
        let _ = EndPaint(hwnd, &ps);
    }
}

fn tile_at(x: i32, y: i32) -> Option<usize> {
    with(|s| {
        s.tiles
            .iter()
            .position(|t| x >= t.rect.left && x < t.rect.right && y >= t.rect.top && y < t.rect.bottom)
    })
    .flatten()
}

fn lp_xy(lp: LPARAM) -> (i32, i32) {
    ((lp.0 & 0xffff) as i16 as i32, ((lp.0 >> 16) & 0xffff) as i16 as i32)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_TIMER if wp.0 == TIMER => {
                tick();
                LRESULT(0)
            }
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_MOUSEMOVE => {
                let (x, y) = lp_xy(lp);
                let hot = tile_at(x, y);
                let changed = with(|s| {
                    let changed = s.hot != hot;
                    s.hot = hot;
                    if !s.tracking {
                        s.tracking = true;
                        let mut tme = TRACKMOUSEEVENT {
                            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        };
                        let _ = TrackMouseEvent(&mut tme);
                    }
                    changed
                })
                .unwrap_or(false);
                if changed {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                let changed = with(|s| {
                    s.tracking = false;
                    s.hot.take().is_some()
                })
                .unwrap_or(false);
                if changed {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let (x, y) = lp_xy(lp);
                if let Some(i) = tile_at(x, y) {
                    let win = with(|s| s.tiles.get(i).map(|t| t.win)).flatten();
                    dismiss();
                    if let Some(win) = win {
                        crate::apps::focus_window(win);
                    }
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
