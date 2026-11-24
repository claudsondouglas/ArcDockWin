//! Opções da janela de Configurações: `%APPDATA%\ArcDockWin\settings.txt` com linhas
//! `chave=valor`, lido uma vez e guardado em memória. Sem o arquivo (ou sem a chave), vale o
//! padrão, que é o comportamento de antes das Configurações.

use std::collections::HashMap;
use std::sync::Mutex;

static CACHE: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn path() -> std::path::PathBuf {
    crate::apps::config_path().with_file_name("settings.txt")
}

fn get(key: &str) -> Option<String> {
    let mut cache = CACHE.lock().ok()?;
    let map = cache.get_or_insert_with(|| {
        std::fs::read_to_string(path())
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect()
    });
    map.get(key).cloned()
}

fn set(key: &str, value: String) {
    get(key); // carrega o arquivo antes de mexer
    let Ok(mut cache) = CACHE.lock() else { return };
    let Some(map) = cache.as_mut() else { return };
    map.insert(key.to_string(), value);
    let mut lines: Vec<String> = map.iter().map(|(k, v)| format!("{k}={v}")).collect();
    lines.sort();
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(p, lines.join("\n") + "\n");
}

fn flag(key: &str) -> bool {
    get(key).is_none_or(|v| v != "0")
}

fn set_flag(key: &str, on: bool) {
    set(key, (on as u8).to_string());
}

fn number(key: &str, default: f32, min: f32, max: f32) -> f32 {
    get(key).and_then(|v| v.parse().ok()).unwrap_or(default).clamp(min, max)
}

/// Tamanho dos ícones do dock (px antes da escala do monitor): 32 a 72, padrão 48.
pub fn icon_size() -> f32 {
    number("icon_size", 48.0, 32.0, 72.0).round()
}

pub fn set_icon_size(px: f32) {
    set("icon_size", px.round().to_string());
}

/// Ampliação ao passar o mouse.
pub fn magnify() -> bool {
    flag("magnify")
}

pub fn set_magnify(on: bool) {
    set_flag("magnify", on);
}

/// Tamanho da ampliação: 1,1× a 2,0×, padrão 1,35×.
pub fn magnify_scale() -> f32 {
    number("magnify_scale", 1.35, 1.1, 2.0)
}

pub fn set_magnify_scale(v: f32) {
    set("magnify_scale", format!("{v:.2}"));
}

/// Tecla Windows sozinha abre o lançador em vez do menu Iniciar.
pub fn win_key_launcher() -> bool {
    flag("win_key_launcher")
}

pub fn set_win_key_launcher(on: bool) {
    set_flag("win_key_launcher", on);
}

/// Esconder o dock quando uma janela encosta nele.
pub fn autohide() -> bool {
    flag("autohide")
}

pub fn set_autohide(on: bool) {
    set_flag("autohide", on);
}

/// Downloads e Lixeira na ponta direita do dock.
pub fn stacks() -> bool {
    flag("stacks")
}

pub fn set_stacks(on: bool) {
    set_flag("stacks", on);
}

/// Prévia das janelas ao parar o mouse num app com 2+ janelas.
pub fn window_previews() -> bool {
    flag("window_previews")
}

pub fn set_window_previews(on: bool) {
    set_flag("window_previews", on);
}

/// Dock em todos os monitores (padrão: desligado, só no principal).
pub fn dock_all_monitors() -> bool {
    get("dock_all_monitors").is_some_and(|v| v == "1")
}

pub fn set_dock_all_monitors(on: bool) {
    set_flag("dock_all_monitors", on);
}

/// Relógio em 24 horas (senão 12 h com AM/PM).
pub fn clock_24h() -> bool {
    flag("clock_24h")
}

pub fn set_clock_24h(on: bool) {
    set_flag("clock_24h", on);
}

/// Ícones da bandeja com as cores do app (`tray_color=1`); o padrão é tons de cinza.
pub fn tray_color() -> bool {
    get("tray_color").is_some_and(|v| v == "1")
}

/// Segundos no relógio (desligado por padrão: atualiza a cada minuto).
pub fn clock_seconds() -> bool {
    get("clock_seconds").is_some_and(|v| v == "1")
}

pub fn set_clock_seconds(on: bool) {
    set_flag("clock_seconds", on);
}

/// Data junto da hora.
pub fn clock_date() -> bool {
    flag("clock_date")
}

pub fn set_clock_date(on: bool) {
    set_flag("clock_date", on);
}

/// Barra do topo em todos os monitores (senão só no principal).
pub fn bar_all_monitors() -> bool {
    flag("bar_all_monitors")
}

pub fn set_bar_all_monitors(on: bool) {
    set_flag("bar_all_monitors", on);
}

/// Módulo da barra do topo. Módulo novo: um variant aqui (com chave, nome e posição padrão),
/// uma função de layout em `topbar.rs` e ele já aparece na lista das Configurações.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Module {
    Stats,
    Clock,
    /// Ícones da bandeja e a seta de ocultos, sempre juntos.
    Tray,
    Control,
    /// Uso do plano do Claude (sessão de 5 h); desligado por padrão.
    Claude,
    /// Uso do plano do Codex (sessão de 5 h); desligado por padrão.
    Codex,
}

/// Zona da barra: a ordem dos módulos vale dentro de cada uma.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Zone {
    Left,
    Center,
    Right,
}

impl Zone {
    pub const ALL: [Zone; 3] = [Zone::Left, Zone::Center, Zone::Right];

    /// Rótulo em pt-BR (passa por `i18n::tr` na interface).
    pub fn label(self) -> &'static str {
        match self {
            Zone::Left => "Esquerda",
            Zone::Center => "Centro",
            Zone::Right => "Direita",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Zone::Left => "left",
            Zone::Center => "center",
            Zone::Right => "right",
        }
    }
}

impl Module {
    pub const ALL: [Module; 6] = [Module::Stats, Module::Clock, Module::Tray, Module::Control, Module::Claude, Module::Codex];

    fn key(self) -> &'static str {
        match self {
            Module::Stats => "stats",
            Module::Clock => "clock",
            Module::Tray => "tray",
            Module::Control => "control",
            Module::Claude => "claude",
            Module::Codex => "codex",
        }
    }

    /// Nome em pt-BR (passa por `i18n::tr`) e versão curta para a prévia.
    pub fn label(self) -> &'static str {
        match self {
            Module::Stats => "CPU e RAM",
            Module::Clock => "Relógio",
            Module::Tray => "Bandeja",
            Module::Control => "Central de Controle",
            Module::Claude => "Uso do Claude",
            Module::Codex => "Uso do Codex",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Module::Stats => "CPU/RAM",
            Module::Control => "Central",
            Module::Claude => "Claude",
            Module::Codex => "Codex",
            m => m.label(),
        }
    }

    /// Zona e ordem padrão: o layout de antes dos módulos.
    fn default_slot(self) -> (Zone, u32) {
        match self {
            Module::Stats => (Zone::Left, 0),
            Module::Clock => (Zone::Center, 0),
            Module::Tray => (Zone::Right, 0),
            Module::Control => (Zone::Right, 1),
            Module::Claude | Module::Codex => (Zone::Right, 0),
        }
    }
}

/// Módulo com a posição e se aparece.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Slot {
    pub module: Module,
    pub zone: Zone,
    pub on: bool,
}

/// `mod.<chave>=<zona>,<ordem>,<on|off>` (ex.: `mod.clock=center,0,on`); partes inválidas
/// caem no padrão. O antigo `stats=0` vale enquanto `mod.stats` não existir.
fn parse_slot(m: Module, value: Option<&str>, legacy_on: bool) -> (Zone, u32, bool) {
    let (dz, dorder) = m.default_slot();
    let Some(v) = value else { return (dz, dorder, legacy_on) };
    let mut parts = v.split(',').map(str::trim);
    let zone = parts.next().and_then(|z| Zone::ALL.into_iter().find(|x| x.key() == z)).unwrap_or(dz);
    let order = parts.next().and_then(|o| o.parse().ok()).unwrap_or(dorder);
    let on = parts.next().map_or(legacy_on, |o| o != "off");
    (zone, order, on)
}

/// Módulos na ordem visual (zona esquerda, centro e direita; cada uma pela ordem).
pub fn modules() -> Vec<Slot> {
    let legacy = flag("stats");
    let mut list: Vec<(Zone, u32, usize, Slot)> = Module::ALL
        .into_iter()
        .enumerate()
        .map(|(i, m)| {
            let legacy_on = match m {
                Module::Stats => legacy,
                Module::Claude | Module::Codex => false,
                _ => true,
            };
            let (zone, order, on) = parse_slot(m, get(&format!("mod.{}", m.key())).as_deref(), legacy_on);
            (zone, order, i, Slot { module: m, zone, on })
        })
        .collect();
    list.sort_by_key(|&(z, o, i, _)| (z, o, i));
    list.into_iter().map(|t| t.3).collect()
}

fn save_modules(list: &[Slot]) {
    for zone in Zone::ALL {
        for (order, s) in list.iter().filter(|s| s.zone == zone).enumerate() {
            let on = if s.on { "on" } else { "off" };
            set(&format!("mod.{}", s.module.key()), format!("{},{order},{on}", zone.key()));
        }
    }
}

pub fn module_on(m: Module) -> bool {
    modules().iter().any(|s| s.module == m && s.on)
}

pub fn set_module_on(m: Module, on: bool) {
    let mut list = modules();
    list.iter_mut().filter(|s| s.module == m).for_each(|s| s.on = on);
    save_modules(&list);
}

/// Leva o módulo para o fim de outra zona.
pub fn set_module_zone(m: Module, zone: Zone) {
    let mut list = modules();
    let Some(i) = list.iter().position(|s| s.module == m && s.zone != zone) else { return };
    let mut s = list.remove(i);
    s.zone = zone;
    let at = list.iter().rposition(|x| x.zone <= zone).map_or(0, |p| p + 1);
    list.insert(at, s);
    save_modules(&list);
}

/// Troca o módulo de lugar com o vizinho da mesma zona (`dir` -1 sobe, +1 desce).
pub fn move_module(m: Module, dir: i32) {
    let mut list = modules();
    let Some(i) = list.iter().position(|s| s.module == m) else { return };
    let j = i as i32 + dir;
    if j >= 0 && (j as usize) < list.len() && list[j as usize].zone == list[i].zone {
        list.swap(i, j as usize);
        save_modules(&list);
    }
}

/// Iniciar com o Windows.
pub fn autostart() -> bool {
    flag("autostart")
}

pub fn set_autostart(on: bool) {
    set_flag("autostart", on);
}

/// Procurar atualizações no GitHub sozinho (instalar só com um clique).
pub fn auto_update() -> bool {
    flag("auto_update")
}

pub fn set_auto_update(on: bool) {
    set_flag("auto_update", on);
}
