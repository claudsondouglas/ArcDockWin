//! Painéis suspensos que abrem sob os itens da topbar (calendário, volume, Wi‑Fi...).
//!
//! # Como montar um painel
//!
//! Um painel é um tipo que implementa [`PanelContent`]. Ele não desenha nada: a cada
//! [`PanelContent::build`] devolve a lista de controles ([`Widget`]) de cima para baixo, e
//! o painel faz o layout, o desenho, o hover e o teclado. Cliques, interruptores, controle
//! deslizante e campo de texto voltam como [`Event`] em [`PanelContent::event`], com o `id`
//! que o conteúdo deu ao controle. Se a resposta for `true`, o painel chama `build` de novo
//! e redesenha (é assim que o estado novo aparece).
//!
//! ```ignore
//! struct Exemplo { ligado: bool }
//!
//! impl PanelContent for Exemplo {
//!     fn build(&mut self) -> Vec<Widget> {
//!         vec![
//!             Widget::Title("Exemplo".into()),
//!             Widget::Toggle { id: 1, text: "Ligado".into(), on: self.ligado, icon: None },
//!             Widget::Separator,
//!             Widget::Button { id: 2, text: "Fechar".into(), primary: false },
//!         ]
//!     }
//!     fn event(&mut self, ev: Event) -> bool {
//!         match ev {
//!             Event::Toggle(1, on) => { self.ligado = on; true }
//!             Event::Click(2) => { panel::close(); false }
//!             _ => false,
//!         }
//!     }
//! }
//!
//! // Na topbar, no clique do item (retângulo do item em coordenadas de tela):
//! panel::toggle("exemplo", item_rect, || Box::new(Exemplo { ligado: false }));
//! ```
//!
//! # Funções do módulo
//! - [`toggle`]: abre o painel `key` sob o item; se ele já está aberto, fecha (segundo clique
//!   no mesmo item). Se outro painel está aberto, troca. Só existe um painel por vez.
//! - [`close`]: fecha o painel aberto (pode ser chamado de dentro de `event`).
//! - [`refresh`]: chama `build` de novo e redesenha (use quando o estado mudou por fora,
//!   ex.: a varredura de Wi‑Fi terminou). Dentro de `event`, devolva `true` em vez disso.
//! - [`notify`]: repassa uma mensagem da janela da topbar (ex.: `status::WM_AUDIO`) para
//!   [`PanelContent::notify`]; se ele devolver `true`, reconstrói.
//! - [`is_open`]: se o painel `key` está aberto.
//!
//! # Regras
//! - Tudo roda na thread da interface (a do dock). Trabalho demorado (varredura, conexão)
//!   deve ir para outra thread, que avisa a janela da topbar com `PostMessageW`; a topbar
//!   então chama [`notify`] ou [`refresh`].
//! - O campo de texto ([`Widget::Field`]) guarda o próprio texto, identificado pelo `id`,
//!   enquanto o campo continuar na lista. Cada edição gera [`Event::Text`] e o Enter gera
//!   [`Event::Submit`]. O primeiro campo que aparece ganha o foco do teclado sozinho.
//! - Glifos (`u16`) são da fonte Segoe Fluent Icons; veja [`glyph`].
//! - Não há rolagem: a altura é a soma dos controles, cortada na altura da tela. Listas
//!   longas devem limitar quantos itens mostram.
//! - Tamanhos estão em px antes da escala do monitor (o painel multiplica).
//! - Fecha sozinho com Esc, clique fora (o painel fica ativo e fecha ao perder o foco) ou
//!   novo clique no item. As janelas e o bitmap são destruídos ao fechar.

use std::cell::{self, RefCell};
use std::ffi::c_void;
use std::time::Instant;

use tiny_skia::{
    BlendMode, FillRule, FilterQuality, Paint, PathBuilder, Pattern, Pixmap, PixmapMut, PixmapPaint, Rect, SpreadMode, Stroke,
    Transform,
};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
    D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_HIT_TEST_METRICS, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING,
    DWRITE_TEXT_METRICS, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP,
    DWRITE_WORD_WRAPPING_WRAP, DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat,
    IDWriteTextLayout,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC,
    CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetMonitorInfoW, HBITMAP, HDC, HGDIOBJ,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_DELETE, VK_END, VK_ESCAPE,
    VK_DOWN, VK_HOME, VK_LEFT, VK_RETURN, VK_RIGHT, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, HSTRING, PCWSTR, w};
use windows_numerics::Vector2;

use crate::glass;
use crate::topbar::rounded_rect;

// Parte da API ainda sem uso: é para os painéis das próximas ondas (Wi‑Fi, Bluetooth).
#[allow(dead_code)]
/// Glifos úteis da Segoe Fluent Icons.
pub mod glyph {
    pub const CHECK: u16 = 0xE73E;
    pub const CHEVRON_LEFT: u16 = 0xE76B;
    pub const CHEVRON_RIGHT: u16 = 0xE76C;
    pub const SPEAKER: u16 = 0xE7F5;
    pub const HEADPHONE: u16 = 0xE7F6;
    pub const WIFI: u16 = 0xE701;
    pub const LOCK: u16 = 0xE72E;
    pub const BLUETOOTH: u16 = 0xE702;
    pub const REFRESH: u16 = 0xE72C;
    pub const SETTINGS: u16 = 0xE713;
    pub const CHEVRON_DOWN: u16 = 0xE70D;
}

/// Um controle do painel. A lista vem de [`PanelContent::build`], de cima para baixo.
#[allow(dead_code)]
pub enum Widget {
    /// Título grande (ex.: a data por extenso).
    Title(String),
    /// Subtítulo em negrito, com botões de ícone à direita: `(glifo, id)` → [`Event::Click`].
    Heading { text: String, buttons: Vec<(u16, u32)> },
    /// Texto secundário, quebra linhas.
    Text(String),
    /// Linha de lista clicável → [`Event::Click`]. `selected` mostra um ✓ na cor de destaque.
    Row { id: u32, glyph: Option<u16>, text: String, sub: Option<String>, selected: bool },
    /// Igual a [`Widget::Row`], com um glifo pequeno e apagado à direita (ex.: o cadeado das
    /// redes protegidas).
    RowBadge { id: u32, glyph: Option<u16>, text: String, sub: Option<String>, selected: bool, badge: u16 },
    /// Interruptor liga/desliga → [`Event::Toggle`] com o valor novo. `icon`: imagem à esquerda.
    Toggle { id: u32, text: String, on: bool, icon: Option<Pixmap> },
    /// Grade de imagens clicáveis (ex.: ícones da bandeja) → [`Event::Click`] e
    /// [`Event::RightClick`].
    Icons { items: Vec<(u32, Pixmap)> },
    /// Controle deslizante (`value` 0..1) → [`Event::Slide`]. `button`: ícone clicável à
    /// esquerda `(glifo, id)` → [`Event::Click`]. `label`: texto à direita (ex.: "45%").
    Slider { id: u32, button: Option<(u16, u32)>, value: f32, label: String },
    /// Botão da largura do painel → [`Event::Click`]. `primary`: na cor de destaque.
    Button { id: u32, text: String, primary: bool },
    Separator,
    /// Campo de texto de uma linha → [`Event::Text`] a cada edição, [`Event::Submit`] no Enter.
    /// `password`: mostra • no lugar dos caracteres.
    Field { id: u32, placeholder: String, password: bool },
    /// Grade de células de texto (calendário), `cols` colunas.
    Grid { cols: usize, cells: Vec<Cell> },
    /// Keyboard-navigable command list with an active-row highlight.
    Menu { items: Vec<(u32, Option<u16>, String)>, selected: Option<usize> },
    /// Campo de busca grande, sem caixa, com um glifo à esquerda e uma linha embaixo (topo de
    /// painel sem margem). Mesmos eventos de [`Widget::Field`].
    Search { id: u32, placeholder: String, glyph: u16 },
    /// Rótulo de seção (texto pequeno e apagado).
    Label(String),
    /// Lista de `rows` linhas com imagem à esquerda → [`Event::Click`] com o id da linha.
    /// `selected`: linha (posição em `items`) com fundo realçado. `scroll`: barra de rolagem
    /// `(início, tamanho)` em fração da lista toda.
    List { items: Vec<(u32, Option<Pixmap>, String)>, rows: usize, selected: Option<usize>, scroll: Option<(f32, f32)> },
    /// Rodapé (base de painel sem margem): texto com glifo à esquerda e dicas de teclado à
    /// direita `(texto, teclas)`.
    Footer { left: Option<(u16, String)>, hints: Vec<(String, Vec<String>)> },
    /// Cartões da Central de Controle numa grade de 2 colunas, com margem própria (use em
    /// painel sem margem). Cada cartão ocupa o primeiro lugar livre, da esquerda para a direita.
    Cards(Vec<Card>),
}

/// Cartão de [`Widget::Cards`]; todos em pílula translúcida.
pub enum Card {
    /// Uma coluna: círculo com glifo + título e subtítulo.
    Tile(Tile),
    /// Uma coluna e duas fileiras: capa (BGRA pré-multiplicado, quadrada), título, artista e
    /// três botões `(glifo, id)` → [`Event::Click`].
    Media { cover: Option<Pixmap>, title: String, artist: String, buttons: [(u16, u32); 3] },
    /// Largura toda: título em cima e, embaixo, glifo `left` (clicável se tiver id), trilho
    /// → [`Event::Slide`], glifo `right` e um botão redondo opcional `(glifo, id)` no fim.
    Slider { id: u32, title: String, value: f32, left: (u16, Option<u32>), right: u16, button: Option<(u16, u32)> },
}

impl Card {
    /// (colunas, fileiras).
    fn span(&self) -> (usize, usize) {
        match self {
            Card::Tile(_) => (1, 1),
            Card::Media { .. } => (1, 2),
            Card::Slider { .. } => (2, 1),
        }
    }
}

/// Posição (coluna, fileira) de cada cartão e o total de fileiras.
fn pack(cards: &[Card]) -> (Vec<(usize, usize)>, usize) {
    let mut used: Vec<[bool; 2]> = Vec::new();
    let free = |used: &Vec<[bool; 2]>, r: usize, c: usize| !used.get(r).is_some_and(|u| u[c]);
    let mut pos = Vec::with_capacity(cards.len());
    for card in cards {
        let (cw, rh) = card.span();
        let mut row = 0;
        let col = loop {
            if let Some(c) = (0..=2 - cw).find(|&c| (row..row + rh).all(|r| (c..c + cw).all(|k| free(&used, r, k)))) {
                break c;
            }
            row += 1;
        };
        if used.len() < row + rh {
            used.resize(row + rh, [false; 2]);
        }
        for r in &mut used[row..row + rh] {
            r[col..col + cw].fill(true);
        }
        pos.push((col, row));
    }
    (pos, used.len())
}

/// Bloco de [`Card::Tile`]: círculo com glifo (branco com glifo na cor de destaque se `on`)
/// que gera `toggle`; o resto do bloco gera `open`.
pub struct Tile {
    pub glyph: u16,
    pub title: String,
    pub sub: String,
    pub on: bool,
    pub toggle: u32,
    pub open: u32,
}

pub struct Cell {
    pub text: String,
    pub style: CellStyle,
    /// Com id, a célula é clicável → [`Event::Click`].
    pub id: Option<u32>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum CellStyle {
    /// Cabeçalho (texto pequeno e apagado).
    Header,
    Normal,
    /// Apagado (ex.: dias de outro mês).
    Dim,
    /// Círculo na cor de destaque (ex.: hoje).
    Accent,
}

fn localize_widgets(widgets: &mut [Widget]) {
    let translate = |value: &mut String| *value = crate::i18n::tr(value);
    for widget in widgets {
        match widget {
            Widget::Title(text) | Widget::Text(text) | Widget::Label(text) => translate(text),
            Widget::Heading { text, .. } | Widget::Toggle { text, .. } | Widget::Button { text, .. } => translate(text),
            Widget::Row { text, sub, .. } | Widget::RowBadge { text, sub, .. } => {
                translate(text);
                if let Some(text) = sub { translate(text); }
            }
            Widget::Field { placeholder, .. } | Widget::Search { placeholder, .. } => translate(placeholder),
            Widget::Grid { cells, .. } => cells.iter_mut().for_each(|cell| translate(&mut cell.text)),
            Widget::Footer { left, hints } => {
                if let Some((_, text)) = left { translate(text); }
                for (text, _) in hints { translate(text); }
            }
            Widget::Cards(cards) => {
                for card in cards {
                    match card {
                        Card::Tile(tile) => { translate(&mut tile.title); translate(&mut tile.sub); }
                        Card::Slider { title, .. } => translate(title),
                        // Titles and artists come from the active player, not the UI.
                        Card::Media { .. } => {}
                    }
                }
            }
            // App names, Wi-Fi SSIDs, and filenames are user/system content, not UI labels.
            Widget::Menu { items, .. } => items.iter_mut().for_each(|(_, _, text)| translate(text)),
            Widget::Icons { .. } | Widget::List { .. } | Widget::Slider { .. } | Widget::Separator => {}
        }
    }
}

/// O que o usuário fez num controle (o `u32` é o id dado no [`Widget`]).
#[allow(dead_code)]
pub enum Event<'a> {
    Click(u32),
    /// Clique com o botão direito numa área clicável.
    RightClick(u32),
    Toggle(u32, bool),
    /// Valor novo (0..1), enquanto arrasta.
    Slide(u32, f32),
    /// Texto do campo depois de uma edição.
    Text(u32, &'a str),
    /// Enter no campo.
    Submit(u32, &'a str),
    /// Navegação por teclado (a roda do mouse também envia cima/baixo): código da tecla.
    Key(u16),
    /// Pointer moved between interactive areas; clears keyboard-only selection when appropriate.
    PointerMove,
}

/// Conteúdo de um painel. Veja o exemplo no topo do módulo.
pub trait PanelContent {
    /// Largura em px (antes da escala).
    fn width(&self) -> f32 {
        320.0
    }
    /// `false` tira a margem de cima e de baixo (para [`Widget::Search`] e [`Widget::Footer`]).
    fn padded(&self) -> bool {
        true
    }
    /// `true` abre centralizado sob o item (ex.: calendário sob a hora); `false` alinha à direita.
    fn centered(&self) -> bool {
        false
    }
    /// `true` abre acima do item (itens do dock, na base da tela); `false` abre abaixo.
    fn above(&self) -> bool {
        false
    }
    /// `true`: vidro mais transparente e cantos bem arredondados (Central de Controle).
    fn liquid(&self) -> bool {
        false
    }
    /// Controles atuais, de cima para baixo. Chamado ao abrir e a cada reconstrução.
    fn build(&mut self) -> Vec<Widget>;
    /// Ação num controle. `true` = reconstruir (chamar `build` de novo).
    fn event(&mut self, ev: Event) -> bool;
    /// Mensagem repassada por [`notify`]. `true` = reconstruir.
    fn notify(&mut self, _msg: u32, _wparam: usize) -> bool {
        false
    }
}

const CLASS: PCWSTR = w!("ArcDockWinPanel");
/// Fecha o painel; postada para a própria janela. lParam 1 = devolver o foco.
const WM_PANEL_CLOSE: u32 = WM_APP + 20;
const WM_MOUSELEAVE: u32 = 0x02A3;
const TIMER_ANIM: usize = 1;
const ANIM_MS: f32 = 120.0;
/// Deslize da animação de abrir (px antes da escala).
const SLIDE: f32 = 6.0;
/// Margens da janela para a sombra (lados e baixo; em cima é só o espaço até a barra).
const SHADOW: f32 = 16.0;
const GAP: f32 = 6.0;
const RADIUS: f32 = 12.0;
/// Raio do painel em [`PanelContent::liquid`]: concêntrico com os cartões de dentro
/// (raio deles + margem até a borda).
const LIQUID_RADIUS: f32 = CARD_RADIUS + CARD_PAD;
/// Margem lateral das áreas clicáveis e recuo do texto dentro delas.
const SIDE: f32 = 8.0;
const INSET: f32 = 8.0;
const PAD_Y: f32 = 8.0;
/// Lado da célula de [`Widget::Icons`].
const ICON_CELL: f32 = 36.0;
/// Altura da linha de [`Widget::List`].
const LIST_ROW: f32 = 44.0;
/// [`Widget::Cards`]: altura da fileira, espaço entre cartões e margem em volta.
const CARD_H: f32 = 60.0;
const CARD_GAP: f32 = 12.0;
const CARD_PAD: f32 = 14.0;
/// Canto dos cartões de [`Widget::Cards`].
const CARD_RADIUS: f32 = 18.0;

thread_local! {
    static PANEL: RefCell<Option<Panel>> = const { RefCell::new(None) };
    static OPEN_HWND: cell::Cell<isize> = const { cell::Cell::new(0) };
}

#[derive(Clone, Copy, PartialEq)]
enum ZoneKind {
    Click(u32),
    Toggle(u32, bool),
    /// Trilho do controle deslizante: x inicial e final (panel-local).
    Slider(u32, f32, f32),
    Field(u32),
}

struct Zone {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    kind: ZoneKind,
}

struct Field {
    id: u32,
    text: Vec<u16>,
    cursor: usize,
}

#[derive(Clone, Copy)]
enum Font {
    Body,
    Bold,
    Title,
    Small,
    Icon,
    Large,
    /// Glifos maiores (botões da mídia).
    IconLarge,
}

/// Texto a desenhar com DirectWrite depois das formas (coordenadas da janela).
struct TextOp {
    text: Vec<u16>,
    font: Font,
    rect: [f32; 4],
    align: DWRITE_TEXT_ALIGNMENT,
    color: [f32; 4],
    wrap: bool,
    clip: Option<[f32; 4]>,
}

struct Palette {
    tint: [u8; 4],
    border: [u8; 4],
    line: [u8; 4],
    hover: [u8; 4],
    control: [u8; 4],
    strong: [u8; 4],
    shadow: u8,
    accent: [u8; 4],
    ink: [f32; 4],
    dim: [f32; 4],
    /// Tinta do vidro em [`PanelContent::liquid`].
    glass: [u8; 4],
    /// Cartões de [`Widget::Cards`]: fundo, contorno, círculo desligado, trilho e preenchimento.
    card: [u8; 4],
    card_line: [u8; 4],
    circle: [u8; 4],
    track: [u8; 4],
    fill: [u8; 4],
}

impl Palette {
    fn new() -> Self {
        let accent = crate::accent_color().unwrap_or([0, 120, 212, 255]);
        if crate::light_theme() {
            Self {
                tint: [249, 249, 251, 205],
                border: [0, 0, 0, 40],
                line: [0, 0, 0, 24],
                hover: [0, 0, 0, 16],
                control: [0, 0, 0, 14],
                strong: [0, 0, 0, 70],
                shadow: 3,
                accent,
                ink: [0.0, 0.0, 0.0, 0.9],
                dim: [0.0, 0.0, 0.0, 0.55],
                glass: [246, 246, 248, 150],
                card: [255, 255, 255, 115],
                card_line: [255, 255, 255, 190],
                circle: [0, 0, 0, 24],
                track: [0, 0, 0, 30],
                fill: [80, 80, 86, 255],
            }
        } else {
            Self {
                tint: [36, 36, 40, 200],
                border: [255, 255, 255, 38],
                line: [255, 255, 255, 26],
                hover: [255, 255, 255, 22],
                control: [255, 255, 255, 18],
                strong: [255, 255, 255, 80],
                shadow: 6,
                accent,
                ink: [1.0, 1.0, 1.0, 0.95],
                dim: [1.0, 1.0, 1.0, 0.6],
                glass: [28, 28, 32, 150],
                card: [255, 255, 255, 30],
                card_line: [255, 255, 255, 45],
                circle: [255, 255, 255, 48],
                track: [255, 255, 255, 55],
                fill: [255, 255, 255, 255],
            }
        }
    }
}

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

fn rgba(c: [u8; 4]) -> [f32; 4] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, c[3] as f32 / 255.0]
}

/// Tinta para o tiny-skia desenhando direto na DIB (BGRA): troca vermelho e azul.
fn paint(c: [u8; 4]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c[2], c[1], c[0], c[3]);
    p.anti_alias = true;
    p
}

/// DIB em que o painel é desenhado (tiny-skia e Direct2D) e que vai para a janela.
struct Buffer {
    memdc: HDC,
    dib: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    w: i32,
    h: i32,
}

impl Buffer {
    fn new(w: i32, h: i32) -> Option<Self> {
        unsafe {
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
            let dib = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
            let memdc = CreateCompatibleDC(None);
            let old = SelectObject(memdc, HGDIOBJ(dib.0));
            Some(Self { memdc, dib, old, bits: bits as *mut u8, w, h })
        }
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.memdc, self.old);
            let _ = DeleteObject(HGDIOBJ(self.dib.0));
            let _ = DeleteDC(self.memdc);
        }
    }
}

struct Panel {
    key: &'static str,
    hwnd: HWND,
    glass: HWND,
    glass_fx: Option<glass::Glass>,
    /// Janela que estava em primeiro plano antes de abrir (volta para ela no Esc).
    prev_fg: HWND,
    item: RECT,
    work: RECT,
    scale: f32,
    /// Retângulo do painel na tela (posição final, sem a sombra).
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    buf: Option<Buffer>,
    target: ID2D1DCRenderTarget,
    dwrite: IDWriteFactory,
    fonts: [IDWriteTextFormat; 7],
    content: Box<dyn PanelContent>,
    /// Controles com y e altura (panel-local).
    widgets: Vec<(Widget, f32, f32)>,
    zones: Vec<Zone>,
    hot: Option<usize>,
    pressed: Option<usize>,
    /// Arrastando o controle deslizante: id e valor atual.
    drag: Option<(u32, f32)>,
    fields: Vec<Field>,
    focus: Option<u32>,
    mouse: Option<(f32, f32)>,
    tracking: bool,
    start: Instant,
    /// Progresso da animação de abrir (0..1, já suavizado).
    t: f32,
}

impl Panel {
    fn new(
        key: &'static str,
        item: RECT,
        content: Box<dyn PanelContent>,
        prev_fg: HWND,
    ) -> windows::core::Result<Self> {
        unsafe {
            let module = GetModuleHandleW(None)?;
            let hinst: HINSTANCE = module.into();
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinst,
                lpszClassName: CLASS,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            };
            // Falha a partir do segundo painel (classe já existe): tudo bem.
            RegisterClassW(&wc);

            let anchor = POINT { x: item.right, y: item.bottom };
            let monitor = MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(monitor, &mut mi);
            let (mut dx, mut dy) = (96u32, 96u32);
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            let scale = dx as f32 / 96.0;

            let ex = WS_EX_TOOLWINDOW | WS_EX_TOPMOST;
            // Vidro (classe do dock): sem superfície GDI, o conteúdo vem da composição.
            let glass = CreateWindowExW(
                ex | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
                w!("ArcDockWinGlass"),
                w!("ArcDockWinPanelGlass"),
                WS_POPUP,
                anchor.x,
                anchor.y,
                1,
                1,
                None,
                None,
                Some(hinst),
                None,
            )?;
            // Conteúdo: em camadas, "dona" pelo vidro; esta pode ser ativada (teclado).
            let hwnd = match CreateWindowExW(
                ex | WS_EX_LAYERED,
                CLASS,
                CLASS,
                WS_POPUP,
                anchor.x,
                anchor.y,
                1,
                1,
                Some(glass),
                None,
                Some(hinst),
                None,
            ) {
                Ok(h) => h,
                Err(e) => {
                    let _ = DestroyWindow(glass);
                    return Err(e);
                }
            };
            let radius = if content.liquid() { LIQUID_RADIUS } else { RADIUS };
            let glass_fx = glass::Glass::new(glass, radius * scale).ok();

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
            let mut coll: Option<IDWriteFontCollection> = None;
            dwrite.GetSystemFontCollection(&mut coll, false)?;
            let has = |name: PCWSTR| {
                let (mut i, mut exists) = (0u32, BOOL(0));
                coll.as_ref().is_some_and(|f| f.FindFamilyName(name, &mut i, &mut exists).is_ok() && exists.as_bool())
            };
            let family = if has(w!("Segoe UI Variable Text")) { w!("Segoe UI Variable Text") } else { w!("Segoe UI") };
            let locale = HSTRING::from(crate::i18n::text_locale());
            let locale_ptr = PCWSTR::from_raw(locale.as_ptr());
            let format = |family: PCWSTR, weight: DWRITE_FONT_WEIGHT, size: f32| -> windows::core::Result<IDWriteTextFormat> {
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
            let fonts = [
                format(family, DWRITE_FONT_WEIGHT_NORMAL, 13.0)?,
                format(family, DWRITE_FONT_WEIGHT_SEMI_BOLD, 13.0)?,
                format(family, DWRITE_FONT_WEIGHT_SEMI_BOLD, 15.0)?,
                format(family, DWRITE_FONT_WEIGHT_NORMAL, 11.0)?,
                format(w!("Segoe Fluent Icons"), DWRITE_FONT_WEIGHT_NORMAL, 16.0)?,
                format(family, DWRITE_FONT_WEIGHT_NORMAL, 15.0)?,
                format(w!("Segoe Fluent Icons"), DWRITE_FONT_WEIGHT_NORMAL, 20.0)?,
            ];

            Ok(Self {
                key,
                hwnd,
                glass,
                glass_fx,
                prev_fg,
                item,
                work: mi.rcWork,
                scale,
                x: 0,
                y: 0,
                w: 0,
                h: 0,
                buf: None,
                target,
                dwrite,
                fonts,
                content,
                widgets: Vec::new(),
                zones: Vec::new(),
                hot: None,
                pressed: None,
                drag: None,
                fields: Vec::new(),
                focus: None,
                mouse: None,
                tracking: false,
                start: Instant::now(),
                t: 0.0,
            })
        }
    }

    fn font(&self, f: Font) -> &IDWriteTextFormat {
        &self.fonts[f as usize]
    }

    fn layout_text(&self, text: &[u16], font: Font, w: f32, h: f32) -> Option<IDWriteTextLayout> {
        unsafe { self.dwrite.CreateTextLayout(text, self.font(font), w.max(1.0), h.max(1.0)).ok() }
    }

    /// Margens da janela em volta do painel: (lados/baixo, cima).
    fn margins(&self) -> (f32, f32) {
        (SHADOW * self.scale, GAP * self.scale)
    }

    fn widget_height(&self, w: &Widget) -> f32 {
        let s = self.scale;
        let px = match w {
            Widget::Title(_) => 40.0,
            Widget::Heading { .. } => 36.0,
            Widget::Text(t) => {
                let inner = self.w as f32 - 2.0 * (SIDE + INSET) * s;
                let text: Vec<u16> = t.encode_utf16().collect();
                let h = self
                    .layout_text(&text, Font::Body, inner, 10_000.0)
                    .and_then(|l| unsafe {
                        l.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP).ok()?;
                        let mut m = DWRITE_TEXT_METRICS::default();
                        l.GetMetrics(&mut m).ok()?;
                        Some(m.height)
                    })
                    .unwrap_or(18.0 * s);
                return h.ceil() + 8.0 * s;
            }
            Widget::Row { sub, .. } | Widget::RowBadge { sub, .. } => {
                if sub.is_some() {
                    48.0
                } else {
                    36.0
                }
            }
            Widget::Toggle { .. } => 40.0,
            Widget::Search { .. } => 56.0,
            Widget::Label(_) => 32.0,
            Widget::List { rows, .. } => LIST_ROW * *rows as f32,
            Widget::Menu { items, .. } => LIST_ROW * items.len() as f32,
            Widget::Footer { .. } => 44.0,
            Widget::Slider { .. } => 44.0,
            Widget::Button { .. } => 40.0,
            Widget::Separator => 9.0,
            Widget::Field { .. } => 40.0,
            Widget::Grid { cols, cells } => 32.0 * cells.len().div_ceil((*cols).max(1)) as f32,
            Widget::Icons { items } => ICON_CELL * items.len().div_ceil(self.icon_cols()) as f32,
            Widget::Cards(c) => {
                let rows = pack(c).1 as f32;
                2.0 * CARD_PAD + rows * CARD_H + (rows - 1.0).max(0.0) * CARD_GAP
            }
        };
        px * s
    }

    /// Pede os controles ao conteúdo, refaz o layout e redesenha.
    fn rebuild(&mut self) {
        let s = self.scale;
        let mut widgets = self.content.build();
        localize_widgets(&mut widgets);

        // Campos de texto: mantém o texto dos que continuam; o primeiro novo ganha o foco.
        let ids: Vec<u32> =
            widgets
            .iter()
            .filter_map(|w| match w {
                Widget::Field { id, .. } | Widget::Search { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        self.fields.retain(|f| ids.contains(&f.id));
        for &id in &ids {
            if !self.fields.iter().any(|f| f.id == id) {
                self.fields.push(Field { id, text: Vec::new(), cursor: 0 });
                if self.focus.is_none_or(|f| !ids.contains(&f)) {
                    self.focus = Some(id);
                }
            }
        }
        if self.focus.is_some_and(|f| !ids.contains(&f)) {
            self.focus = None;
        }

        self.w = (self.content.width() * s).round() as i32;
        let pad = if self.content.padded() { PAD_Y * s } else { 0.0 };
        let mut y = pad;
        let mut placed = Vec::with_capacity(widgets.len());
        for wd in widgets {
            let h = self.widget_height(&wd);
            placed.push((wd, y, h));
            y += h;
        }
        let above = self.content.above();
        let room = if above { self.item.top - self.work.top } else { self.work.bottom - self.item.bottom };
        let max_h = room as f32 - (GAP + SHADOW) * s;
        self.h = (y + pad).min(max_h).ceil() as i32;
        self.widgets = placed;

        // Posição: borda direita na do item (ou centro no centro), sem sair da área de trabalho.
        let side = (8.0 * s) as i32;
        let want = if self.content.centered() {
            (self.item.left + self.item.right - self.w) / 2
        } else {
            self.item.right - self.w
        };
        self.x = want.min(self.work.right - side - self.w).max(self.work.left + side);
        self.y = if above {
            self.item.top - (GAP * s).round() as i32 - self.h
        } else {
            self.item.bottom + (GAP * s).round() as i32
        };

        let (m, mt) = self.margins();
        let bw = self.w + 2 * m as i32;
        let bh = self.h + mt as i32 + m as i32;
        if self.buf.as_ref().is_none_or(|b| b.w != bw || b.h != bh) {
            self.buf = None;
            self.buf = Buffer::new(bw, bh);
            if let Some(fx) = &self.glass_fx {
                fx.resize(self.w as f32, self.h as f32);
            }
        }
        self.hot = None;
        self.pressed = None;
        self.redraw();
        if let Some((x, y)) = self.mouse {
            self.hover(x, y);
        }
    }

    fn redraw(&mut self) {
        let Some(buf) = &self.buf else { return };
        let (bw, bh) = (buf.w, buf.h);
        let (memdc, bits) = (buf.memdc, buf.bits);
        let s = self.scale;
        let pal = Palette::new();
        let (m, mt) = self.margins();
        let (w, h) = (self.w as f32, self.h as f32);
        let bytes = unsafe { std::slice::from_raw_parts_mut(bits, (bw * bh * 4) as usize) };
        bytes.fill(0);
        let Some(mut pm) = PixmapMut::from_bytes(bytes, bw as u32, bh as u32) else { return };
        let id = Transform::identity();
        let tf = Transform::from_translate(m, mt);
        let liquid = self.content.liquid();
        let radius = if liquid { LIQUID_RADIUS } else { RADIUS } * s;

        // Sombra: anéis cada vez maiores e fracos, deslocados para baixo; depois limpa o miolo.
        let steps = 8;
        for i in 1..=steps {
            let e = m * i as f32 / steps as f32;
            if let Some(p) = rounded_rect(m - e, mt + 3.0 * s - e, w + 2.0 * e, h + 2.0 * e, radius + e) {
                pm.fill_path(&p, &paint([0, 0, 0, pal.shadow]), FillRule::Winding, id, None);
            }
        }
        if let Some(p) = rounded_rect(0.0, 0.0, w, h, radius) {
            let mut clear = paint([0, 0, 0, 255]);
            clear.blend_mode = BlendMode::Clear;
            pm.fill_path(&p, &clear, FillRule::Winding, tf, None);
            pm.fill_path(&p, &paint(if liquid { pal.glass } else { pal.tint }), FillRule::Winding, tf, None);
        }
        if let Some(p) = rounded_rect(0.5, 0.5, w - 1.0, h - 1.0, radius - 0.5) {
            let stroke = Stroke { width: 1.0, ..Default::default() };
            pm.stroke_path(&p, &paint(pal.border), &stroke, tf, None);
        }

        let fill = |pm: &mut PixmapMut, x: f32, y: f32, w: f32, h: f32, r: f32, c: [u8; 4]| {
            if let Some(p) = rounded_rect(x, y, w, h, r) {
                pm.fill_path(&p, &paint(c), FillRule::Winding, tf, None);
            }
        };

        // Imagem (RGBA) centrada em (cx, cy); a DIB é BGRA, então troca vermelho e azul.
        let draw_icon = |pm: &mut PixmapMut, icon: &Pixmap, cx: f32, cy: f32| {
            let mut c = icon.clone();
            for px in c.data_mut().chunks_exact_mut(4) {
                px.swap(0, 2);
            }
            let x = (cx - icon.width() as f32 / 2.0).round() as i32;
            let y = (cy - icon.height() as f32 / 2.0).round() as i32;
            pm.draw_pixmap(x, y, c.as_ref(), &PixmapPaint::default(), tf, None);
        };

        let mut texts: Vec<TextOp> = Vec::new();
        let mk = |t: &str, font: Font, r: [f32; 4], align: DWRITE_TEXT_ALIGNMENT, color: [f32; 4]| {
            TextOp {
                text: t.encode_utf16().collect(),
                font,
                rect: [r[0] + m, r[1] + mt, r[2], r[3]],
                align,
                color,
                wrap: false,
                clip: None,
            }
        };
        let glyph = |g: u16| String::from_utf16_lossy(&[g]);
        let (left, right) = (SIDE * s, w - SIDE * s);
        let inset = INSET * s;
        let mut zones = Vec::new();
        let mut caret: Option<[f32; 4]> = None;
        let mut field_ops: Vec<TextOp> = Vec::new();

        for (wd, y, wh) in &self.widgets {
            let (y, wh) = (*y, *wh);
            if y + wh > h {
                break;
            }
            match wd {
                Widget::Title(t) => {
                    texts.push(mk(t, Font::Title, [left + inset, y, right - left - 2.0 * inset, wh], DWRITE_TEXT_ALIGNMENT_LEADING, pal.ink));
                }
                Widget::Heading { text: t, buttons } => {
                    let (bs, gap) = (28.0 * s, 4.0 * s);
                    let n = buttons.len() as f32;
                    let first = right - n * bs - (n - 1.0).max(0.0) * gap;
                    for (k, &(g, bid)) in buttons.iter().enumerate() {
                        let bx = first + k as f32 * (bs + gap);
                        let by = y + (wh - bs) / 2.0;
                        zones.push(Zone { x: bx, y: by, w: bs, h: bs, radius: 6.0 * s, kind: ZoneKind::Click(bid) });
                        texts.push(mk(&glyph(g), Font::Icon, [bx, by, bs, bs], DWRITE_TEXT_ALIGNMENT_CENTER, pal.ink));
                    }
                    let tw = if buttons.is_empty() { right - inset } else { first - 4.0 * s } - (left + inset);
                    texts.push(mk(t, Font::Bold, [left + inset, y, tw, wh], DWRITE_TEXT_ALIGNMENT_LEADING, pal.ink));
                }
                Widget::Text(t) => {
                    texts.push(TextOp {
                        text: t.encode_utf16().collect(),
                        font: Font::Body,
                        rect: [left + inset + m, y + mt, right - left - 2.0 * inset, wh],
                        align: DWRITE_TEXT_ALIGNMENT_LEADING,
                        color: pal.dim,
                        wrap: true,
                        clip: None,
                    });
                }
                Widget::Row { id: rid, glyph: g, text: t, sub, selected }
                | Widget::RowBadge { id: rid, glyph: g, text: t, sub, selected, .. } => {
                    zones.push(Zone {
                        x: left,
                        y: y + 2.0 * s,
                        w: right - left,
                        h: wh - 4.0 * s,
                        radius: 6.0 * s,
                        kind: ZoneKind::Click(*rid),
                    });
                    let mut tx = left + inset;
                    if let Some(g) = g {
                        texts.push(mk(&glyph(*g), Font::Icon, [tx, y, 20.0 * s, wh], DWRITE_TEXT_ALIGNMENT_CENTER, pal.ink));
                        tx += 30.0 * s;
                    }
                    let check = 26.0 * s;
                    if *selected {
                        let a = rgba(pal.accent);
                        texts.push(mk(&glyph(glyph::CHECK), Font::Icon, [right - inset - 18.0 * s, y, 18.0 * s, wh], DWRITE_TEXT_ALIGNMENT_CENTER, a));
                    }
                    let mut tw = right - inset - check - tx;
                    if let Widget::RowBadge { badge, .. } = wd {
                        let bx = if *selected { right - inset - check - 16.0 * s } else { right - inset - 18.0 * s };
                        texts.push(mk(&glyph(*badge), Font::Icon, [bx, y, 16.0 * s, wh], DWRITE_TEXT_ALIGNMENT_CENTER, pal.dim));
                        if *selected {
                            tw -= 20.0 * s;
                        }
                    }
                    let font = if *selected { Font::Bold } else { Font::Body };
                    match sub {
                        Some(sub) => {
                            texts.push(mk(t, font, [tx, y + 5.0 * s, tw, 20.0 * s], DWRITE_TEXT_ALIGNMENT_LEADING, pal.ink));
                            texts.push(mk(sub, Font::Small, [tx, y + 24.0 * s, tw, 18.0 * s], DWRITE_TEXT_ALIGNMENT_LEADING, pal.dim));
                        }
                        None => texts.push(mk(t, font, [tx, y, tw, wh], DWRITE_TEXT_ALIGNMENT_LEADING, pal.ink)),
                    }
                }
                Widget::Toggle { id: tid, text: t, on, icon } => {
                    zones.push(Zone {
                        x: left,
                        y: y + 2.0 * s,
                        w: right - left,
                        h: wh - 4.0 * s,
                        radius: 6.0 * s,
                        kind: ZoneKind::Toggle(*tid, *on),
                    });
                    let (sw, sh) = (36.0 * s, 20.0 * s);
                    let (sx, sy) = (right - inset - sw, y + (wh - sh) / 2.0);
                    fill(&mut pm, sx, sy, sw, sh, sh / 2.0, if *on { pal.accent } else { pal.strong });
                    let kr = sh / 2.0 - 3.0 * s;
                    let kx = if *on { sx + sw - sh / 2.0 } else { sx + sh / 2.0 };
                    fill(&mut pm, kx - kr, sy + sh / 2.0 - kr, 2.0 * kr, 2.0 * kr, kr, [255, 255, 255, 255]);
                    let mut tx = left + inset;
                    if let Some(icon) = icon {
                        draw_icon(&mut pm, icon, tx + 10.0 * s, y + wh / 2.0);
                        tx += 30.0 * s;
                    }
                    texts.push(mk(t, Font::Body, [tx, y, sx - tx - inset, wh], DWRITE_TEXT_ALIGNMENT_LEADING, pal.ink));
                }
                Widget::Label(t) => {
                    texts.push(mk(t, Font::Small, [left + inset, y + 6.0 * s, right - left - 2.0 * inset, wh - 6.0 * s], DWRITE_TEXT_ALIGNMENT_LEADING, pal.dim));
                }
                Widget::List { items, selected, scroll, .. } => {
                    let rh = LIST_ROW * s;
                    let bar = if scroll.is_some() { 10.0 * s } else { 0.0 };
                    for (k, (rid, icon, t)) in items.iter().enumerate() {
                        let ry = y + k as f32 * rh;
                        let (zy, zh) = (ry + 2.0 * s, rh - 4.0 * s);
                        zones.push(Zone { x: left, y: zy, w: right - left - bar, h: zh, radius: 6.0 * s, kind: ZoneKind::Click(*rid) });
                        if *selected == Some(k) {
                            fill(&mut pm, left, zy, right - left - bar, zh, 6.0 * s, pal.hover);
                        }
                        let tx = left + inset;
                        if let Some(icon) = icon {
                            draw_icon(&mut pm, icon, tx + 12.0 * s, ry + rh / 2.0);
                        }
                        texts.push(mk(t, Font::Body, [tx + 40.0 * s, ry, right - bar - inset - tx - 40.0 * s, rh], DWRITE_TEXT_ALIGNMENT_LEADING, pal.ink));
                    }
                    if let Some((start, size)) = scroll {
                        let (bw, bx) = (3.0 * s, right - 3.0 * s);
                        let (ty, th) = (y + 4.0 * s, wh - 8.0 * s);
                        let tl = (th * size).max(24.0 * s).min(th);
                        let top = ty + (th - tl) * (start / (1.0 - size).max(0.0001)).clamp(0.0, 1.0);
                        fill(&mut pm, bx, top, bw, tl, bw / 2.0, pal.strong);
                    }
                }
                Widget::Menu { items, selected } => {
                    let rh = LIST_ROW * s;
                    for (i, (id, icon, label)) in items.iter().enumerate() {
                        let ry = y + i as f32 * rh;
                        let (zy, zh) = (ry + 2.0 * s, rh - 4.0 * s);
                        zones.push(Zone { x: left, y: zy, w: right - left, h: zh, radius: 6.0 * s, kind: ZoneKind::Click(*id) });
                        if Some(i) == *selected {
                            fill(&mut pm, left, zy, right - left, zh, 6.0 * s, pal.hover);
                        }
                        let tx = left + inset;
                        if let Some(icon) = icon {
                            texts.push(mk(&glyph(*icon), Font::Icon, [tx, ry, 28.0 * s, rh], DWRITE_TEXT_ALIGNMENT_CENTER, pal.dim));
                        }
                        texts.push(mk(label, Font::Body, [tx + 40.0 * s, ry, right - inset - tx - 40.0 * s, rh], DWRITE_TEXT_ALIGNMENT_LEADING, pal.ink));
                    }
                }
                Widget::Footer { left: label, hints } => {
                    // Fundo um pouco mais escuro, com os cantos de baixo arredondados como o painel.
                    let r = radius;
                    let k = r * 0.5523;
                    let mut pb = PathBuilder::new();
                    pb.move_to(0.0, y);
                    pb.line_to(w, y);
                    pb.line_to(w, y + wh - r);
                    pb.cubic_to(w, y + wh - r + k, w - r + k, y + wh, w - r, y + wh);
                    pb.line_to(r, y + wh);
                    pb.cubic_to(r - k, y + wh, 0.0, y + wh - r + k, 0.0, y + wh - r);
                    pb.close();
                    if let Some(p) = pb.finish() {
                        pm.fill_path(&p, &paint(pal.control), FillRule::Winding, tf, None);
                    }
                    if let Some(rl) = Rect::from_xywh(0.0, y, w, 1.0) {
                        pm.fill_rect(rl, &paint(pal.line), tf, None);
                    }
                    let mut lx = left + inset;
                    if let Some((g, t)) = label {
                        texts.push(mk(&glyph(*g), Font::Icon, [lx, y, 18.0 * s, wh], DWRITE_TEXT_ALIGNMENT_CENTER, pal.ink));
                        lx += 26.0 * s;
                        texts.push(mk(t, Font::Small, [lx, y, w / 2.0, wh], DWRITE_TEXT_ALIGNMENT_LEADING, pal.ink));
                    }
                    // Dicas da direita para a esquerda: teclas em "chips" com borda, texto antes.
                    let text_w = |t: &str, font: Font| {
                        let t: Vec<u16> = t.encode_utf16().collect();
                        self.layout_text(&t, font, 10_000.0, 100.0)
                            .and_then(|l| unsafe {
                                let mut m = DWRITE_TEXT_METRICS::default();
                                l.GetMetrics(&mut m).ok()?;
                                Some(m.widthIncludingTrailingWhitespace)
                            })
                            .unwrap_or(0.0)
                    };
                    let (ch, cy) = (20.0 * s, y + (wh - 20.0 * s) / 2.0);
                    let mut x = right - inset;
                    for (t, keys) in hints.iter().rev() {
                        for key in keys.iter().rev() {
                            let cw = (text_w(key, Font::Small) + 12.0 * s).max(ch);
                            x -= cw;
                            if let Some(p) = rounded_rect(x + 0.5, cy + 0.5, cw - 1.0, ch - 1.0, 4.0 * s) {
                                let stroke = Stroke { width: 1.0, ..Default::default() };
                                pm.stroke_path(&p, &paint(pal.strong), &stroke, tf, None);
                            }
                            texts.push(mk(key, Font::Small, [x, cy, cw, ch], DWRITE_TEXT_ALIGNMENT_CENTER, pal.ink));
                            x -= 4.0 * s;
                        }
                        let tw = text_w(t, Font::Small).ceil();
                        x -= tw + 4.0 * s;
                        texts.push(mk(t, Font::Small, [x, y, tw + 2.0 * s, wh], DWRITE_TEXT_ALIGNMENT_LEADING, pal.ink));
                        x -= 16.0 * s;
                    }
                }
                Widget::Icons { items } => {
                    let cols = self.icon_cols();
                    let cell = ICON_CELL * s;
                    for (i, (iid, icon)) in items.iter().enumerate() {
                        let cx = left + (i % cols) as f32 * cell;
                        let cy = y + (i / cols) as f32 * cell;
                        let k = 2.0 * s;
                        zones.push(Zone { x: cx + k, y: cy + k, w: cell - 2.0 * k, h: cell - 2.0 * k, radius: 6.0 * s, kind: ZoneKind::Click(*iid) });
                        draw_icon(&mut pm, icon, cx + cell / 2.0, cy + cell / 2.0);
                    }
                }
                Widget::Cards(cards) => {
                    let (pad, gap, ch) = (CARD_PAD * s, CARD_GAP * s, CARD_H * s);
                    let cw = (w - 2.0 * pad - gap) / 2.0;
                    let r = CARD_RADIUS * s;
                    let stroke = Stroke { width: 1.0, ..Default::default() };
                    let (pos, _) = pack(cards);
                    for (card, &(col, row)) in cards.iter().zip(&pos) {
                        let (sc, sr) = card.span();
                        let cx = pad + col as f32 * (cw + gap);
                        let cy = y + pad + row as f32 * (ch + gap);
                        let bw = sc as f32 * cw + (sc - 1) as f32 * gap;
                        let bh = sr as f32 * ch + (sr - 1) as f32 * gap;
                        fill(&mut pm, cx, cy, bw, bh, r, pal.card);
                        if let Some(p) = rounded_rect(cx + 0.5, cy + 0.5, bw - 1.0, bh - 1.0, r - 0.5) {
                            pm.stroke_path(&p, &paint(pal.card_line), &stroke, tf, None);
                        }
                        match card {
                            Card::Tile(t) => {
                                let d = 38.0 * s;
                                let (ox, oy) = (cx + (ch - d) / 2.0, cy + (ch - d) / 2.0);
                                fill(&mut pm, ox, oy, d, d, d / 2.0, if t.on { [255, 255, 255, 255] } else { pal.circle });
                                // Círculo antes do bloco: a primeira zona sob o mouse ganha o clique.
                                zones.push(Zone { x: ox, y: oy, w: d, h: d, radius: d / 2.0, kind: ZoneKind::Click(t.toggle) });
                                zones.push(Zone { x: cx, y: cy, w: bw, h: bh, radius: r, kind: ZoneKind::Click(t.open) });
                                let color = if t.on { rgba(pal.accent) } else { pal.ink };
                                texts.push(mk(&glyph(t.glyph), Font::Icon, [ox, oy, d, d], DWRITE_TEXT_ALIGNMENT_CENTER, color));
                                let lx = ox + d + 10.0 * s;
                                let lw = cx + bw - 14.0 * s - lx;
                                texts.push(mk(&t.title, Font::Bold, [lx, cy + ch / 2.0 - 17.0 * s, lw, 18.0 * s], DWRITE_TEXT_ALIGNMENT_LEADING, pal.ink));
                                texts.push(mk(&t.sub, Font::Small, [lx, cy + ch / 2.0 + 1.0 * s, lw, 16.0 * s], DWRITE_TEXT_ALIGNMENT_LEADING, pal.dim));
                            }
                            Card::Media { cover, title, artist, buttons } => {
                                let (ix, iw) = (cx + 16.0 * s, bw - 32.0 * s);
                                let (a, ay) = (36.0 * s, cy + 14.0 * s);
                                let art = rounded_rect(ix, ay, a, a, 8.0 * s);
                                match (cover, art) {
                                    (Some(img), Some(p)) => {
                                        let k = a / img.width().max(1) as f32;
                                        let ts = Transform::from_scale(k, k).post_translate(ix, ay);
                                        let mut pt = Paint::default();
                                        pt.anti_alias = true;
                                        pt.shader = Pattern::new(img.as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, 1.0, ts);
                                        pm.fill_path(&p, &pt, FillRule::Winding, tf, None);
                                    }
                                    _ => {
                                        fill(&mut pm, ix, ay, a, a, 8.0 * s, pal.circle);
                                        texts.push(mk(&glyph(0xEC4F), Font::Icon, [ix, ay, a, a], DWRITE_TEXT_ALIGNMENT_CENTER, pal.ink));
                                    }
                                }
                                texts.push(mk(title, Font::Bold, [ix, ay + a + 4.0 * s, iw, 18.0 * s], DWRITE_TEXT_ALIGNMENT_LEADING, pal.ink));
                                texts.push(mk(artist, Font::Small, [ix, ay + a + 22.0 * s, iw, 16.0 * s], DWRITE_TEXT_ALIGNMENT_LEADING, pal.dim));
                                let (bs, bhh) = (40.0 * s, 30.0 * s);
                                let by = cy + bh - 10.0 * s - bhh;
                                let bx0 = cx + (bw - 3.0 * bs) / 2.0;
                                for (k, &(g, bid)) in buttons.iter().enumerate() {
                                    let bx = bx0 + k as f32 * bs;
                                    zones.push(Zone { x: bx, y: by, w: bs, h: bhh, radius: 8.0 * s, kind: ZoneKind::Click(bid) });
                                    texts.push(mk(&glyph(g), Font::IconLarge, [bx, by, bs, bhh], DWRITE_TEXT_ALIGNMENT_CENTER, pal.ink));
                                }
                            }
                            Card::Slider { id: sid, title, value, left, right, button } => {
                                texts.push(mk(title, Font::Bold, [cx + 18.0 * s, cy + 6.0 * s, bw - 36.0 * s, 20.0 * s], DWRITE_TEXT_ALIGNMENT_LEADING, pal.ink));
                                let gs = 24.0 * s;
                                let ry = cy + 40.0 * s;
                                let gx = cx + 12.0 * s;
                                if let (g, Some(bid)) = left {
                                    zones.push(Zone { x: gx, y: ry - gs / 2.0, w: gs, h: gs, radius: gs / 2.0, kind: ZoneKind::Click(*bid) });
                                    texts.push(mk(&glyph(*g), Font::Icon, [gx, ry - gs / 2.0, gs, gs], DWRITE_TEXT_ALIGNMENT_CENTER, pal.ink));
                                } else {
                                    texts.push(mk(&glyph(left.0), Font::Icon, [gx, ry - gs / 2.0, gs, gs], DWRITE_TEXT_ALIGNMENT_CENTER, pal.ink));
                                }
                                let mut end = cx + bw - 12.0 * s;
                                if let Some((g, bid)) = button {
                                    let d = 28.0 * s;
                                    let bx = end - d;
                                    fill(&mut pm, bx, ry - d / 2.0, d, d, d / 2.0, pal.circle);
                                    zones.push(Zone { x: bx, y: ry - d / 2.0, w: d, h: d, radius: d / 2.0, kind: ZoneKind::Click(*bid) });
                                    texts.push(mk(&glyph(*g), Font::Icon, [bx, ry - d / 2.0, d, d], DWRITE_TEXT_ALIGNMENT_CENTER, pal.ink));
                                    end = bx - 6.0 * s;
                                }
                                texts.push(mk(&glyph(*right), Font::Icon, [end - gs, ry - gs / 2.0, gs, gs], DWRITE_TEXT_ALIGNMENT_CENTER, pal.ink));
                                let (tl, tr) = (gx + gs + 6.0 * s, end - gs - 6.0 * s);
                                let v = match self.drag {
                                    Some((d, v)) if d == *sid => v,
                                    _ => value.clamp(0.0, 1.0),
                                };
                                let th = 5.0 * s;
                                fill(&mut pm, tl, ry - th / 2.0, tr - tl, th, th / 2.0, pal.track);
                                fill(&mut pm, tl, ry - th / 2.0, (v * (tr - tl)).max(th), th, th / 2.0, pal.fill);
                                zones.push(Zone { x: tl, y: ry - 12.0 * s, w: tr - tl, h: 24.0 * s, radius: 0.0, kind: ZoneKind::Slider(*sid, tl, tr) });
                            }
                        }
                    }
                }
                Widget::Slider { id: sid, button, value, label } => {
                    let mut base = left + inset;
                    if let Some((g, bid)) = button {
                        let bs = 28.0 * s;
                        let by = y + (wh - bs) / 2.0;
                        zones.push(Zone { x: left, y: by, w: bs, h: bs, radius: 6.0 * s, kind: ZoneKind::Click(*bid) });
                        texts.push(mk(&glyph(*g), Font::Icon, [left, by, bs, bs], DWRITE_TEXT_ALIGNMENT_CENTER, pal.ink));
                        base = left + bs + 4.0 * s;
                    }
                    let kr = 8.0 * s;
                    let label_w = 40.0 * s;
                    let (tl, tr) = (base + kr, right - inset - label_w - 8.0 * s - kr);
                    let v = match self.drag {
                        Some((d, v)) if d == *sid => v,
                        _ => value.clamp(0.0, 1.0),
                    };
                    let cy = y + wh / 2.0;
                    let th = 4.0 * s;
                    let kx = tl + v * (tr - tl);
                    fill(&mut pm, tl, cy - th / 2.0, tr - tl, th, th / 2.0, pal.strong);
                    fill(&mut pm, tl, cy - th / 2.0, kx - tl, th, th / 2.0, pal.accent);
                    if let Some(p) = rounded_rect(kx - kr, cy - kr, 2.0 * kr, 2.0 * kr, kr) {
                        pm.fill_path(&p, &paint([255, 255, 255, 255]), FillRule::Winding, tf, None);
                        let stroke = Stroke { width: 1.0, ..Default::default() };
                        pm.stroke_path(&p, &paint([0, 0, 0, 50]), &stroke, tf, None);
                    }
                    zones.push(Zone {
                        x: tl - kr,
                        y,
                        w: tr - tl + 2.0 * kr,
                        h: wh,
                        radius: 0.0,
                        kind: ZoneKind::Slider(*sid, tl, tr),
                    });
                    texts.push(mk(label, Font::Body, [right - inset - label_w, y, label_w, wh], DWRITE_TEXT_ALIGNMENT_TRAILING, pal.ink));
                }
                Widget::Button { id: bid, text: t, primary } => {
                    let (by, bh) = (y + 4.0 * s, 32.0 * s);
                    fill(&mut pm, left, by, right - left, bh, 6.0 * s, if *primary { pal.accent } else { pal.control });
                    zones.push(Zone { x: left, y: by, w: right - left, h: bh, radius: 6.0 * s, kind: ZoneKind::Click(*bid) });
                    let color = if *primary { WHITE } else { pal.ink };
                    texts.push(mk(t, Font::Body, [left, by, right - left, bh], DWRITE_TEXT_ALIGNMENT_CENTER, color));
                }
                Widget::Separator => {
                    if let Some(r) = Rect::from_xywh(left + inset, y + 4.0 * s, right - left - 2.0 * inset, 1.0) {
                        pm.fill_rect(r, &paint(pal.line), tf, None);
                    }
                }
                Widget::Field { id: fid, placeholder, .. } | Widget::Search { id: fid, placeholder, .. } => {
                    let focused = self.focus == Some(*fid);
                    let (password, big) = match wd {
                        Widget::Field { password, .. } => (*password, false),
                        _ => (false, true),
                    };
                    let (fy, fh, ix, iw, font) = if let Widget::Search { glyph: g, .. } = wd {
                        if let Some(rl) = Rect::from_xywh(0.0, y + wh - 1.0, w, 1.0) {
                            pm.fill_rect(rl, &paint(pal.line), tf, None);
                        }
                        let gx = left + inset;
                        texts.push(mk(&glyph(*g), Font::Icon, [gx, y, 24.0 * s, wh], DWRITE_TEXT_ALIGNMENT_CENTER, pal.dim));
                        let ix = gx + 40.0 * s;
                        (y, wh - 1.0, ix, right - inset - ix, Font::Large)
                    } else {
                        let (fy, fh) = (y + 4.0 * s, 32.0 * s);
                        fill(&mut pm, left, fy, right - left, fh, 6.0 * s, pal.control);
                        if let Some(p) = rounded_rect(left + 0.5, fy + 0.5, right - left - 1.0, fh - 1.0, 6.0 * s) {
                            let stroke = Stroke { width: if focused { 1.5 * s } else { 1.0 }, ..Default::default() };
                            let c = if focused { pal.accent } else { pal.strong };
                            pm.stroke_path(&p, &paint(c), &stroke, tf, None);
                        }
                        (fy, fh, left + inset, right - left - 2.0 * inset, Font::Body)
                    };
                    zones.push(Zone { x: left, y: fy, w: right - left, h: fh, radius: 6.0 * s, kind: ZoneKind::Field(*fid) });
                    let Some(f) = self.fields.iter().find(|f| f.id == *fid) else { continue };
                    let shown: Vec<u16> = if password { vec![0x2022; f.text.len()] } else { f.text.clone() };
                    // Posição do cursor e rolagem para ele ficar visível.
                    let cx = self
                        .layout_text(&shown, font, 10_000.0, fh)
                        .and_then(|l| unsafe {
                            let (mut px, mut py) = (0.0f32, 0.0f32);
                            let mut hm = DWRITE_HIT_TEST_METRICS::default();
                            let (pos, trailing) = if f.cursor == 0 { (0, false) } else { (f.cursor as u32 - 1, true) };
                            l.HitTestTextPosition(pos, trailing, &mut px, &mut py, &mut hm).ok()?;
                            Some(px)
                        })
                        .unwrap_or(0.0);
                    let off = (cx - iw + 2.0 * s).max(0.0);
                    if shown.is_empty() {
                        texts.push(mk(placeholder, font, [ix, fy, iw, fh], DWRITE_TEXT_ALIGNMENT_LEADING, pal.dim));
                    } else {
                        field_ops.push(TextOp {
                            text: shown,
                            font,
                            rect: [ix - off + m, fy + mt, 10_000.0, fh],
                            align: DWRITE_TEXT_ALIGNMENT_LEADING,
                            color: pal.ink,
                            wrap: false,
                            clip: Some([ix + m, fy + mt, ix + iw + m, fy + fh + mt]),
                        });
                    }
                    if focused {
                        let ci = if big { 16.0 } else { 7.0 } * s;
                        caret = Some([ix + cx - off, fy + ci, (1.5 * s).max(1.0), fh - 2.0 * ci]);
                    }
                }
                Widget::Grid { cols, cells } => {
                    let cols = (*cols).max(1);
                    let (cw, ch) = ((right - left) / cols as f32, 32.0 * s);
                    for (i, c) in cells.iter().enumerate() {
                        let cx = left + (i % cols) as f32 * cw;
                        let cy = y + (i / cols) as f32 * ch;
                        let r = (cw.min(ch) / 2.0 - 2.0 * s).max(1.0);
                        let (mx, my) = (cx + cw / 2.0, cy + ch / 2.0);
                        if c.style == CellStyle::Accent {
                            fill(&mut pm, mx - r, my - r, 2.0 * r, 2.0 * r, r, pal.accent);
                        }
                        if let Some(cid) = c.id {
                            zones.push(Zone { x: mx - r, y: my - r, w: 2.0 * r, h: 2.0 * r, radius: r, kind: ZoneKind::Click(cid) });
                        }
                        let (font, color) = match c.style {
                            CellStyle::Header => (Font::Small, pal.dim),
                            CellStyle::Normal => (Font::Body, pal.ink),
                            CellStyle::Dim => (Font::Body, pal.dim),
                            CellStyle::Accent => (Font::Bold, WHITE),
                        };
                        texts.push(mk(&c.text, font, [cx, cy, cw, ch], DWRITE_TEXT_ALIGNMENT_CENTER, color));
                    }
                }
            }
        }

        // Realce do hover por cima das formas (e por baixo do texto).
        if let Some(z) = self.hot.and_then(|i| zones.get(i))
            && matches!(z.kind, ZoneKind::Click(_) | ZoneKind::Toggle(..))
        {
            fill(&mut pm, z.x, z.y, z.w, z.h, z.radius, pal.hover);
        }
        if let Some([x, y, cw, ch]) = caret
            && let Some(r) = Rect::from_xywh(x, y, cw, ch)
        {
            let c = pal.ink.map(|v| (v * 255.0) as u8);
            pm.fill_rect(r, &paint(c), tf, None);
        }
        self.zones = zones;
        texts.append(&mut field_ops);
        drop(pm);

        // Texto e glifos por cima, direto na DIB.
        unsafe {
            if self.target.BindDC(memdc, &RECT { left: 0, top: 0, right: bw, bottom: bh }).is_ok()
                && let Ok(brush) = self.target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }, None)
            {
                let sign = self.dwrite.CreateEllipsisTrimmingSign(self.font(Font::Body)).ok();
                let trim = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, ..Default::default() };
                self.target.BeginDraw();
                for op in &texts {
                    let [x, y, tw, th] = op.rect;
                    let Some(layout) = self.layout_text(&op.text, op.font, tw, th) else { continue };
                    let _ = layout.SetTextAlignment(op.align);
                    if op.wrap {
                        let _ = layout.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
                    } else if op.clip.is_none() {
                        let _ = layout.SetTrimming(&trim, sign.as_ref());
                    }
                    let [r, g, b, a] = op.color;
                    brush.SetColor(&D2D1_COLOR_F { r, g, b, a });
                    if let Some([l, t, r, b]) = op.clip {
                        self.target.PushAxisAlignedClip(&D2D_RECT_F { left: l, top: t, right: r, bottom: b }, D2D1_ANTIALIAS_MODE_ALIASED);
                    }
                    self.target.DrawTextLayout(Vector2 { X: x, Y: y }, &layout, &brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
                    if op.clip.is_some() {
                        self.target.PopAxisAlignedClip();
                    }
                }
                let _ = self.target.EndDraw(None, None);
            }
        }
        self.present();
    }

    /// Põe o bitmap na tela na posição e opacidade atuais da animação.
    fn present(&self) {
        let Some(buf) = &self.buf else { return };
        let (m, mt) = self.margins();
        let dy = (-(1.0 - self.t) * SLIDE * self.scale).round() as i32;
        unsafe {
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: (self.t * 255.0).round() as u8,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let pos = POINT { x: self.x - m as i32, y: self.y - mt as i32 + dy };
            let _ = UpdateLayeredWindow(
                self.hwnd,
                None,
                Some(&pos),
                Some(&SIZE { cx: buf.w, cy: buf.h }),
                Some(buf.memdc),
                Some(&POINT::default()),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            let _ = SetWindowPos(
                self.glass,
                None,
                self.x,
                self.y + dy,
                self.w,
                self.h,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW,
            );
        }
        if let Some(fx) = &self.glass_fx {
            fx.set_opacity(self.t);
        }
    }

    fn animate(&mut self) {
        let p = (self.start.elapsed().as_secs_f32() * 1000.0 / ANIM_MS).min(1.0);
        self.t = 1.0 - (1.0 - p).powi(3);
        self.present();
        if p >= 1.0 {
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_ANIM);
            }
        }
    }

    /// Ponto do mouse (cliente da janela) em coordenadas do painel.
    fn local(&self, lp: LPARAM) -> (f32, f32) {
        let (m, mt) = self.margins();
        let x = (lp.0 & 0xffff) as i16 as f32;
        let y = ((lp.0 >> 16) & 0xffff) as i16 as f32;
        (x - m, y - mt)
    }

    fn inside(&self, x: f32, y: f32) -> bool {
        x >= 0.0 && y >= 0.0 && x < self.w as f32 && y < self.h as f32
    }

    /// Colunas de [`Widget::Icons`] na largura atual.
    fn icon_cols(&self) -> usize {
        ((self.w as f32 / self.scale - 2.0 * SIDE) / ICON_CELL).floor().max(1.0) as usize
    }

    fn hit(&self, x: f32, y: f32) -> Option<usize> {
        self.zones.iter().position(|z| x >= z.x && x < z.x + z.w && y >= z.y && y < z.y + z.h)
    }

    fn hover(&mut self, x: f32, y: f32) {
        self.mouse = Some((x, y));
        if self.content.event(Event::PointerMove) {
            self.rebuild();
            return;
        }
        let hot = self.hit(x, y);
        if hot != self.hot {
            self.hot = hot;
            self.redraw();
        }
    }

    fn emit(&mut self, ev: Event) {
        if self.content.event(ev) {
            self.rebuild();
        } else {
            self.redraw();
        }
    }

    fn slide(&mut self, x: f32) {
        let Some((id, _)) = self.drag else { return };
        let Some((l, r)) = self.zones.iter().find_map(|z| match z.kind {
            ZoneKind::Slider(sid, l, r) if sid == id => Some((l, r)),
            _ => None,
        }) else {
            return;
        };
        let v = ((x - l) / (r - l).max(1.0)).clamp(0.0, 1.0);
        self.drag = Some((id, v));
        self.emit(Event::Slide(id, v));
    }

    fn mouse_down(&mut self, lp: LPARAM) -> bool {
        let (x, y) = self.local(lp);
        if !self.inside(x, y) {
            // Na sombra: conta como clique fora.
            return false;
        }
        match self.hit(x, y).map(|i| (i, self.zones[i].kind)) {
            Some((_, ZoneKind::Slider(id, ..))) => {
                self.drag = Some((id, 0.0));
                unsafe { SetCapture(self.hwnd) };
                self.slide(x);
            }
            Some((_, ZoneKind::Field(id))) => {
                self.focus = Some(id);
                if let Some(f) = self.fields.iter_mut().find(|f| f.id == id) {
                    f.cursor = f.text.len();
                }
                self.redraw();
            }
            Some((i, _)) => {
                self.pressed = Some(i);
                unsafe { SetCapture(self.hwnd) };
            }
            None => {}
        }
        true
    }

    fn right_click(&mut self, lp: LPARAM) {
        let (x, y) = self.local(lp);
        if let Some(ZoneKind::Click(id)) = self.hit(x, y).map(|i| self.zones[i].kind) {
            self.emit(Event::RightClick(id));
        }
    }

    fn mouse_up(&mut self, lp: LPARAM) {
        let (x, y) = self.local(lp);
        unsafe {
            let _ = ReleaseCapture();
        }
        if self.drag.take().is_some() {
            self.rebuild();
            return;
        }
        let Some(i) = self.pressed.take() else { return };
        if self.hit(x, y) != Some(i) {
            return;
        }
        match self.zones[i].kind {
            ZoneKind::Click(id) => self.emit(Event::Click(id)),
            ZoneKind::Toggle(id, on) => self.emit(Event::Toggle(id, !on)),
            _ => {}
        }
    }

    /// Edição do campo em foco; `true` se o texto mudou.
    fn edit(&mut self, f: impl FnOnce(&mut Field) -> bool) {
        let Some(id) = self.focus else { return };
        let Some(field) = self.fields.iter_mut().find(|x| x.id == id) else { return };
        if f(field) {
            let text = String::from_utf16_lossy(&field.text);
            self.emit(Event::Text(id, &text));
        } else {
            self.redraw();
        }
    }

    fn key_down(&mut self, vk: u16) {
        match vk {
            v if v == VK_LEFT.0 => self.edit(|f| {
                f.cursor = f.cursor.saturating_sub(1);
                false
            }),
            v if v == VK_RIGHT.0 => self.edit(|f| {
                f.cursor = (f.cursor + 1).min(f.text.len());
                false
            }),
            v if v == VK_HOME.0 => self.edit(|f| {
                f.cursor = 0;
                false
            }),
            v if v == VK_END.0 => self.edit(|f| {
                f.cursor = f.text.len();
                false
            }),
            v if v == VK_UP.0 || v == VK_DOWN.0 => self.emit(Event::Key(v)),
            v if v == VK_DELETE.0 => self.edit(|f| {
                if f.cursor < f.text.len() {
                    f.text.remove(f.cursor);
                    true
                } else {
                    false
                }
            }),
            _ => {}
        }
    }

    fn char(&mut self, c: u16) {
        match c {
            0x08 => self.edit(|f| {
                if f.cursor > 0 {
                    f.cursor -= 1;
                    f.text.remove(f.cursor);
                    true
                } else {
                    false
                }
            }),
            0x0D => {
                let Some(id) = self.focus else {
                    self.emit(Event::Key(VK_RETURN.0));
                    return;
                };
                let Some(field) = self.fields.iter().find(|x| x.id == id) else { return };
                let text = String::from_utf16_lossy(&field.text);
                self.emit(Event::Submit(id, &text));
            }
            c if c < 0x20 || c == 0x7F => {}
            c => self.edit(|f| {
                f.text.insert(f.cursor, c);
                f.cursor += 1;
                true
            }),
        }
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        // Composição antes da janela dela.
        self.glass_fx = None;
        OPEN_HWND.with(|h| {
            if h.get() == self.hwnd.0 as isize {
                h.set(0);
            }
        });
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            let _ = DestroyWindow(self.glass);
        }
    }
}

fn with_panel<R>(f: impl FnOnce(&mut Panel) -> R) -> Option<R> {
    // try_borrow_mut: chamadas do Windows podem reentrar no wndproc.
    PANEL.with(|p| p.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

/// Tira o painel do estado (para destruir fora do borrow). `Err` se estiver emprestado.
fn take() -> Result<Option<Panel>, ()> {
    PANEL.with(|p| p.try_borrow_mut().map(|mut g| g.take()).map_err(|_| ()))
}

fn close_now(restore: bool) {
    if let Ok(Some(p)) = take() {
        unsafe {
            if restore && GetForegroundWindow() == p.hwnd && IsWindow(Some(p.prev_fg)).as_bool() {
                let _ = SetForegroundWindow(p.prev_fg);
            }
        }
        drop(p);
        crate::topbar::panel_visibility_changed();
    }
}

/// Abre o painel `key` sob o item `item` (retângulo do item na tela). Se ele já está aberto,
/// fecha; se outro está aberto, troca. `make` só é chamado quando o painel vai abrir.
pub fn toggle(key: &'static str, item: RECT, make: impl FnOnce() -> Box<dyn PanelContent>) {
    let Ok(old) = take() else { return };
    if let Some(old) = old {
        // Mesmo painel noutra barra (outro monitor): troca de lugar em vez de fechar.
        if old.key == key && old.item == item {
            PANEL.with(|p| *p.borrow_mut() = Some(old));
            close_now(true);
            return;
        }
        // Abre o novo antes de destruir o antigo, para não perder o primeiro plano.
        let prev = old.prev_fg;
        open(key, item, make(), prev);
        drop(old);
        return;
    }
    open(key, item, make(), unsafe { GetForegroundWindow() });
}

fn open(key: &'static str, item: RECT, content: Box<dyn PanelContent>, prev_fg: HWND) {
    let Ok(mut p) = Panel::new(key, item, content, prev_fg) else { return };
    p.rebuild();
    let hwnd = p.hwnd;
    unsafe {
        let _ = ShowWindow(p.glass, SW_SHOWNOACTIVATE);
        let _ = ShowWindow(hwnd, SW_SHOW);
        SetTimer(Some(hwnd), TIMER_ANIM, 8, None);
    }
    p.start = Instant::now();
    OPEN_HWND.with(|h| h.set(hwnd.0 as isize));
    PANEL.with(|c| *c.borrow_mut() = Some(p));
    // Ativa para receber o teclado; fecha ao perder a ativação (WM_ACTIVATE).
    unsafe {
        let _ = SetForegroundWindow(hwnd);
    }
    crate::topbar::panel_visibility_changed();
}

/// Fecha o painel aberto, se houver. Seguro de chamar de dentro de [`PanelContent::event`].
pub fn close() {
    match take() {
        Ok(Some(p)) => {
            PANEL.with(|c| *c.borrow_mut() = Some(p));
            close_now(true);
        }
        Ok(None) => {}
        Err(()) => {
            let h = OPEN_HWND.with(|h| h.get());
            if h != 0 {
                unsafe {
                    let _ = PostMessageW(Some(HWND(h as *mut c_void)), WM_PANEL_CLOSE, WPARAM(0), LPARAM(1));
                }
            }
        }
    }
}

/// Se o painel `key` está aberto.
#[allow(dead_code)]
pub fn is_open(key: &str) -> bool {
    with_panel(|p| p.key == key).unwrap_or(false)
}

/// Whether this item on this monitor owns the currently open popover.
pub fn is_open_at(key: &str, item: RECT) -> bool {
    with_panel(|p| p.key == key && p.item == item).unwrap_or(false)
}

/// Se há algum painel aberto.
pub fn any_open() -> bool {
    with_panel(|_| ()).is_some()
}

/// Reconstrói o painel aberto (estado mudou por fora).
#[allow(dead_code)]
pub fn refresh() {
    with_panel(|p| p.rebuild());
}

/// Rebuild an open panel after a Windows UI-language change.
pub fn language_changed() {
    refresh();
}

/// Repassa uma mensagem ao conteúdo aberto ([`PanelContent::notify`]).
pub fn notify(msg: u32, wparam: usize) {
    with_panel(|p| {
        if p.content.notify(msg, wparam) {
            p.rebuild();
        }
    });
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        // Só a janela do painel aberto reage (a antiga, na troca, ainda recebe mensagens).
        if OPEN_HWND.with(|h| h.get()) != hwnd.0 as isize {
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        match msg {
            WM_MOUSEMOVE => {
                with_panel(|p| {
                    if !p.tracking {
                        p.tracking = true;
                        let mut tme = TRACKMOUSEEVENT {
                            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        };
                        let _ = TrackMouseEvent(&mut tme);
                    }
                    let (x, y) = p.local(lp);
                    if p.drag.is_some() {
                        p.slide(x);
                    } else {
                        p.hover(x, y);
                    }
                });
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                with_panel(|p| {
                    p.tracking = false;
                    p.mouse = None;
                    if p.hot.take().is_some() {
                        p.redraw();
                    }
                });
                LRESULT(0)
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                if with_panel(|p| p.mouse_down(lp)) == Some(false) {
                    close_now(true);
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                with_panel(|p| p.mouse_up(lp));
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                with_panel(|p| p.right_click(lp));
                LRESULT(0)
            }
            WM_KEYDOWN => {
                if wp.0 as u16 == VK_ESCAPE.0 {
                    close_now(true);
                } else {
                    with_panel(|p| p.key_down(wp.0 as u16));
                }
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let up = (wp.0 >> 16) as i16 > 0;
                with_panel(|p| p.emit(Event::Key(if up { VK_UP.0 } else { VK_DOWN.0 })));
                LRESULT(0)
            }
            WM_CHAR => {
                with_panel(|p| p.char(wp.0 as u16));
                LRESULT(0)
            }
            WM_ACTIVATE => {
                // Perdeu a ativação (clique fora, Alt+Tab...): fecha depois, fora daqui.
                if (wp.0 & 0xffff) as u32 == WA_INACTIVE {
                    let _ = PostMessageW(Some(hwnd), WM_PANEL_CLOSE, WPARAM(0), LPARAM(0));
                }
                LRESULT(0)
            }
            WM_PANEL_CLOSE => {
                close_now(lp.0 != 0);
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_ANIM => {
                with_panel(|p| p.animate());
                LRESULT(0)
            }
            WM_SETTINGCHANGE => {
                // Pode ser a troca de tema claro/escuro.
                with_panel(|p| p.redraw());
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
