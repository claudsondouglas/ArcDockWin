//! Topbar: barra fina no topo de cada monitor, estilo barra de menus do macOS.
//! Mesmo padrão do dock: janela de vidro atrás + janela em camadas por cima. A tinta, a
//! linha de baixo, o realce e os ícones da bandeja saem do tiny-skia; texto e glifos de
//! status são desenhados com DirectWrite (Direct2D ligado ao memdc da DIB).

use std::cell::{Cell, RefCell};
use std::ffi::c_void;

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Transform};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory,
    ID2D1DCRenderTarget, ID2D1Factory,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory,
    IDWriteFontCollection, IDWriteTextFormat, IDWriteTextLayout,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, EnumDisplayMonitors,
    GetMonitorInfoW, HBITMAP, HDC, HGDIOBJ, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromWindow, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::Shell::{
    ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, ABN_FULLSCREENAPP, ABN_POSCHANGED,
    APPBARDATA, SHAppBarMessage, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, HSTRING, PCWSTR, w};
use windows_numerics::Vector2;

use crate::glass;
use crate::panel::{self, PanelContent};
use crate::status::{self, Audio};

/// Botão usado num clique de ícone da bandeja.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayButton {
    Left,
    Right,
    Double,
}

/// Ícone da bandeja (já em tons de cinza e no tamanho certo, ~16 px × escala).
#[derive(Clone)]
pub struct TrayEntry {
    pub icon: Pixmap,
    pub tip: String,
    /// Chave no banco (`exe#uid`).
    pub key: String,
    /// User preference: keep the icon on the bar (`true`) or in the overflow panel.
    pub shown: bool,
    /// Visível na barra depois de aplicar o limite de espaço do monitor.
    pub visible: bool,
}

/// Callback de clique na bandeja: índice do ícone, botão e ponto (tela) onde menus devem abrir.
pub type TrayClick = fn(index: usize, button: TrayButton, anchor: POINT);

#[derive(Clone, Copy)]
#[repr(usize)]
pub enum AppMenuAction {
    Launcher = 1,
    ControlCenter = 2,
    TaskManager = 3,
    Quit = 4,
}

const WM_APPBAR: u32 = WM_APP + 1;
const WM_MOUSELEAVE: u32 = 0x02A3;
const TIMER_CLOCK: usize = 1;
/// Releitura da cor do app maximizado: logo depois de um evento de janela e, enquanto
/// pintada, de tempos em tempos (o app pode mudar de cor sem mexer na janela).
const TIMER_TINT: usize = 3;
/// Relê CPU/RAM (esquerda) e avisa a Central de Controle aberta.
const TIMER_STATS: usize = 4;
/// Releitura depois de eventos de janela: no máximo uma a cada TINT_THROTTLE_MS
/// (não é reiniciada a cada evento, senão uma rajada adiaria a cor até acabar).
const TIMER_TINT_EVENT: usize = 5;
const TINT_THROTTLE_MS: u32 = 50;
/// Monitores mudaram (conectar, resolução, escala): recria as barras depois de um respiro.
const TIMER_REBUILD: usize = 6;
const WM_APP_MENU_ACTION: u32 = WM_APP + 70;
const REBUILD_MS: u32 = 300;
const STATS_MS: u32 = 2000;
/// Largura (px) de cada fatia do brilho do wallpaper.
const LUMA_BUCKET: usize = 16;
const GLYPH_CPU: u16 = 0xE950;
const GLYPH_RAM: u16 = 0xE964;
const TINT_DEBOUNCE_MS: u32 = 120;
const TINT_REFRESH_MS: u32 = 1500;
/// Segunda leitura antes de gravar a cor de um app novo no banco.
const TINT_CONFIRM_MS: u32 = 400;

/// Altura da barra, espaço interno de cada item e largura de um glifo (px antes da escala).
const HEIGHT: f32 = 24.0;
const PAD: f32 = 6.0;
const GLYPH_W: f32 = 16.0;
const TRAY_PAD: f32 = 4.0;

thread_local! {
    /// Uma barra por monitor; a primeira é a do monitor principal.
    static BARS: RefCell<Vec<Bar>> = const { RefCell::new(Vec::new()) };
    /// Janela da barra principal: nunca muda (destino de mensagens de outros módulos).
    static MAIN: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
    /// Som e CPU/RAM são um só para todas as barras.
    static AUDIO: RefCell<Option<Audio>> = const { RefCell::new(None) };
    static USAGE: RefCell<Option<status::Usage>> = const { RefCell::new(None) };
    static TRAY_CLICK: Cell<Option<TrayClick>> = const { Cell::new(None) };
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    AppMenu,
    Tray(usize),
    /// Seta que abre a bandeja (ícones ocultos e a escolha de quem fica na barra).
    Overflow,
    Clock,
    /// Central de Controle (último à direita): rede, Bluetooth, som, energia, mídia.
    Control,
    /// CPU/RAM status extra; clicking opens Task Manager.
    Stats,
}

/// Item desenhado: faixa horizontal [l, r) e textos (layout, x, opacidade).
struct Item {
    kind: Kind,
    l: f32,
    r: f32,
    text: Vec<(IDWriteTextLayout, f32, f32)>,
}

struct Bar {
    hwnd: HWND,
    /// Janela atrás que só fornece o desfoque (vidro).
    glass: HWND,
    glass_fx: Option<glass::Glass>,
    scale: f32,
    /// Monitor desta barra e o retângulo inteiro dele.
    hmon: HMONITOR,
    monitor: RECT,
    /// Posição atual na tela (dada pelo sistema como AppBar).
    rect: RECT,
    pixmap: Pixmap,
    memdc: HDC,
    dib: HBITMAP,
    bits: *mut u8,
    target: ID2D1DCRenderTarget,
    dwrite: IDWriteFactory,
    text_fmt: IDWriteTextFormat,
    app_fmt: IDWriteTextFormat,
    icon_fmt: IDWriteTextFormat,
    /// Ícones da bandeja (os ocultos ficam na lista, mas fora da barra).
    tray: Vec<TrayEntry>,
    items: Vec<Item>,
    hot: Option<Kind>,
    tracking: bool,
    /// App em tela cheia: a barra sai da frente.
    fullscreen: bool,
    clock: String,
    /// Cor do app maximizado logo abaixo (RGB); None = vidro normal.
    app_color: Option<[u8; 3]>,
    /// Brilho do wallpaper sob a barra, por fatia de LUMA_BUCKET px (vazio = não lido).
    wall: Vec<f32>,
    /// Primeira leitura de um app ainda sem cor no banco: (app, cor), esperando confirmação.
    tint_pending: Option<(String, [u8; 3])>,
    /// Releitura da cor já agendada (TIMER_TINT_EVENT).
    tint_queued: bool,
    cpu: Option<u8>,
    ram: Option<u8>,
}

impl Bar {
    /// `glass_fx`: o vidro de uma barra anterior da mesma janela (só um por janela).
    unsafe fn new(
        hwnd: HWND,
        glass_hwnd: HWND,
        hmon: HMONITOR,
        monitor: RECT,
        glass_fx: Option<glass::Glass>,
    ) -> windows::core::Result<Self> {
        unsafe {
            let scale = monitor_scale(hmon);
            let w = monitor.right - monitor.left;
            let h = (HEIGHT * scale).round() as i32;

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
            let dib = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
            let memdc = CreateCompatibleDC(None);
            SelectObject(memdc, HGDIOBJ(dib.0));

            // Direct2D em software desenha direto na DIB, por cima do que já está nela, sem
            // dispositivo de GPU (96 dpi: 1 DIP = 1 px; a escala vai nos tamanhos).
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };
            let target = factory.CreateDCRenderTarget(&props)?;
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);

            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let mut fonts: Option<IDWriteFontCollection> = None;
            dwrite.GetSystemFontCollection(&mut fonts, false)?;
            let has = |name: PCWSTR| {
                let (mut i, mut exists) = (0u32, BOOL(0));
                fonts.as_ref().is_some_and(|f| f.FindFamilyName(name, &mut i, &mut exists).is_ok() && exists.as_bool())
            };
            let family = if has(w!("Segoe UI Variable Text")) { w!("Segoe UI Variable Text") } else { w!("Segoe UI") };
            let locale = HSTRING::from(crate::i18n::text_locale());
            let locale_ptr = PCWSTR::from_raw(locale.as_ptr());
            let format = |family: PCWSTR, size: f32, weight: DWRITE_FONT_WEIGHT| -> windows::core::Result<IDWriteTextFormat> {
                let f = dwrite.CreateTextFormat(
                    family,
                    None::<&IDWriteFontCollection>,
                    weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size * scale,
                    locale_ptr,
                )?;
                f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
                f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                Ok(f)
            };
            let text_fmt = format(family, 12.0, DWRITE_FONT_WEIGHT_NORMAL)?;
            let app_fmt = format(family, 12.0, DWRITE_FONT_WEIGHT_SEMI_BOLD)?;
            let icon_fmt = format(w!("Segoe Fluent Icons"), 14.0, DWRITE_FONT_WEIGHT_NORMAL)?;
            icon_fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;

            let glass_fx = glass_fx.or_else(|| glass::Glass::new(glass_hwnd, 0.0).ok());
            if let Some(fx) = &glass_fx {
                fx.resize(w as f32, h as f32);
                // Sem desfoque: a barra é transparente na área de trabalho e sólida
                // (cor do app) sobre um app maximizado.
                fx.set_opacity(0.0);
            }

            Ok(Self {
                hwnd,
                glass: glass_hwnd,
                glass_fx,
                scale,
                hmon,
                monitor,
                rect: RECT { left: monitor.left, top: monitor.top, right: monitor.right, bottom: monitor.top + h },
                pixmap: Pixmap::new(w as u32, h as u32).expect("pixmap"),
                memdc,
                dib,
                bits: bits as *mut u8,
                target,
                dwrite,
                text_fmt,
                app_fmt,
                icon_fmt,
                tray: Vec::new(),
                items: Vec::new(),
                hot: None,
                tracking: false,
                fullscreen: false,
                app_color: None,
                wall: Vec::new(),
                tint_pending: None,
                tint_queued: false,
                clock: String::new(),
                cpu: None,
                ram: None,
            })
        }
    }

    fn appbar_data(&self) -> APPBARDATA {
        APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: self.hwnd,
            uCallbackMessage: WM_APPBAR,
            uEdge: ABE_TOP,
            ..Default::default()
        }
    }

    /// Pede a borda de cima ao sistema e põe as duas janelas lá.
    /// Acha o app maximizado embaixo e pega a cor dele: do banco (colordb) se já houver;
    /// senão lê da tela e grava quando duas leituras seguidas concordam.
    fn update_tint(&mut self) {
        let mut next = 0;
        let color = if panel::any_open() || self.fullscreen {
            // Painel aberto cobre a faixa lida: mantém a cor e tenta de novo depois.
            next = TINT_REFRESH_MS;
            self.app_color
        } else if let crate::tint::Below::Maximized(win) = crate::tint::below(self.rect) {
            let key = crate::tint::app_key(win);
            if let Some(c) = key.as_deref().and_then(crate::colordb::get) {
                self.tint_pending = None;
                Some(c)
            } else {
                let c = crate::tint::read(self.rect);
                match (key, c) {
                    (Some(k), Some(c)) => {
                        if self.tint_pending.as_ref().is_some_and(|(pk, pc)| *pk == k && *pc == c) {
                            crate::colordb::set(&k, c);
                            self.tint_pending = None;
                        } else {
                            self.tint_pending = Some((k, c));
                            next = TINT_CONFIRM_MS;
                        }
                    }
                    // Sem chave (UWP): sem banco, relê de tempos em tempos.
                    (None, Some(_)) => next = TINT_REFRESH_MS,
                    _ => self.tint_pending = None,
                }
                c
            }
        } else {
            self.tint_pending = None;
            // Área de trabalho livre embaixo: relê o brilho do wallpaper (com janela solta
            // encostada, mantém a última leitura).
            if matches!(crate::tint::below(self.rect), crate::tint::Below::Desktop)
                && let Some(l) = crate::tint::wallpaper_luma(self.rect, LUMA_BUCKET)
                && l != self.wall
            {
                self.wall = l;
                if self.app_color.is_none() {
                    self.redraw();
                }
            }
            None
        };
        unsafe {
            if next > 0 {
                SetTimer(Some(self.hwnd), TIMER_TINT, next, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), TIMER_TINT);
            }
        }
        if color != self.app_color {
            self.app_color = color;
            self.redraw();
        }
    }

    fn place(&mut self) {
        let (w, h) = (self.pixmap.width() as i32, self.pixmap.height() as i32);
        let mut abd = self.appbar_data();
        let m = self.monitor;
        abd.rc = RECT { left: m.left, top: m.top, right: m.left + w, bottom: m.top + h };
        unsafe {
            SHAppBarMessage(ABM_QUERYPOS, &mut abd);
            abd.rc.bottom = abd.rc.top + h;
            SHAppBarMessage(ABM_SETPOS, &mut abd);
        }
        self.rect = RECT { left: abd.rc.left, top: abd.rc.top, right: abd.rc.left + w, bottom: abd.rc.top + h };
        if self.fullscreen {
            return;
        }
        unsafe {
            let _ = SetWindowPos(
                self.glass,
                None,
                self.rect.left,
                self.rect.top,
                w,
                h,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW,
            );
        }
        self.redraw();
    }

    fn set_fullscreen(&mut self, on: bool) {
        if on == self.fullscreen {
            return;
        }
        self.fullscreen = on;
        unsafe {
            if on {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
                let _ = ShowWindow(self.glass, SW_HIDE);
            } else {
                self.place();
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
        }
    }

    fn set_clock(&mut self, text: &str) {
        if text != self.clock {
            self.clock = text.to_string();
            self.relayout();
            self.redraw();
        }
    }

    fn set_stats(&mut self, cpu: Option<u8>, ram: Option<u8>) {
        if (cpu, ram) != (self.cpu, self.ram) {
            self.cpu = cpu;
            self.ram = ram;
            self.relayout();
            self.redraw();
        }
    }

    fn text_layout(&self, text: &[u16], fmt: &IDWriteTextFormat, w: f32) -> Option<IDWriteTextLayout> {
        let h = self.pixmap.height() as f32 - 1.0;
        unsafe { self.dwrite.CreateTextLayout(text, fmt, w, h).ok() }
    }

    /// Item de glifos (sobrepostos) com a borda direita em `x`; devolve a nova borda.
    fn glyph_item(&self, items: &mut Vec<Item>, x: f32, kind: Kind, glyphs: &[(u16, f32)]) -> f32 {
        let (pad, w) = (PAD * self.scale, GLYPH_W * self.scale);
        let l = x - w - 2.0 * pad;
        let text = glyphs
            .iter()
            .filter_map(|&(g, alpha)| self.text_layout(&[g], &self.icon_fmt, w).map(|t| (t, l + pad, alpha)))
            .collect();
        items.push(Item { kind, l, r: x, text });
        l
    }

    /// Keeps the app menu on the leading side and groups status items on the trailing side.
    /// Tray icons collapse into the overflow panel before they can collide with the app menu.
    fn relayout(&mut self) {
        let s = self.scale;
        let pad = PAD * s;
        let mut items = Vec::new();
        let width = self.pixmap.width() as f32;

        let app_name: Vec<u16> = crate::i18n::tr("ArcDockWin").encode_utf16().collect();
        if let Some(text) = self.text_layout(&app_name, &self.app_fmt, 160.0 * s) {
            let mut metrics = DWRITE_TEXT_METRICS::default();
            let _ = unsafe { text.GetMetrics(&mut metrics) };
            let left = 6.0 * s;
            let right = left + metrics.width.ceil() + 2.0 * pad;
            items.push(Item { kind: Kind::AppMenu, l: left, r: right, text: vec![(text, left + pad, 1.0)] });
        }
        let left_limit = items.last().map_or(6.0 * s, |item| item.r + 8.0 * s);

        // Status extras stay together on the trailing edge: control center, clock, tray, stats.
        let mut x = width - 6.0 * s;
        x = self.glyph_item(&mut items, x, Kind::Control, &[(crate::control::GLYPH, 1.0)]);
        x -= 4.0 * s;

        let clock: Vec<u16> = self.clock.encode_utf16().collect();
        if let Some(text) = self.text_layout(&clock, &self.text_fmt, 1000.0) {
            let mut metrics = DWRITE_TEXT_METRICS::default();
            let _ = unsafe { text.GetMetrics(&mut metrics) };
            let clock_w = metrics.width.ceil() + 2.0 * pad;
            let left = x - clock_w;
            items.push(Item { kind: Kind::Clock, l: left, r: x, text: vec![(text, left + pad, 1.0)] });
            x = left - 4.0 * s;
        }

        let available = (x - left_limit).max(0.0);
        let width_of = |text: &IDWriteTextLayout| {
            let mut metrics = DWRITE_TEXT_METRICS::default();
            let _ = unsafe { text.GetMetrics(&mut metrics) };
            metrics.width.ceil()
        };
        let value_w = self
            .text_layout(&"00%".encode_utf16().collect::<Vec<_>>(), &self.text_fmt, 200.0)
            .map(|text| width_of(&text))
            .unwrap_or(30.0 * s);
        let stats_w = 2.0 * (GLYPH_W * s + 2.0 * s + value_w.max(30.0 * s) + 8.0 * s) + 2.0 * pad;
        let overflow_w = GLYPH_W * s + 2.0 * pad;

        let mut visible: Vec<usize> = self
            .tray
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, entry)| entry.shown)
            .map(|(i, _)| i)
            .collect();
        let total_shown = visible.len();
        let mut has_overflow = self.tray.iter().any(|entry| !entry.shown);
        let mut show_stats = stats_w <= available;
        let icon_width = |i: usize| self.tray[i].icon.width() as f32 + 2.0 * TRAY_PAD * s;

        loop {
            let tray_w: f32 = visible.iter().map(|&i| icon_width(i)).sum();
            let used = tray_w
                + if has_overflow { overflow_w + 4.0 * s } else { 0.0 }
                + if show_stats { stats_w + 6.0 * s } else { 0.0 };
            if used <= available {
                break;
            }
            if visible.pop().is_some() {
                has_overflow = true;
            } else if show_stats {
                show_stats = false;
            } else {
                break;
            }
        }

        for entry in &mut self.tray {
            entry.visible = false;
        }
        for &i in &visible {
            self.tray[i].visible = true;
        }
        let auto_hidden = visible.len() < total_shown;
        has_overflow |= auto_hidden;

        // Index order is stable: the rightmost shell icons stay visible first when space is tight.
        for i in visible {
            let entry = &self.tray[i];
            let left = x - entry.icon.width() as f32 - 2.0 * TRAY_PAD * s;
            items.push(Item { kind: Kind::Tray(i), l: left, r: x, text: Vec::new() });
            x = left;
        }
        if has_overflow {
            x = self.glyph_item(&mut items, x - 2.0 * s, Kind::Overflow, &[(panel::glyph::CHEVRON_DOWN, 0.8)]);
        }

        if show_stats {
            let right = x - 6.0 * s;
            let left = right - stats_w;
            let mut text = Vec::new();
            let mut lx = left + pad;
            for (glyph, value) in [(GLYPH_CPU, self.cpu), (GLYPH_RAM, self.ram)] {
                let value = value.map(|v| format!("{v}%")).unwrap_or_else(|| "–".into());
                if let Some(layout) = self.text_layout(&[glyph], &self.icon_fmt, GLYPH_W * s) {
                    text.push((layout, lx, 0.75));
                    lx += GLYPH_W * s + 2.0 * s;
                }
                if let Some(layout) = self.text_layout(&value.encode_utf16().collect::<Vec<_>>(), &self.text_fmt, 200.0) {
                    text.push((layout, lx, 1.0));
                    lx += value_w.max(30.0 * s) + 8.0 * s;
                }
            }
            items.push(Item { kind: Kind::Stats, l: left, r: right, text });
        }
        self.items = items;
    }

    /// Se o wallpaper sob [l, r) é claro (texto escuro); None se ainda não foi lido.
    fn wall_light(&self, l: f32, r: f32) -> Option<bool> {
        let a = (l.max(0.0) as usize / LUMA_BUCKET).min(self.wall.len().checked_sub(1)?);
        let b = ((r.max(0.0) as usize).div_ceil(LUMA_BUCKET)).clamp(a + 1, self.wall.len());
        let avg = self.wall[a..b].iter().sum::<f32>() / (b - a) as f32;
        Some(avg > 150.0)
    }

    fn hit(&self, x: f32) -> Option<Kind> {
        self.items.iter().find(|i| x >= i.l && x < i.r).map(|i| i.kind)
    }

    fn redraw(&mut self) {
        if self.fullscreen {
            return;
        }
        // Com app maximizado embaixo: fundo sólido na cor dele, sem linha, tinta pelo contraste.
        // 10% mais escura que o app, para separar barra e app.
        let app = self.app_color.map(|c| c.map(|v| (v as f32 * 0.9).round() as u8));
        let light = match app {
            Some([r, g, b]) => 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32 > 150.0,
            None => crate::light_theme(),
        };
        let ink = if light { [0.0, 0.0, 0.0, 0.88] } else { [1.0, 1.0, 1.0, 0.95] };
        // Sobre o app: fundo sólido na cor dele. Área de trabalho: transparente
        // (alfa 1 para continuar recebendo cliques).
        let tint = match app {
            Some([r, g, b]) => [r, g, b, 255],
            None => [0, 0, 0, 1],
        };
        let line = tint;
        // Tinta de cada item: pela cor do app ou pelo brilho do wallpaper sob o item.
        let item_light: Vec<bool> = self
            .items
            .iter()
            .map(|it| match app {
                Some(_) => light,
                None => self.wall_light(it.l, it.r).unwrap_or(light),
            })
            .collect();
        let s = self.scale;
        let (w, h) = (self.pixmap.width() as f32, self.pixmap.height() as f32);
        let id = Transform::identity();

        // Vidro: o desfoque vem da janela de fundo; aqui a tinta, a linha de baixo e o realce.
        let pm = &mut self.pixmap;
        pm.fill(Color::TRANSPARENT);
        if let Some(r) = Rect::from_xywh(0.0, 0.0, w, h - 1.0) {
            pm.fill_rect(r, &solid(tint), id, None);
        }
        if let Some(r) = Rect::from_xywh(0.0, h - 1.0, w, 1.0) {
            pm.fill_rect(r, &solid(line), id, None);
        }
        let active_panel = self.items.iter().find_map(|item| {
            let key = match item.kind {
                Kind::AppMenu => "appmenu",
                Kind::Clock => "clock",
                Kind::Control => "control",
                Kind::Overflow => "tray",
                _ => return None,
            };
            self.item_rect(item.kind)
                .is_some_and(|rect| panel::is_open_at(key, rect))
                .then_some(item.kind)
        });
        let highlighted = self.hot.or(active_panel);
        if let Some((i, it)) = self.items.iter().enumerate().find(|(_, i)| Some(i.kind) == highlighted) {
            let hover = if item_light[i] { [0, 0, 0, 24] } else { [255, 255, 255, 36] };
            let m = 2.0 * s;
            if let Some(p) = rounded_rect(it.l, m, it.r - it.l, h - 1.0 - 2.0 * m, 5.0 * s) {
                pm.fill_path(&p, &solid(hover), FillRule::Winding, id, None);
            }
        }
        for it in &self.items {
            if let Kind::Tray(i) = it.kind
                && let Some(icon) = self.tray.get(i).map(|e| &e.icon)
            {
                let x = ((it.l + it.r - icon.width() as f32) / 2.0).round() as i32;
                let y = ((h - 1.0 - icon.height() as f32) / 2.0).round() as i32;
                pm.draw_pixmap(x, y, icon.as_ref(), &PixmapPaint::default(), id, None);
            }
        }

        // RGBA (tiny-skia) -> BGRA (GDI), ambos com alfa pré-multiplicado.
        let src = self.pixmap.data();
        let dst = unsafe { std::slice::from_raw_parts_mut(self.bits, src.len()) };
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            d.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
        }

        // Texto e glifos por cima, direto na DIB.
        let (wi, hi) = (w as i32, h as i32);
        unsafe {
            if self.target.BindDC(self.memdc, &RECT { left: 0, top: 0, right: wi, bottom: hi }).is_ok() {
                let color = D2D1_COLOR_F { r: ink[0], g: ink[1], b: ink[2], a: ink[3] };
                if let Ok(brush) = self.target.CreateSolidColorBrush(&color, None) {
                    self.target.BeginDraw();
                    for (it, &il) in self.items.iter().zip(&item_light) {
                        let c = if app.is_some() || il == light {
                            color
                        } else if il {
                            D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.88 }
                        } else {
                            D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 }
                        };
                        brush.SetColor(&c);
                        for (t, x, alpha) in &it.text {
                            brush.SetOpacity(*alpha);
                            self.target.DrawTextLayout(Vector2 { X: *x, Y: 0.0 }, t, &brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
                        }
                    }
                    let _ = self.target.EndDraw(None, None);
                }
            }

            let size = SIZE { cx: wi, cy: hi };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let pos = POINT { x: self.rect.left, y: self.rect.top };
            let _ = UpdateLayeredWindow(
                self.hwnd,
                None,
                Some(&pos),
                Some(&size),
                Some(self.memdc),
                Some(&POINT::default()),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }

    /// Retângulo do item na tela (altura inteira da barra).
    fn item_rect(&self, kind: Kind) -> Option<RECT> {
        let it = self.items.iter().find(|i| i.kind == kind)?;
        Some(RECT {
            left: self.rect.left + it.l.round() as i32,
            top: self.rect.top,
            right: self.rect.left + it.r.round() as i32,
            bottom: self.rect.bottom,
        })
    }

    /// Ponto (tela) abaixo do centro do item, onde menus devem abrir.
    fn anchor(&self, kind: Kind) -> POINT {
        let it = self.items.iter().find(|i| i.kind == kind);
        let cx = it.map_or(0.0, |i| (i.l + i.r) / 2.0);
        POINT { x: self.rect.left + cx.round() as i32, y: self.rect.bottom }
    }
}

impl Drop for Bar {
    /// Barras são refeitas quando os monitores mudam: solta a DIB e o DC.
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.memdc);
            let _ = DeleteObject(HGDIOBJ(self.dib.0));
        }
    }
}

/// Glifo de volume da fonte Segoe Fluent Icons.
pub(crate) fn volume_glyph((level, muted): (f32, bool)) -> u16 {
    match level {
        _ if muted => 0xE74F,
        l if l <= 0.0 => 0xE992,
        l if l < 0.34 => 0xE993,
        l if l < 0.67 => 0xE994,
        _ => 0xE995,
    }
}

fn solid([r, g, b, a]: [u8; 4]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(r, g, b, a);
    p.anti_alias = true;
    p
}

pub(crate) fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    // Constante de Bézier para quartos de círculo.
    let k = r * 0.5523;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

/// Painel de cada item de status: chave (para o segundo clique fechar) e o conteúdo.
/// É aqui que se pluga um painel novo.
type PanelFactory = Box<dyn FnOnce() -> Box<dyn PanelContent>>;

fn panel_for(kind: Kind, source_bar: HWND) -> Option<(&'static str, PanelFactory)> {
    Some(match kind {
        Kind::AppMenu => ("appmenu", Box::new(move || Box::new(crate::appmenu::AppMenu::new(source_bar)))),
        Kind::Clock => ("clock", Box::new(|| Box::new(crate::calendar::Calendar::new()))),
        Kind::Control => ("control", Box::new(|| Box::new(crate::control::ControlCenter::new()))),
        Kind::Overflow => ("tray", Box::new(|| Box::new(crate::overflow::Overflow::new()))),
        Kind::Tray(_) | Kind::Stats => return None,
    })
}

/// Janela da barra principal (destino das mensagens de trabalho feito em outra thread).
pub fn hwnd() -> HWND {
    MAIN.get()
}

/// Posts a command from the ArcDockWin app menu to the bar's UI thread.
pub fn post_app_menu_action(action: AppMenuAction, source_bar: HWND) {
    unsafe {
        let _ = PostMessageW(Some(source_bar), WM_APP_MENU_ACTION, WPARAM(action as usize), LPARAM(0));
    }
}

/// Refreshes active-item styling when a popover opens or closes.
pub fn panel_visibility_changed() {
    for_bars(|bar| bar.redraw());
}

/// Barra da janela `hwnd`.
fn with_bar<R>(hwnd: HWND, f: impl FnOnce(&mut Bar) -> R) -> Option<R> {
    // try_borrow_mut: chamadas do Windows podem reentrar no wndproc.
    BARS.with(|b| b.try_borrow_mut().ok().and_then(|mut g| g.iter_mut().find(|b| b.hwnd == hwnd).map(f)))
}

/// Todas as barras.
fn for_bars(mut f: impl FnMut(&mut Bar)) {
    BARS.with(|b| {
        if let Ok(mut g) = b.try_borrow_mut() {
            g.iter_mut().for_each(&mut f);
        }
    });
}

/// Relógio agora em todas as barras e timer até a virada do próximo minuto.
fn update_clock() {
    let (text, ms) = status::clock();
    unsafe { SetTimer(Some(MAIN.get()), TIMER_CLOCK, ms, None) };
    for_bars(|b| b.set_clock(&text));
}

/// Repaint date labels and any open panel after a Windows UI-language change.
pub fn language_changed() {
    update_clock();
    panel::language_changed();
}

fn update_stats() {
    let Some((cpu, ram)) = USAGE.with(|u| u.borrow_mut().as_mut().map(|u| (u.cpu(), u.ram()))) else { return };
    for_bars(|b| b.set_stats(cpu, ram));
}

/// Registra quem trata os cliques nos ícones da bandeja.
pub fn on_tray_click(f: TrayClick) {
    TRAY_CLICK.with(|c| c.set(Some(f)));
}

/// Troca os ícones da bandeja e redesenha.
pub fn set_tray(icons: Vec<TrayEntry>) {
    for_bars(|b| {
        b.tray = icons.clone();
        if matches!(b.hot, Some(Kind::Tray(i)) if i >= b.tray.len()) {
            b.hot = None;
        }
        b.relayout();
        b.redraw();
    });
    // Fora do borrow da barra: o painel lê a lista de novo.
    if panel::is_open("tray") {
        panel::refresh();
    }
}

/// Ícones da bandeja, na ordem dos índices de clique.
pub fn tray_list() -> Vec<TrayEntry> {
    with_bar(MAIN.get(), |b| b.tray.clone()).unwrap_or_default()
}

/// Põe o ícone `i` na barra (`true`) ou dentro da bandeja, e guarda a escolha.
pub fn set_tray_shown(i: usize, shown: bool) {
    let mut key = None;
    for_bars(|b| {
        let Some(e) = b.tray.get_mut(i) else { return };
        e.shown = shown;
        key = Some(e.key.clone());
        b.relayout();
        b.redraw();
    });
    if let Some(k) = key {
        crate::colordb::set_tray_shown(&k, shown);
    }
}

/// Clique no ícone `i` vindo de fora da barra (a bandeja); o menu abre no cursor.
pub fn tray_click_index(i: usize, button: TrayButton) {
    let mut anchor = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut anchor);
    }
    if let Some(f) = TRAY_CLICK.with(|c| c.get()) {
        f(i, button, anchor);
    }
}

/// Item da bandeja sob `x` e o ponto de menu dele.
fn tray_hit(hwnd: HWND, lp: LPARAM) -> Option<(usize, POINT)> {
    with_bar(hwnd, |b| match b.hit(mouse_x(lp)) {
        Some(k @ Kind::Tray(i)) => Some((i, b.anchor(k))),
        _ => None,
    })
    .flatten()
}

fn tray_click(hwnd: HWND, lp: LPARAM, button: TrayButton) {
    // Fora do borrow: o callback pode chamar set_tray.
    if let Some((i, anchor)) = tray_hit(hwnd, lp)
        && let Some(f) = TRAY_CLICK.with(|c| c.get())
    {
        panel::close();
        f(i, button, anchor);
    }
}

fn mouse_x(lp: LPARAM) -> f32 {
    (lp.0 & 0xffff) as i16 as f32
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let main = hwnd == MAIN.get();
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_MOUSEMOVE => {
                with_bar(hwnd, |b| {
                    if !b.tracking {
                        b.tracking = true;
                        let mut tme = TRACKMOUSEEVENT {
                            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        };
                        let _ = TrackMouseEvent(&mut tme);
                    }
                    let hot = b.hit(mouse_x(lp));
                    if hot != b.hot {
                        b.hot = hot;
                        b.redraw();
                    }
                });
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                with_bar(hwnd, |b| {
                    b.tracking = false;
                    if b.hot.take().is_some() {
                        b.redraw();
                    }
                });
                LRESULT(0)
            }
            WM_APP_MENU_ACTION => {
                match wp.0 {
                    x if x == AppMenuAction::Launcher as usize => crate::launcher::toggle(),
                    x if x == AppMenuAction::ControlCenter as usize => {
                        if let Some(rect) = with_bar(hwnd, |bar| bar.item_rect(Kind::Control)).flatten() {
                            panel::toggle("control", rect, || Box::new(crate::control::ControlCenter::new()));
                        }
                    }
                    x if x == AppMenuAction::TaskManager as usize => {
                        let _ = ShellExecuteW(None, w!("open"), w!("taskmgr.exe"), None, None, SW_SHOWNORMAL);
                    }
                    x if x == AppMenuAction::Quit as usize => crate::request_quit(),
                    _ => {}
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let hit = with_bar(hwnd, |b| b.hit(mouse_x(lp)).map(|k| (k, b.item_rect(k)))).flatten();
                match hit {
                    Some((Kind::Tray(_), _)) => tray_click(hwnd, lp, TrayButton::Left),
                    Some((Kind::Stats, _)) => {
                        ShellExecuteW(None, w!("open"), w!("taskmgr.exe"), None, None, SW_SHOWNORMAL);
                    }
                    // Fora do borrow da barra: abrir o painel ativa outra janela.
                    // O painel abre no monitor do item (rect em coordenadas de tela).
                    Some((kind, Some(rect))) => {
                        if let Some((key, make)) = panel_for(kind, hwnd) {
                            panel::toggle(key, rect, make);
                        }
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                tray_click(hwnd, lp, TrayButton::Right);
                LRESULT(0)
            }
            WM_LBUTTONDBLCLK => {
                tray_click(hwnd, lp, TrayButton::Double);
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                // Coordenadas de tela; ±2% de volume por passo da roda sobre a central.
                let x = (lp.0 & 0xffff) as i16 as i32;
                let steps = ((wp.0 >> 16) as u16 as i16) as f32 / WHEEL_DELTA as f32;
                let on_control = with_bar(hwnd, |b| b.hit((x - b.rect.left) as f32) == Some(Kind::Control));
                if on_control == Some(true) {
                    AUDIO.with(|a| {
                        if let Ok(a) = a.try_borrow()
                            && let Some(a) = a.as_ref()
                            && let Some((level, _)) = a.state()
                        {
                            a.set_level(level + 0.02 * steps);
                        }
                    });
                }
                LRESULT(0)
            }
            WM_TIMER => {
                match wp.0 {
                    TIMER_CLOCK => update_clock(),
                    TIMER_STATS => {
                        update_stats();
                        panel::notify(crate::control::TICK, 0);
                    }
                    TIMER_REBUILD => {
                        let _ = KillTimer(Some(hwnd), TIMER_REBUILD);
                        rebuild();
                    }
                    TIMER_TINT => {
                        with_bar(hwnd, |b| b.update_tint());
                    }
                    TIMER_TINT_EVENT => {
                        let _ = KillTimer(Some(hwnd), TIMER_TINT_EVENT);
                        with_bar(hwnd, |b| {
                            b.tint_queued = false;
                            b.update_tint();
                        });
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            status::WM_AUDIO => {
                if wp.0 == 1 {
                    AUDIO.with(|a| {
                        if let Ok(mut a) = a.try_borrow_mut()
                            && let Some(a) = a.as_mut()
                        {
                            a.bind();
                        }
                    });
                }
                panel::notify(msg, wp.0);
                LRESULT(0)
            }
            crate::bluetooth::WM_BT_RADIO
            | crate::bluetooth::WM_BT_PANEL
            | crate::wifi::WM_WIFI
            | crate::control::WM_UPDATE => {
                panel::notify(msg, wp.0);
                LRESULT(0)
            }
            WM_APPBAR => {
                with_bar(hwnd, |b| match wp.0 as u32 {
                    ABN_POSCHANGED => b.place(),
                    // O aviso pode vir para todas as AppBars: só esconde a do monitor em tela cheia.
                    ABN_FULLSCREENAPP if lp.0 != 0 => {
                        if MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTONEAREST) == b.hmon {
                            b.set_fullscreen(true);
                        }
                    }
                    ABN_FULLSCREENAPP => b.set_fullscreen(false),
                    _ => {}
                });
                LRESULT(0)
            }
            WM_DISPLAYCHANGE => {
                SetTimer(Some(MAIN.get()), TIMER_REBUILD, REBUILD_MS, None);
                LRESULT(0)
            }
            WM_DPICHANGED => {
                // A posição vem do monitor (rebuild), não do retângulo sugerido.
                let dpi = (wp.0 & 0xffff) as f32;
                if with_bar(hwnd, |b| b.scale != dpi / 96.0) != Some(false) {
                    SetTimer(Some(MAIN.get()), TIMER_REBUILD, REBUILD_MS, None);
                }
                LRESULT(0)
            }
            WM_SETTINGCHANGE => {
                // Pode ser a troca de tema claro/escuro ou de wallpaper.
                with_bar(hwnd, |b| {
                    b.redraw();
                    SetTimer(Some(hwnd), TIMER_TINT, 300, None);
                });
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            // Chegam a todas as barras: só a principal trata.
            WM_TIMECHANGE if main => {
                update_clock();
                LRESULT(0)
            }
            WM_POWERBROADCAST if main => {
                // Volta da suspensão ou mudança na energia: relógio e bateria na hora.
                update_clock();
                panel::notify(crate::control::TICK, 0);
                LRESULT(1)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// Escala (DPI efetivo / 96) do monitor.
fn monitor_scale(hmon: HMONITOR) -> f32 {
    let (mut dx, mut dy) = (96u32, 96u32);
    let _ = unsafe { GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) };
    dx as f32 / 96.0
}

/// Monitores ativos (handle e retângulo inteiro), o principal primeiro.
fn monitors() -> Vec<(HMONITOR, RECT)> {
    unsafe extern "system" fn add(hmon: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> BOOL {
        let list = unsafe { &mut *(lp.0 as *mut Vec<(HMONITOR, RECT)>) };
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if unsafe { GetMonitorInfoW(hmon, &mut mi) }.as_bool() {
            let entry = (hmon, mi.rcMonitor);
            if mi.dwFlags & MONITORINFOF_PRIMARY != 0 {
                list.insert(0, entry);
            } else {
                list.push(entry);
            }
        }
        BOOL(1)
    }
    let mut list = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(add), LPARAM(&mut list as *mut _ as isize));
    }
    list
}

/// Par de janelas de uma barra (vidro atrás, conteúdo em camadas à frente), já como AppBar.
unsafe fn new_windows() -> Option<(HWND, HWND)> {
    unsafe {
        let hinst: HINSTANCE = GetModuleHandleW(None).ok()?.into();
        let ex = WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE;
        // Vidro: sem superfície GDI, o conteúdo vem só da composição (glass.rs).
        let glass = CreateWindowExW(
            ex | WS_EX_NOREDIRECTIONBITMAP,
            w!("ArcDockWinGlass"),
            w!("ArcDockWinTopbarGlass"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinst),
            None,
        )
        .ok()?;
        // Conteúdo: janela em camadas, "dona" pelo vidro para ficar sempre à frente dele.
        let class = w!("ArcDockWinTopbar");
        let Ok(hwnd) =
            CreateWindowExW(ex | WS_EX_LAYERED, class, class, WS_POPUP, 0, 0, 0, 0, Some(glass), None, Some(hinst), None)
        else {
            let _ = DestroyWindow(glass);
            return None;
        };
        // AppBar na borda de cima: janelas maximizadas ficam abaixo da barra.
        let mut abd = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: hwnd,
            uCallbackMessage: WM_APPBAR,
            uEdge: ABE_TOP,
            ..Default::default()
        };
        SHAppBarMessage(ABM_NEW, &mut abd);
        Some((hwnd, glass))
    }
}

/// Tira a AppBar do sistema e destrói as janelas.
fn remove(mut bar: Bar) {
    let mut abd = bar.appbar_data();
    bar.glass_fx = None;
    unsafe {
        SHAppBarMessage(ABM_REMOVE, &mut abd);
        let _ = DestroyWindow(bar.hwnd);
        let _ = DestroyWindow(bar.glass);
    }
}

/// Uma barra por monitor. A do principal é sempre a primeira e mantém a janela (MAIN);
/// barras de monitores que mudaram de área ou escala são refeitas nas mesmas janelas.
fn rebuild() {
    let mons = monitors();
    if mons.is_empty() {
        return;
    }
    // Fora de BARS durante a troca: mensagens reentrantes só são ignoradas.
    let Some(mut old) = BARS.with(|b| b.try_borrow_mut().ok().map(|mut g| std::mem::take(&mut *g))) else {
        return;
    };
    let shared = old.first().map(|b| (b.tray.clone(), b.clock.clone(), b.cpu, b.ram));
    if old.len() > mons.len() {
        old.drain(mons.len()..).for_each(remove);
    }
    let mut old = old.into_iter();
    let mut bars = Vec::with_capacity(mons.len());
    for &(hmon, rc) in &mons {
        let mut prev = old.next();
        if prev.as_ref().is_some_and(|b| b.monitor == rc && b.scale == monitor_scale(hmon))
            && let Some(mut b) = prev.take()
        {
            b.hmon = hmon;
            b.place();
            bars.push(b);
            continue;
        }
        // Mesmas janelas (e vidro) da barra anterior nesta posição, ou janelas novas.
        let (hwnd, glass, fx) = match prev {
            Some(mut b) => (b.hwnd, b.glass, b.glass_fx.take()),
            None => match unsafe { new_windows() } {
                Some((h, g)) => (h, g, None),
                None => continue,
            },
        };
        let Ok(mut b) = (unsafe { Bar::new(hwnd, glass, hmon, rc, fx) }) else {
            continue;
        };
        if let Some((tray, clock, cpu, ram)) = &shared {
            (b.tray, b.clock, b.cpu, b.ram) = (tray.clone(), clock.clone(), *cpu, *ram);
        }
        b.relayout();
        b.place();
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            SetTimer(Some(hwnd), TIMER_TINT, TINT_DEBOUNCE_MS, None);
        }
        bars.push(b);
    }
    if let Some(b) = bars.first() {
        MAIN.set(b.hwnd);
    }
    BARS.with(|b| *b.borrow_mut() = bars);
}

/// Cria as topbars (depois do dock: reaproveita a classe da janela de vidro dele).
pub fn create() {
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else { return };
        let wc = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            hInstance: module.into(),
            lpszClassName: w!("ArcDockWinTopbar"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        rebuild();
        let hwnd = MAIN.get();
        if hwnd.is_invalid() {
            return;
        }
        // Avisa mudanças de som (WM_AUDIO); a roda do mouse no botão da central muda o volume.
        AUDIO.with(|a| *a.borrow_mut() = Audio::new(hwnd));
        USAGE.with(|u| *u.borrow_mut() = Some(status::Usage::new()));
        update_clock();
        SetTimer(Some(hwnd), TIMER_STATS, STATS_MS, None);
        crate::bluetooth::watch(hwnd);
        // Eventos de janela (foco, minimizar, mover, maximizar) relêem a cor do app.
        let _ = SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND, None, Some(on_window_event), 0, 0, WINEVENT_OUTOFCONTEXT);
        let _ = SetWinEventHook(EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, None, Some(on_window_event), 0, 0, WINEVENT_OUTOFCONTEXT);
        let _ = SetWinEventHook(EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE, None, Some(on_window_event), 0, 0, WINEVENT_OUTOFCONTEXT);
    }
}

/// Tira as barras do sistema de AppBars (chamado onde o dock encerra).
pub fn shutdown() {
    let bars = BARS.with(|b| b.try_borrow_mut().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default());
    for bar in bars {
        let mut abd = bar.appbar_data();
        unsafe {
            SHAppBarMessage(ABM_REMOVE, &mut abd);
            let _ = ShowWindow(bar.hwnd, SW_HIDE);
            let _ = ShowWindow(bar.glass, SW_HIDE);
        }
    }
}

/// Algo mudou numa janela de topo: troca de app pinta na hora; o resto agenda uma releitura
/// em cada barra.
unsafe extern "system" fn on_window_event(
    _: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    obj: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    if hwnd.is_invalid() || obj != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 {
        return;
    }
    let ours = BARS.with(|b| b.try_borrow().ok().map(|g| g.iter().any(|b| hwnd == b.hwnd || hwnd == b.glass)));
    if ours != Some(false) {
        return;
    }
    // Só janelas de topo: as internas dos apps (vídeo, progresso...) geram eventos sem parar.
    if unsafe { GetAncestor(hwnd, GA_ROOT) } != hwnd {
        return;
    }
    for_bars(|b| {
        if event == EVENT_SYSTEM_FOREGROUND {
            // Troca de app: pinta já (com a cor do banco não precisa ler a tela).
            b.update_tint();
        }
        // Releitura logo depois, para pegar o fim de animações (restaurar, maximizar).
        if !b.tint_queued {
            b.tint_queued = true;
            unsafe { SetTimer(Some(b.hwnd), TIMER_TINT_EVENT, TINT_THROTTLE_MS, None) };
        }
    });
}
