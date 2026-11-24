#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod animcfg;
mod apps;
mod attention;
mod colordb;
mod config;
mod control;
mod drag;
mod bluetooth;
mod brightness;
mod calendar;
mod usage;
mod glass;
mod guard;
mod i18n;
mod launcher;
mod media;
mod minimize;
mod panel;
mod perf;
mod power;
mod preview;
mod render;
mod settings;
mod stacks;
mod status;
mod taskbar;
mod tint;
mod topbar;
mod tray;
mod update;
mod overflow;
mod volume;
mod wifi;
mod winkey;

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use tiny_skia::Pixmap;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, EnumDisplayMonitors, GetMonitorInfoW,
    HDC, HGDIOBJ, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST, MonitorFromWindow,
    ScreenToClient, SelectObject,
};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmFlush, DwmGetWindowAttribute};
use windows::core::BOOL;
use windows::Win32::Foundation::{FALSE, TRUE};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_DWORD, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, GetDpiForSystem, MDT_EFFECTIVE_DPI,
    SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::core::{HSTRING, PCWSTR, w};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use apps::App;
use render::{Metrics, Theme};

const TIMER_REFRESH: usize = 2;
const TIMER_EDGE: usize = 3;
const TIMER_GLASS: usize = 4;
const TIMER_MONITORS: usize = 5;
/// Quanto tempo o dock continua visível depois que o motivo para ficar visível acabou.
const HIDE_DELAY: Duration = Duration::from_millis(500);
/// Tempo de subida e descida do dock ao aparecer/sumir (segundos).
const SLIDE_TIME: f32 = 0.18;
/// Tempo de entrada/saída do zoom (segundos).
const HOVER_TIME: f32 = 0.14;
const MENU_QUIT: usize = 1;
const MENU_PIN: usize = 2;
const MENU_CLOSE: usize = 3;
const MENU_SETTINGS: usize = 4;
const MENU_NEW: usize = 5;
const MENU_ADMIN: usize = 6;
const MENU_LOCATION: usize = 7;
const MENU_MORE: usize = 8;
const MENU_UPDATE: usize = 9;
/// Itens das janelas do app: MENU_WINDOW + índice.
const MENU_WINDOW: usize = 100;
const WM_MOUSELEAVE: u32 = 0x02A3;
/// Abre/fecha o lançador. Mandada de fora (atalho do AutoHotkey) para a janela do dock
/// (classe `ArcDockWin`): `PostMessage 0x8032`.
pub const WM_LAUNCHER: u32 = WM_APP + 50;
/// Aviso do shell: a Lixeira mudou (stacks::watch_trash).
const WM_TRASH: u32 = WM_APP + 51;

thread_local! {
    static DOCKS: RefCell<Vec<Dock>> = const { RefCell::new(Vec::new()) };
}

struct Dock {
    hwnd: HWND,
    /// Janela atrás dos ícones que só fornece o desfoque (vidro).
    glass: HWND,
    monitor: HMONITOR,
    monitor_bounds: RECT,
    primary: bool,
    scale: f32,
    /// Último retângulo aplicado ao vidro (com o deslocamento vertical), para não repetir.
    glass_rect: Option<(i32, i32, i32, i32)>,
    glass_fx: Option<glass::Glass>,
    m: Metrics,
    theme: Theme,
    icon_size: i32,
    /// Data do arquivo de ícones trocados, para recarregar quando ele mudar.
    icons_mtime: Option<std::time::SystemTime>,
    apps: Vec<App>,
    /// Downloads e Lixeira, na ponta direita depois de um separador (stacks.rs).
    stacks: stacks::Stacks,
    pixmap: Pixmap,
    memdc: HDC,
    dib: HGDIOBJ,
    bits: *mut u8,
    pos: POINT,
    /// Posição x do mouse usada no zoom.
    mx: f32,
    /// Alvo real do cursor; a posição usada no zoom acompanha este alvo com suavização.
    mx_target: f32,
    mouse_step: Instant,
    /// Intensidade do zoom (0..1), animada de `hover_from` até `target`.
    hover: f32,
    target: f32,
    hover_from: f32,
    hover_start: Instant,
    animating: bool,
    tracking: bool,
    /// 0 = visível, 1 = escondido abaixo da tela.
    slide: f32,
    slide_target: f32,
    slide_from: f32,
    slide_start: Instant,
    /// Janelas do dock mostradas (escondidas de vez quando slide chega a 1).
    shown: bool,
    /// Última vez que havia motivo para o dock ficar visível.
    last_inside: Option<Instant>,
    /// Último resultado de `covered` e o retângulo testado; vale até um evento de janela
    /// ou o retângulo mudar.
    cover: Option<(RECT, bool)>,
    cover_dirty: bool,
    apps_dirty: bool,
    trash_dirty: bool,
    /// Tiques do TIMER_REFRESH (varredura completa a cada 5).
    ticks: u32,
    /// Arrastar ícones para reordenar/fixar/desafixar (drag.rs).
    drag: drag::Drag,
}

impl Dock {
    unsafe fn new(hwnd: HWND, glass: HWND, monitor: HMONITOR, bounds: RECT, primary: bool) -> Self {
        unsafe {
            let scale = monitor_scale(monitor);
            let m = Metrics::new(scale);
            let w = bounds.right - bounds.left;
            let h = m.window_height();
            let (dib, bits) = dib(w, h);
            let memdc = CreateCompatibleDC(None);
            SelectObject(memdc, dib);

            let icon_size = m.max.ceil() as i32;
            Self {
                hwnd,
                glass,
                monitor,
                monitor_bounds: bounds,
                primary,
                scale,
                glass_rect: None,
                glass_fx: glass::Glass::new(glass, m.radius).ok(),
                apps: apps::load_pinned(icon_size),
                stacks: stacks::Stacks::new(icon_size),
                icon_size,
                icons_mtime: icons_mtime(),
                m,
                theme: Theme::new(light_theme(), accent_color()),
                pixmap: Pixmap::new(w as u32, h as u32).expect("pixmap"),
                memdc,
                dib,
                bits,
                pos: POINT { x: bounds.left, y: bounds.bottom - h },
                mx: 0.0,
                mx_target: 0.0,
                mouse_step: Instant::now(),
                hover: 0.0,
                target: 0.0,
                hover_from: 0.0,
                hover_start: Instant::now(),
                animating: false,
                tracking: false,
                slide: 0.0,
                slide_target: 0.0,
                slide_from: 0.0,
                slide_start: Instant::now(),
                shown: true,
                last_inside: None,
                cover: None,
                cover_dirty: true,
                apps_dirty: true,
                trash_dirty: true,
                ticks: 0,
                drag: drag::Drag::default(),
            }
        }
    }

    /// Índice do último fixado, se houver apps abertos não fixados depois dele
    /// (o botão Iniciar, na ponta direita, não conta em nenhum dos dois grupos).
    fn sep_after(&self) -> Option<usize> {
        let regular = |a: &&App| a.source != apps::START;
        let pinned = self.apps.iter().filter(regular).filter(|a| a.pinned).count();
        let open = self.apps.iter().filter(regular).filter(|a| !a.pinned).count();
        (pinned > 0 && open > 0).then(|| pinned - 1)
    }

    /// Separadores: fixados | abertos e apps | pilhas.
    fn seps(&self) -> Vec<usize> {
        let stacks = self.apps.len().checked_sub(1).filter(|_| !self.stack_items().is_empty());
        self.sep_after().into_iter().chain(stacks).collect()
    }

    /// Quantos itens o dock desenha (apps e pilhas).
    fn count(&self) -> usize {
        self.apps.len() + self.stack_items().len()
    }

    /// Pilhas da ponta direita (nenhuma se desligadas nas Configurações).
    fn stack_items(&self) -> &[App] {
        if config::stacks() { &self.stacks.items } else { &[] }
    }

    /// Opções do dock mudaram (Configurações): refaz métricas, superfície e layout.
    /// `reload`: recarrega os ícones no tamanho novo (no fim do arrasto do controle).
    fn apply_settings(&mut self, reload: bool) {
        self.scale = monitor_scale(self.monitor);
        self.m = Metrics::new(self.scale);
        let w = self.monitor_bounds.right - self.monitor_bounds.left;
        let h = self.m.window_height();
        if h as u32 != self.pixmap.height() {
            let (dib, bits) = dib(w, h);
            unsafe {
                let old = SelectObject(self.memdc, dib);
                let _ = DeleteObject(old);
            }
            self.dib = dib;
            self.bits = bits;
            self.pixmap = Pixmap::new(w as u32, h as u32).expect("pixmap");
            self.pos.y = self.monitor_bounds.bottom - h;
        }
        let size = self.m.max.ceil() as i32;
        if reload && size != self.icon_size {
            self.icon_size = size;
            apps::reload_icons(&mut self.apps, size);
            self.stacks = stacks::Stacks::new(size);
        }
        self.glass_rect = None;
        self.edge_tick();
        self.redraw();
    }

    fn slots(&self) -> Vec<render::Slot> {
        let w = self.pixmap.width() as f32;
        render::layout(self.count(), &self.seps(), w, &self.m, self.mx, self.hover)
    }

    /// Pilha (source) sob o índice `i` e o ponto acima do ícone, se `i` for uma pilha.
    fn stack_at(&self, i: usize) -> Option<(String, POINT)> {
        let s = self.stack_items().get(i.checked_sub(self.apps.len())?)?;
        Some((s.source.clone(), self.menu_anchor(i)?))
    }

    fn refresh(&mut self) -> bool {
        self.refresh_some(true, true)
    }

    /// Refresh do timer de 1 s: apps só depois de um evento de janela, Lixeira só depois de
    /// um aviso do shell; a cada 5 s tudo, por segurança.
    fn refresh_tick(&mut self) -> bool {
        self.ticks = self.ticks.wrapping_add(1);
        let sweep = self.ticks % 5 == 0;
        let apps = std::mem::replace(&mut self.apps_dirty, false) || sweep;
        let trash = std::mem::replace(&mut self.trash_dirty, false) || sweep;
        self.refresh_some(apps, trash)
    }

    fn refresh_some(&mut self, apps: bool, trash: bool) -> bool {
        // Durante o arrasto a lista é do arrasto (drag.rs).
        if self.drag.busy() {
            return false;
        }
        let mtime = icons_mtime();
        let icons_changed = mtime != self.icons_mtime;
        if icons_changed {
            self.icons_mtime = mtime;
            apps::reload_icons(&mut self.apps, self.icon_size);
        }
        let stacks = self.stacks.refresh(icons_changed, trash);
        (apps && apps::refresh(&mut self.apps, self.icon_size)) || icons_changed || stacks
    }

    /// Deslocamento vertical atual da animação de esconder.
    fn offset(&self) -> i32 {
        (self.slide * self.pixmap.height() as f32).round() as i32
    }

    fn redraw(&mut self) {
        if !self.shown {
            return;
        }
        let slots = self.shown_slots();
        let seps = self.seps();
        // App em foco: dono da última janela em primeiro plano fora do dock (o menu do dock
        // toma o foco, mas o destaque continua no app).
        let fg = unsafe { GetForegroundWindow() };
        if fg != self.hwnd && fg != self.glass {
            LAST_FG.store(fg.0 as isize, Ordering::Relaxed);
        }
        let fg = HWND(LAST_FG.load(Ordering::Relaxed) as *mut c_void);
        let focus = self.apps.iter().position(|a| a.windows.contains(&fg));
        let stack: &[App] = if config::stacks() { &self.stacks.items } else { &[] };
        let items: Vec<&App> = self.apps.iter().chain(stack).collect();
        render::draw(&mut self.pixmap, &slots, &seps, &items, &self.m, &self.theme, self.hover, focus, self.animating);
        self.update_glass(render::box_rect(&slots, &self.m, self.pixmap.height() as f32));

        // RGBA (tiny-skia) -> BGRA (GDI), ambos com alfa pré-multiplicado.
        let src = self.pixmap.data();
        let dst = unsafe { std::slice::from_raw_parts_mut(self.bits, src.len()) };
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            d.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
        }

        let size = SIZE { cx: self.pixmap.width() as i32, cy: self.pixmap.height() as i32 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let pos = POINT { x: self.pos.x, y: self.pos.y + self.offset() };
        unsafe {
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

    /// O DWM nem sempre re-amostra o fundo do vidro quando um app minimiza/fecha por trás
    /// (fica com a cor do app). Esconder o vidro por um quadro e mostrar de novo força a releitura.
    fn nudge_glass(&mut self) {
        if self.glass_rect.is_none() {
            return;
        }
        unsafe {
            let _ = ShowWindow(self.glass, SW_HIDE);
            let _ = DwmFlush();
        }
        if let Some(fx) = &self.glass_fx {
            fx.refresh();
        }
        self.glass_rect = None;
        self.redraw();
    }

    /// Move a janela de vidro para trás do fundo do dock.
    fn update_glass(&mut self, rect: Option<(i32, i32, i32, i32)>) {
        let off = self.offset();
        let key = rect.map(|(l, t, r, b)| (l, t + off, r, b + off));
        if key == self.glass_rect {
            return;
        }
        self.glass_rect = key;
        let Some((l, t, r, b)) = rect else {
            unsafe {
                let _ = ShowWindow(self.glass, SW_HIDE);
            }
            return;
        };
        let (w, h) = (r - l, b - t);
        if let Some(fx) = &self.glass_fx {
            fx.resize(w as f32, h as f32);
        }
        unsafe {
            let _ = SetWindowPos(
                self.glass,
                None,
                self.pos.x + l,
                self.pos.y + t + off,
                w,
                h,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW,
            );
        }
    }

    /// Liga o laço de animação (sincronizado com a tela em `main`).
    fn start_anim(&mut self) {
        self.animating = true;
    }

    /// Um quadro de animação, com passo proporcional ao tempo real (independe da taxa da tela).
    fn step(&mut self) {
        let now = Instant::now();

        // Suaviza os pequenos saltos dos eventos de mouse sem introduzir atraso acumulado.
        let dt = now.saturating_duration_since(self.mouse_step).as_secs_f32().min(0.05);
        self.mouse_step = now;
        let follow = 1.0 - (-dt / 0.045).exp();
        self.mx += (self.mx_target - self.mx) * follow;
        let mouse_settling = (self.mx_target - self.mx).abs() > 0.25;

        // Entrada/saída do zoom: duração fixa, recomeça do ponto atual se o alvo mudar no meio.
        let th = ((now - self.hover_start).as_secs_f32() / HOVER_TIME).min(1.0);
        self.hover = self.hover_from + (self.target - self.hover_from) * ease_out_quad(th);

        // Subir/descer: curva com duração fixa; recomeça do ponto atual se o alvo mudar no meio.
        let ease: fn(f32) -> f32 =
            if self.slide_target < self.slide_from { ease_out_cubic } else { ease_in_out_cubic };
        let t = ((now - self.slide_start).as_secs_f32() / SLIDE_TIME).min(1.0);
        self.slide = self.slide_from + (self.slide_target - self.slide_from) * ease(t);

        let settling = self.drag_step() || mouse_settling;
        if th >= 1.0 && t >= 1.0 && !attention::bouncing() && !settling {
            self.animating = false;
        }
        self.apply_shown();
        self.redraw();
    }

    /// Muda o alvo do zoom (0 ou 1) e começa a animação a partir do valor atual.
    fn set_hover(&mut self, target: f32) {
        if target != self.target {
            self.hover_from = self.hover;
            self.hover_start = Instant::now();
            self.target = target;
            self.start_anim();
        }
    }

    /// Esconde as janelas quando o dock termina de descer e mostra de novo ao subir.
    fn apply_shown(&mut self) {
        let want = self.slide < 1.0;
        if want == self.shown {
            return;
        }
        self.shown = want;
        self.glass_rect = None;
        unsafe {
            if want {
                if let Some(fx) = &self.glass_fx {
                    fx.refresh();
                }
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                // Escondido, o dock pode ficar abaixo de um app maximizado que veio para a frente
                // (o WS_EX_TOPMOST continua marcado, mas o DWM o compõe atrás): ao subir, volta
                // ao topo da faixa "sempre no topo". Vidro primeiro; o dock é dono-dependente dele.
                let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
                let _ = SetWindowPos(self.glass, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
                let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
            } else {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
                let _ = ShowWindow(self.glass, SW_HIDE);
            }
        }
    }

    /// Mouse encostado na borda de baixo da tela, em qualquer ponto da largura (como no macOS).
    fn at_edge(&self) -> bool {
        let mut pt = POINT::default();
        if unsafe { GetCursorPos(&mut pt) }.is_err() {
            return false;
        }
        let b = self.monitor_bounds;
        pt.y >= b.bottom - 2 && pt.y < b.bottom && pt.x >= b.left && pt.x < b.right
    }

    /// Cursor sobre o dock na tela: fundo, ícones ampliados ou a faixa até a borda de baixo.
    /// Não depende do WM_MOUSEMOVE: se ele se perde (o cursor para logo depois de entrar), o
    /// `tracking` fica falso e o dock descia com o mouse em cima dele.
    fn mouse_over(&self) -> bool {
        if !self.shown {
            return false;
        }
        let mut pt = POINT::default();
        if unsafe { GetCursorPos(&mut pt) }.is_err() {
            return false;
        }
        let Some((l, t, r, _)) = render::box_rect(&self.slots(), &self.m, self.pixmap.height() as f32) else {
            return false;
        };
        // Com zoom, os ícones passam do fundo: conta a altura toda da janela.
        let top = if self.hover > 0.01 { 0 } else { t };
        let y = self.pos.y + self.offset();
        pt.x >= self.pos.x + l
            && pt.x < self.pos.x + r
            && pt.y >= y + top
            && pt.y < self.monitor_bounds.bottom
    }

    /// Decide se o dock deve aparecer ou sumir (chamado a cada ~50 ms).
    fn edge_tick(&mut self) {
        let now = Instant::now();
        // Com a pilha da Downloads aberta o dock fica à vista (como no macOS).
        if !config::autohide()
            || self.tracking
            || self.drag.busy()
            || MENU_OPEN.load(Ordering::Relaxed)
            || minimize::busy()
            || panel::is_open("downloads")
            || preview::is_open()
            || self.at_edge()
            || self.mouse_over()
            || !self.covered()
        {
            self.last_inside = Some(now);
        }
        // Aparecer é imediato; esconder espera a carência.
        let near = self.last_inside.is_some_and(|t| now - t < HIDE_DELAY);
        let target = if near { 0.0 } else { 1.0 };
        if target != self.slide_target {
            self.slide_from = self.slide;
            self.slide_start = now;
            self.slide_target = target;
            if target == 1.0 {
                self.set_hover(0.0);
            }
            self.start_anim();
        }
    }

    /// Alguma janela encosta no fundo do dock em repouso (sem zoom), estendido até a base da tela?
    fn covered(&mut self) -> bool {
        let w = self.pixmap.width() as f32;
        let slots = render::layout(self.count(), &self.seps(), w, &self.m, self.mx, 0.0);
        let Some((l, t, r, _)) = render::box_rect(&slots, &self.m, self.pixmap.height() as f32) else {
            return false;
        };
        let rect = RECT {
            left: (self.pos.x + l).max(self.monitor_bounds.left),
            top: self.pos.y + t,
            right: (self.pos.x + r).min(self.monitor_bounds.right),
            bottom: self.monitor_bounds.bottom,
        };
        // Percorrer as janelas custa ~75 µs; sem evento de janela desde a última vez, o
        // resultado não mudou.
        if !self.cover_dirty
            && let Some((last, hit)) = self.cover
            && last == rect
        {
            return hit;
        }
        let mut s = CoverSearch { rect, own: [self.hwnd, self.glass], hit: false };
        unsafe {
            let _ = EnumWindows(Some(cover_proc), LPARAM(&mut s as *mut _ as isize));
        }
        self.cover = Some((rect, s.hit));
        self.cover_dirty = false;
        s.hit
    }

    /// Ponto do menu de contexto do ícone `i`: centralizado nele, um espaço acima do topo
    /// (já com o zoom atual, que fica parado enquanto o menu está aberto).
    fn menu_anchor(&self, i: usize) -> Option<POINT> {
        let slots = self.slots();
        let slot = slots.get(i)?;
        let (_, _, _, b) = render::box_rect(&slots, &self.m, self.pixmap.height() as f32)?;
        let top = b as f32 - self.m.pad - slot.size;
        Some(POINT {
            x: self.pos.x + (slot.x + slot.size / 2.0).round() as i32,
            y: self.pos.y + self.offset() + (top - self.m.gap).round() as i32,
        })
    }

    fn hit(&self, x: f32) -> Option<usize> {
        self.slots()
            .iter()
            .position(|s| x >= s.x - self.m.gap / 2.0 && x < s.x + s.size + self.m.gap / 2.0)
    }
}

impl Drop for Dock {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.memdc);
            let _ = DeleteObject(self.dib);
        }
    }
}

fn ease_out_quad(t: f32) -> f32 {
    1.0 - (1.0 - t) * (1.0 - t)
}

fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn ease_in_out_cubic(t: f32) -> f32 {
    if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 }
}

/// Estado da busca de janelas que encostam no dock (passado ao `EnumWindows`).
struct CoverSearch {
    rect: RECT,
    own: [HWND; 2],
    hit: bool,
}

/// Marca `hit` e para a busca na primeira janela visível que encosta no retângulo.
unsafe extern "system" fn cover_proc(hwnd: HWND, lp: LPARAM) -> BOOL {
    unsafe {
        let s = &mut *(lp.0 as *mut CoverSearch);
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() || s.own.contains(&hwnd) {
            return TRUE;
        }
        if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
            return TRUE;
        }
        // Retângulo real, sem as bordas invisíveis de redimensionar.
        let mut r = RECT::default();
        if DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut _ as *mut c_void, size_of::<RECT>() as u32)
            .is_err()
        {
            return TRUE;
        }
        let d = s.rect;
        if r.right <= r.left || r.bottom <= r.top
            || r.right <= d.left || r.left >= d.right || r.bottom <= d.top || r.top >= d.bottom
        {
            return TRUE;
        }
        // Escondida pelo DWM: outra área de trabalho virtual ou app UWP suspenso.
        let mut cloaked: u32 = 0;
        let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut c_void, size_of::<u32>() as u32);
        if cloaked != 0 {
            return TRUE;
        }
        let mut buf = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut buf).max(0) as usize;
        let class = String::from_utf16_lossy(&buf[..n]);
        // Área de trabalho e barra do Windows.
        if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
            return TRUE;
        }
        s.hit = true;
        FALSE
    }
}

/// Bitmap 32 bits (BGRA) de `w` × `h` para a janela em camadas e os bytes dele.
fn dib(w: i32, h: i32) -> (HGDIOBJ, *mut u8) {
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
    let dib = unsafe { CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) }.expect("CreateDIBSection");
    (HGDIOBJ(dib.0), bits as *mut u8)
}

/// Aplica as opções do dock mudadas nas Configurações (ver `Dock::apply_settings`).
pub fn apply_settings(reload: bool) {
    with_docks(|d| d.apply_settings(reload));
}

/// Recria as barras quando a topologia de monitores ou a opção de exibição muda.
pub fn rebuild_docks() {
    let mut monitors = display_monitors();
    if !config::dock_all_monitors() {
        monitors.retain(|(_, _, primary)| *primary);
    }
    if monitors.is_empty() {
        return;
    }

    let unchanged = DOCKS.with(|docks| {
        let Ok(docks) = docks.try_borrow() else { return false };
        docks.len() == monitors.len()
            && docks.iter().zip(&monitors).all(|(dock, (monitor, bounds, primary))| {
                dock.monitor == *monitor
                    && dock.monitor_bounds == *bounds
                    && dock.primary == *primary
                    && (dock.scale - monitor_scale(*monitor)).abs() < 0.001
            })
    });
    if unchanged {
        return;
    }

    let mut new_docks = Vec::new();
    for (monitor, bounds, primary) in monitors {
        if let Some(dock) = unsafe { create_dock(monitor, bounds, primary) } {
            new_docks.push(dock);
        }
    }
    if new_docks.is_empty() {
        return;
    }

    let old_docks = DOCKS.with(|docks| {
        docks
            .try_borrow_mut()
            .ok()
            .map(|mut docks| std::mem::take(&mut *docks))
    });
    let Some(old_docks) = old_docks else {
        REBUILDING_DOCKS.store(true, Ordering::Release);
        for dock in new_docks {
            dispose_dock(dock);
        }
        REBUILDING_DOCKS.store(false, Ordering::Release);
        return;
    };
    REBUILDING_DOCKS.store(true, Ordering::Release);
    for dock in old_docks {
        dispose_dock(dock);
    }
    DOCKS.with(|docks| *docks.borrow_mut() = new_docks);
    REBUILDING_DOCKS.store(false, Ordering::Release);
    with_docks(|dock| {
        if dock.primary {
            attention::register(dock.hwnd);
            stacks::watch_trash(dock.hwnd, WM_TRASH);
            update::set_window(dock.hwnd);
        }
    });
    taskbar::hide();
}

fn icons_mtime() -> Option<std::time::SystemTime> {
    std::fs::metadata(apps::icons_path()).and_then(|m| m.modified()).ok()
}

/// Destino da animação de minimizar (minimize.rs): quadrado do ícone do app dono de `win`
/// na tela, com o dock em repouso. Faz o dock aparecer se estiver escondido.
fn minimize_target(win: HWND) -> Option<RECT> {
    let monitor = unsafe { MonitorFromWindow(win, MONITOR_DEFAULTTONEAREST) };
    with_dock_on_monitor(monitor, |d| {
        d.edge_tick();
        if !d.apps.iter().any(|a| a.windows.contains(&win)) && d.refresh() {
            d.redraw();
        }
        let slots = d.slots();
        let (l, _, r, b) = render::box_rect(&slots, &d.m, d.pixmap.height() as f32)?;
        // Janela sem ícone no dock: vai para o meio dele.
        let (x, size) = match d.apps.iter().position(|a| a.windows.contains(&win)).and_then(|i| slots.get(i)) {
            Some(s) => (s.x, s.size),
            None => ((l + r) as f32 / 2.0 - d.m.base / 2.0, d.m.base),
        };
        let bottom = d.pos.y + (b as f32 - d.m.pad).round() as i32;
        let (x, size) = (d.pos.x + x.round() as i32, size.round() as i32);
        Some(RECT { left: x, top: bottom - size, right: x + size, bottom })
    })
    .flatten()
}

fn with_dock<R>(hwnd: HWND, f: impl FnOnce(&mut Dock) -> R) -> Option<R> {
    // try_borrow_mut: chamadas do Windows podem reentrar no wndproc.
    DOCKS.with(|docks| {
        let mut docks = docks.try_borrow_mut().ok()?;
        docks.iter_mut().find(|dock| dock.hwnd == hwnd).map(f)
    })
}

fn with_docks(mut f: impl FnMut(&mut Dock)) {
    DOCKS.with(|docks| {
        if let Ok(mut docks) = docks.try_borrow_mut() {
            for dock in docks.iter_mut() {
                f(dock);
            }
        }
    });
}

fn reload_pins() {
    with_docks(|dock| {
        dock.apps = apps::load_pinned(dock.icon_size);
        let _ = apps::refresh(&mut dock.apps, dock.icon_size);
        dock.redraw();
    });
}

fn with_dock_on_monitor<R>(monitor: HMONITOR, f: impl FnOnce(&mut Dock) -> R) -> Option<R> {
    DOCKS.with(|docks| {
        let mut docks = docks.try_borrow_mut().ok()?;
        let index = docks
            .iter()
            .position(|dock| dock.monitor == monitor)
            .or_else(|| docks.iter().position(|dock| dock.primary))
            .or_else(|| (!docks.is_empty()).then_some(0))?;
        docks.get_mut(index).map(f)
    })
}

fn docks_animating() -> bool {
    DOCKS.with_borrow(|docks| docks.iter().any(|dock| dock.animating))
}

fn step_docks() {
    with_docks(|dock| {
        if dock.animating {
            dock.step();
        }
    });
}

fn display_monitors() -> Vec<(HMONITOR, RECT, bool)> {
    unsafe extern "system" fn add(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let list = unsafe { &mut *(data.0 as *mut Vec<(HMONITOR, RECT, bool)>) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            list.push((monitor, info.rcMonitor, info.dwFlags & MONITORINFOF_PRIMARY != 0));
        }
        BOOL(1)
    }

    let mut monitors: Vec<(HMONITOR, RECT, bool)> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(add), LPARAM(&mut monitors as *mut _ as isize));
    }
    monitors.sort_by_key(|(_, _, primary)| !*primary);
    monitors
}

fn monitor_scale(monitor: HMONITOR) -> f32 {
    let (mut x, mut y) = (96u32, 96u32);
    if unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) }.is_err() {
        x = unsafe { GetDpiForSystem() };
    }
    x as f32 / 96.0
}

unsafe fn create_dock(monitor: HMONITOR, bounds: RECT, primary: bool) -> Option<Dock> {
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None).ok()?.into();
        let ex = WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE;
        let glass = CreateWindowExW(
            ex | WS_EX_NOREDIRECTIONBITMAP,
            w!("ArcDockWinGlass"),
            w!("ArcDockWinGlass"),
            WS_POPUP,
            bounds.left,
            bounds.bottom - 1,
            1,
            1,
            None,
            None,
            Some(instance),
            None,
        )
        .ok()?;
        let hwnd = match CreateWindowExW(
            ex | WS_EX_LAYERED,
            w!("ArcDockWin"),
            w!("ArcDockWin"),
            WS_POPUP,
            bounds.left,
            bounds.bottom - 1,
            1,
            1,
            Some(glass),
            None,
            Some(instance),
            None,
        ) {
            Ok(hwnd) => hwnd,
            Err(_) => {
                let _ = DestroyWindow(glass);
                return None;
            }
        };

        let mut dock = Dock::new(hwnd, glass, monitor, bounds, primary);
        dock.refresh();
        dock.redraw();
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = SetTimer(Some(hwnd), TIMER_REFRESH, 1000, None);
        let _ = SetTimer(Some(hwnd), TIMER_EDGE, 50, None);
        Some(dock)
    }
}

fn dispose_dock(mut dock: Dock) {
    // Composição antes da janela dela.
    dock.glass_fx = None;
    unsafe {
        let _ = KillTimer(Some(dock.hwnd), TIMER_EDGE);
        let _ = KillTimer(Some(dock.hwnd), TIMER_REFRESH);
        let _ = KillTimer(Some(dock.hwnd), TIMER_GLASS);
        let _ = KillTimer(Some(dock.hwnd), TIMER_MONITORS);
        let _ = DestroyWindow(dock.hwnd);
        let _ = DestroyWindow(dock.glass);
    }
    drop(dock);
}

fn mouse_x(lp: LPARAM) -> f32 {
    (lp.0 & 0xffff) as i16 as f32
}

fn mouse_y(lp: LPARAM) -> f32 {
    ((lp.0 >> 16) & 0xffff) as i16 as f32
}

/// Tema da barra de tarefas (claro/escuro).
/// Bandeja do sistema na topbar: os apps registram os ícones em nós (tray.rs) e a barra
/// mostra e repassa os cliques.
fn start_tray() {
    fn push() {
        // Ícone sem imagem vira um espaço vazio, para os índices baterem com os de tray.rs.
        let px = tray_px();
        let icons = tray::icons()
            .into_iter()
            .filter_map(|t| {
                let mut icon = t.icon.or_else(|| tiny_skia::Pixmap::new(px, px))?;
                if !config::tray_color() {
                    grayscale(&mut icon);
                }
                let shown = colordb::tray_shown(&t.key).unwrap_or(true);
                Some(topbar::TrayEntry { icon, tip: t.tip, key: t.key, shown, visible: shown })
            })
            .collect();
        topbar::set_tray(icons);
    }
    /// Tons de cinza (alfa pré-multiplicado: a média ponderada continua válida).
    fn grayscale(p: &mut tiny_skia::Pixmap) {
        for c in p.data_mut().chunks_exact_mut(4) {
            let y = (0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32).round() as u8;
            c[..3].fill(y);
        }
    }
    fn clicked(index: usize, button: topbar::TrayButton, anchor: POINT) {
        let b = match button {
            topbar::TrayButton::Left => tray::TrayButton::Left,
            topbar::TrayButton::Right => tray::TrayButton::Right,
            topbar::TrayButton::Double => tray::TrayButton::Double,
        };
        tray::click(index, b, anchor);
    }
    fn tray_px() -> u32 {
        (16.0 * unsafe { GetDpiForSystem() } as f32 / 96.0).round() as u32
    }
    topbar::on_tray_click(clicked);
    if tray::start(tray_px(), push).is_ok() {
        push();
    }
}

/// Inicia com o Windows (opção das Configurações): ligado, grava o caminho deste exe em
/// HKCU\...\Run (só se mudou); desligado, apaga o valor.
pub fn autostart() {
    let key = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
    if !config::autostart() {
        unsafe {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, key, w!("ArcDockWin"));
        }
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let value: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().chain([0]).collect();
    let mut buf = [0u16; 1024];
    let mut size = size_of_val(&buf) as u32;
    unsafe {
        let same = RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            w!("ArcDockWin"),
            windows::Win32::System::Registry::RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut c_void),
            Some(&mut size),
        )
        .is_ok()
            && buf[..(size as usize / 2)] == value[..];
        if !same {
            let _ = RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key,
                w!("ArcDockWin"),
                REG_SZ.0,
                Some(value.as_ptr() as *const c_void),
                (value.len() * 2) as u32,
            );
        }
    }
}

pub fn light_theme() -> bool {
    let mut value: u32 = 0;
    let mut size = size_of::<u32>() as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut _ as *mut c_void),
            Some(&mut size),
        )
    };
    ok.is_ok() && value != 0
}

/// Cor de destaque do Windows (no registro vem como ABGR).
fn accent_color() -> Option<[u8; 4]> {
    let mut value: u32 = 0;
    let mut size = size_of::<u32>() as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\DWM"),
            w!("AccentColor"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut _ as *mut c_void),
            Some(&mut size),
        )
    };
    ok.is_ok().then(|| [value as u8, (value >> 8) as u8, (value >> 16) as u8, 255])
}

/// Janela em primeiro plano mudou: atualiza o destaque do app em foco na hora.
unsafe extern "system" fn on_foreground(
    _: HWINEVENTHOOK,
    _: u32,
    hwnd: HWND,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    with_docks(|d| {
        d.cover_dirty = true;
        // O que está atrás do vidro mudou: re-amostra depois que a animação do Windows acaba.
        unsafe {
            SetTimer(Some(d.hwnd), TIMER_GLASS, 500, None);
        }
        if hwnd == d.hwnd || hwnd == d.glass {
            return;
        }
        // Janela ainda não listada (acabou de abrir): atualiza os apps antes.
        if !hwnd.is_invalid() && !d.apps.iter().any(|a| a.windows.contains(&hwnd)) {
            d.refresh();
        }
        d.redraw();
    });
}

/// Janela apareceu, sumiu, mexeu, mudou de título ou foi escondida pelo DWM: marca como velhos
/// `covered` e (menos no movimento) a lista de apps.
unsafe extern "system" fn on_window_change(
    _: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    obj: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    if !hwnd.is_invalid() && obj == OBJID_WINDOW.0 && child == CHILDID_SELF as i32 {
        with_docks(|dock| {
            dock.cover_dirty = true;
            if event != EVENT_OBJECT_LOCATIONCHANGE {
                dock.apps_dirty = true;
            }
        });
    }
}

/// Menu do botão direito: com `target` (key, source, fixado, janelas), as opções do app; sempre "Sair".
/// `anchor`: ponto acima do centro do ícone onde o menu se apoia (sem ícone, abre no mouse).
unsafe fn context_menu(hwnd: HWND, target: Option<(String, String, bool, Vec<HWND>)>, anchor: Option<POINT>) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let fg = GetForegroundWindow();
        if let Some((_, source, pinned, windows)) = &target {
            if windows.len() >= 2 {
                for (k, &win) in windows.iter().enumerate() {
                    let flags = if win == fg { MF_STRING | MF_CHECKED } else { MF_STRING };
                    let title = HSTRING::from(apps::menu_title(win));
                    let _ = AppendMenuW(menu, flags, MENU_WINDOW + k, &title);
                }
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            }
            if !windows.is_empty() {
                let text = HSTRING::from(crate::i18n::tr("Nova janela"));
                let _ = AppendMenuW(menu, MF_STRING, MENU_NEW, &text);
            }
            let text = HSTRING::from(crate::i18n::tr("Executar como administrador"));
            let _ = AppendMenuW(menu, MF_STRING, MENU_ADMIN, &text);
            if !source.starts_with(apps::APPS_FOLDER) {
                let text = HSTRING::from(crate::i18n::tr("Abrir local do arquivo"));
                let _ = AppendMenuW(menu, MF_STRING, MENU_LOCATION, &text);
            }
            let text = HSTRING::from(crate::i18n::tr("Mais opções…"));
            let _ = AppendMenuW(menu, MF_STRING, MENU_MORE, &text);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let pin = HSTRING::from(crate::i18n::tr(if *pinned { "Desafixar do dock" } else { "Fixar no dock" }));
            let _ = AppendMenuW(menu, MF_STRING, MENU_PIN, &pin);
            match windows.len() {
                0 => {}
                1 => {
                    let text = HSTRING::from(crate::i18n::tr("Fechar janela"));
                    let _ = AppendMenuW(menu, MF_STRING, MENU_CLOSE, &text);
                }
                n => {
                    let text = HSTRING::from(crate::i18n::tr(&format!("Fechar {n} janelas")));
                    let _ = AppendMenuW(menu, MF_STRING, MENU_CLOSE, &text);
                }
            }
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        }
        if matches!(update::status(), update::Status::Available(_)) {
            let text = HSTRING::from(crate::i18n::tr("Atualizar o ArcDockWin"));
            let _ = AppendMenuW(menu, MF_STRING, MENU_UPDATE, &text);
        }
        let settings = HSTRING::from(crate::i18n::tr("Configurações…"));
        let _ = AppendMenuW(menu, MF_STRING, MENU_SETTINGS, &settings);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let quit = HSTRING::from(crate::i18n::tr("Sair do ArcDockWin"));
        let _ = AppendMenuW(menu, MF_STRING, MENU_QUIT, &quit);

        let (pt, align) = match anchor {
            Some(p) => (p, TPM_CENTERALIGN),
            None => {
                let mut p = POINT::default();
                let _ = GetCursorPos(&mut p);
                (p, TPM_LEFTALIGN)
            }
        };
        let _ = SetForegroundWindow(hwnd);
        MENU_OPEN.store(true, Ordering::Relaxed);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | align,
            pt.x,
            pt.y,
            None,
            hwnd,
            None,
        );
        MENU_OPEN.store(false, Ordering::Relaxed);
        let _ = DestroyMenu(menu);

        let cmd = cmd.0 as usize;
        if cmd == MENU_QUIT {
            let _ = DestroyWindow(hwnd);
            return;
        }
        if cmd == MENU_SETTINGS {
            settings::open();
            return;
        }
        if cmd == MENU_UPDATE {
            update::install();
            return;
        }
        let Some((key, source, pinned, windows)) = target else { return };
        match cmd {
            MENU_NEW => {
                ShellExecuteW(None, w!("open"), &HSTRING::from(&source), None, None, SW_SHOWNORMAL);
                return;
            }
            MENU_ADMIN => {
                ShellExecuteW(None, w!("runas"), &HSTRING::from(&source), None, None, SW_SHOWNORMAL);
                return;
            }
            MENU_LOCATION => {
                let args = HSTRING::from(format!("/select,\"{source}\""));
                ShellExecuteW(None, w!("open"), w!("explorer.exe"), &args, None, SW_SHOWNORMAL);
                return;
            }
            MENU_MORE => {
                apps::shell_menu(hwnd, &source, pt, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | align);
                return;
            }
            MENU_PIN => {
                with_dock(hwnd, |d| apps::set_pinned(&mut d.apps, &key, !pinned));
            }
            MENU_CLOSE => {
                for &win in &windows {
                    let _ = PostMessageW(Some(win), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            c if c >= MENU_WINDOW => {
                if let Some(&win) = windows.get(c - MENU_WINDOW) {
                    apps::focus_window(win);
                }
            }
            _ => return,
        }
        if cmd == MENU_PIN {
            reload_pins();
        } else {
            with_docks(|d| {
                let _ = d.refresh();
                d.redraw();
            });
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        // Submenus do menu do Windows ("Mais opções…"), como "Abrir com", se preenchem sozinhos.
        if let Some(r) = apps::shell_menu_msg(msg, wp, lp) {
            return r;
        }
        match msg {
            WM_TRASH => {
                stacks::trash_notified(wp.0, lp.0);
                with_docks(|dock| dock.trash_dirty = true);
                LRESULT(0)
            }
            WM_LAUNCHER => {
                launcher::toggle();
                LRESULT(0)
            }
            update::WM_UPDATE => {
                if wp.0 == 1 {
                    update::relaunch(hwnd);
                } else {
                    settings::refresh();
                }
                LRESULT(0)
            }
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_MOUSEMOVE => {
                // App com 2+ janelas sob o mouse: a prévia delas abre depois de uma pequena espera.
                let preview_target = with_dock(hwnd, |d| {
                    if d.drag.busy() {
                        return None;
                    }
                    let i = d.hit(mouse_x(lp))?;
                    let app = d.apps.get(i).filter(|a| a.windows.len() >= 2)?;
                    Some(preview::Target { key: app.key.clone(), windows: app.windows.clone(), anchor: d.menu_anchor(i)? })
                })
                .flatten();
                preview::hover(preview_target);
                with_dock(hwnd, |d| {
                    d.mx_target = mouse_x(lp);
                    // O ícone arrastado deve continuar colado ao cursor.
                    if d.drag.busy() {
                        d.mx = d.mx_target;
                    } else if (d.mx_target - d.mx).abs() > 0.25 {
                        d.start_anim();
                    }
                    if !d.tracking {
                        d.tracking = true;
                        let mut tme = TRACKMOUSEEVENT {
                            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        };
                        let _ = TrackMouseEvent(&mut tme);
                    }
                    d.drag_move(d.mx_target, mouse_y(lp));
                    d.set_hover(1.0);
                    if !d.animating {
                        d.redraw();
                    }
                });
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                preview::dock_left();
                with_dock(hwnd, |d| {
                    d.tracking = false;
                    // Com o mouse capturado no arrasto, o zoom fica até soltar.
                    if d.drag.busy() {
                        return;
                    }
                    d.set_hover(0.0);
                });
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                // A wheel scroll over a grouped app cycles through that app's windows.
                let mut point = POINT {
                    x: (lp.0 & 0xffff) as i16 as i32,
                    y: ((lp.0 >> 16) & 0xffff) as i16 as i32,
                };
                let _ = ScreenToClient(hwnd, &mut point);
                preview::dismiss();
                let windows = with_dock(hwnd, |d| {
                    let i = d.hit(point.x as f32)?;
                    let app = d.apps.get(i)?;
                    (app.windows.len() > 1).then(|| app.windows.clone())
                })
                .flatten();
                if let Some(windows) = windows {
                    apps::cycle_window(&windows, (wp.0 >> 16) as i16 >= 0);
                    with_dock(hwnd, |d| d.redraw());
                }
                LRESULT(0)
            }
            WM_TIMER => {
                if wp.0 == TIMER_MONITORS {
                    let _ = KillTimer(Some(hwnd), TIMER_MONITORS);
                    rebuild_docks();
                    return LRESULT(0);
                }
                let language_changed = wp.0 == TIMER_REFRESH && i18n::refresh_language();
                if language_changed {
                    topbar::language_changed();
                }
                with_dock(hwnd, |d| match wp.0 {
                    TIMER_EDGE => d.edge_tick(),
                    TIMER_GLASS => {
                        let _ = KillTimer(Some(d.hwnd), TIMER_GLASS);
                        d.nudge_glass();
                    },
                    TIMER_REFRESH => {
                        if d.primary {
                            taskbar::ensure_hidden();
                        }
                        if d.refresh_tick() || language_changed {
                            d.redraw();
                        }
                    }
                    _ => {}
                });
                if language_changed {
                    with_docks(|dock| dock.redraw());
                }
                LRESULT(0)
            }
            WM_DISPLAYCHANGE | WM_DPICHANGED => {
                let _ = SetTimer(Some(hwnd), TIMER_MONITORS, 250, None);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                with_dock(hwnd, |d| d.drag_down(mouse_x(lp), mouse_y(lp)));
                LRESULT(0)
            }
            WM_CAPTURECHANGED => {
                with_dock(hwnd, |d| d.drag_up());
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                preview::dismiss();
                // Foi um arrasto, não um clique.
                if with_dock(hwnd, |d| d.drag_up()).unwrap_or(false) {
                    reload_pins();
                    return LRESULT(0);
                }
                // Pilhas da ponta (Downloads, Lixeira).
                if let Some((source, anchor)) = with_dock(hwnd, |d| d.stack_at(d.hit(mouse_x(lp))?)).flatten() {
                    stacks::click(&source, anchor);
                    return LRESULT(0);
                }
                let target = with_dock(hwnd, |d| {
                    if d.refresh() {
                        d.redraw();
                    }
                    // get: depois do refresh o índice pode cair numa pilha.
                    d.hit(mouse_x(lp)).and_then(|i| d.apps.get(i)).map(|a| (a.source.clone(), a.windows.clone()))
                })
                .flatten();
                if let Some((source, windows)) = target {
                    apps::activate(&source, &windows);
                }
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                preview::dismiss();
                if let Some((source, anchor)) = with_dock(hwnd, |d| d.stack_at(d.hit(mouse_x(lp))?)).flatten() {
                    MENU_OPEN.store(true, Ordering::Relaxed);
                    stacks::context_menu(hwnd, &source, anchor);
                    MENU_OPEN.store(false, Ordering::Relaxed);
                    with_dock(hwnd, |d| if d.refresh() { d.redraw() });
                    return LRESULT(0);
                }
                // App sob o mouse; fora de um ícone ou no botão Iniciar, só "Sair".
                let (target, anchor) = with_dock(hwnd, |d| {
                    if d.refresh() {
                        d.redraw();
                    }
                    let i = d.hit(mouse_x(lp));
                    let target = i
                        .and_then(|i| d.apps.get(i))
                        .filter(|a| a.source != apps::START)
                        .map(|a| (a.key.clone(), a.source.clone(), a.pinned, a.windows.clone()));
                    (target, i.and_then(|i| d.menu_anchor(i)))
                })
                .unwrap_or((None, None));
                context_menu(hwnd, target, anchor);
                LRESULT(0)
            }
            WM_SETTINGCHANGE => {
                // Tema claro/escuro ou cor de destaque mudou: refaz as cores do dock.
                with_dock(hwnd, |d| {
                    d.theme = Theme::new(light_theme(), accent_color());
                    d.redraw();
                });
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_ENDSESSION => {
                if wp.0 != 0 && !SHUTTING_DOWN.swap(true, Ordering::AcqRel) {
                    tray::stop();
                    topbar::shutdown();
                    taskbar::restore();
                    preview::shutdown();
                    minimize::shutdown();
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                if !REBUILDING_DOCKS.load(Ordering::Acquire)
                    && !SHUTTING_DOWN.swap(true, Ordering::AcqRel)
                {
                    tray::stop();
                    topbar::shutdown();
                    taskbar::restore();
                    preview::shutdown();
                    minimize::shutdown();
                    PostQuitMessage(0);
                }
                LRESULT(0)
            }
            _ if attention::is_hook(msg) => {
                if attention::handle(wp, lp) {
                    with_docks(|d| {
                        if attention::bouncing() {
                            d.start_anim();
                        }
                        d.redraw();
                    });
                }
                LRESULT(0)
            }
            _ if msg == TASKBAR_CREATED.load(Ordering::Relaxed) && msg != 0 => {
                // O Explorer reiniciou e recriou a barra: esconde de novo e devolve à barra de
                // cima a reserva do topo da tela, que ele esqueceu (uma vez, pelo dock principal).
                if with_dock(hwnd, |dock| dock.primary).unwrap_or(false) {
                    taskbar::hide();
                    topbar::explorer_restarted();
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static REBUILDING_DOCKS: AtomicBool = AtomicBool::new(false);
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// Menu do botão direito aberto: o dock não some enquanto ele estiver na tela.
static MENU_OPEN: AtomicBool = AtomicBool::new(false);

/// Última janela em primeiro plano fora do dock (HWND como número).
static LAST_FG: AtomicIsize = AtomicIsize::new(0);

unsafe fn register(hinst: HINSTANCE, name: PCWSTR, proc: WNDPROC) -> windows::core::Result<()> {
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: proc,
            hInstance: hinst,
            lpszClassName: name,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        RegisterClassW(&wc);
        Ok(())
    }
}

unsafe extern "system" fn glass_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

use windows::Win32::Foundation::HINSTANCE;

fn main() -> windows::core::Result<()> {
    if guard::run_if_requested() {
        return Ok(());
    }
    i18n::initialize();
    update::after_update();
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        // Uma cópia só.
        let _single = windows::Win32::System::Threading::CreateMutexW(None, true, w!("ArcDockWin.Single"))?;
        if windows::Win32::Foundation::GetLastError() == windows::Win32::Foundation::ERROR_ALREADY_EXISTS {
            return Ok(());
        }

        let hinst: HINSTANCE = GetModuleHandleW(None)?.into();
        register(hinst, w!("ArcDockWinGlass"), Some(glass_proc))?;
        register(hinst, w!("ArcDockWin"), Some(wndproc))?;

        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed);
        guard::spawn();
        taskbar::hide();
        let mut monitors = display_monitors();
        if !config::dock_all_monitors() {
            monitors.retain(|(_, _, primary)| *primary);
        }
        let docks: Vec<Dock> = monitors
            .into_iter()
            .filter_map(|(monitor, bounds, primary)| create_dock(monitor, bounds, primary))
            .collect();
        if docks.is_empty() {
            return Ok(());
        }
        let main_hwnd = docks
            .iter()
            .find(|dock| dock.primary)
            .map(|dock| dock.hwnd)
            .unwrap_or(docks[0].hwnd);
        DOCKS.with(|all| *all.borrow_mut() = docks);
        attention::register(main_hwnd);
        stacks::watch_trash(main_hwnd, WM_TRASH);

        topbar::create();
        minimize::init();
        winkey::config_changed();
        launcher::preload();
        start_tray();
        autostart();
        update::start(main_hwnd);
        let _ = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        // Minimizar/restaurar também muda o que está atrás do vidro.
        let _ = SetWinEventHook(
            EVENT_SYSTEM_MINIMIZESTART,
            EVENT_SYSTEM_MINIMIZEEND,
            None,
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );

        // O que encosta no dock (auto-esconder) e a lista de apps só mudam com estes eventos.
        for (from, to) in [
            (EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE),
            (EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_NAMECHANGE),
            (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
        ] {
            let _ = SetWinEventHook(from, to, None, Some(on_window_change), 0, 0, WINEVENT_OUTOFCONTEXT);
        }

        // Parado: espera mensagens. Animando: um quadro por atualização da tela (DwmFlush
        // espera o próximo quadro do compositor), na taxa do monitor (60, 120, 144 Hz...).
        let mut msg = MSG::default();
        loop {
            if docks_animating() || minimize::busy() {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        return Ok(());
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                step_docks();
                minimize::frame();
                let _ = DwmFlush();
            } else {
                if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
    Ok(())
}
