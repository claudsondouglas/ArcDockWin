//! Apps do dock: fixados (configuração) e abertos, ícones e ações de clique.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tiny_skia::Pixmap;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, SIZE, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits,
    GetObjectW, HBITMAP, HGDIOBJ, ReleaseDC,
};
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GWL_EXSTYLE, GetClassNameW, GetForegroundWindow, GetWindow,
    GetWindowLongW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    PostMessageW, SC_MINIMIZE, SW_MINIMIZE, SW_SHOWNORMAL, SetForegroundWindow, ShowWindow,
    SwitchToThisWindow, WM_SYSCOMMAND, WS_EX_TOOLWINDOW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_LWIN,
    VK_X,
};
use windows::core::{BOOL, HSTRING, PWSTR, w};
use windows::core::{Interface, PCSTR};
use windows::Win32::Foundation::{LRESULT, POINT};
use windows::Win32::UI::Shell::{
    BHID_SFUIObject, CMF_NORMAL, CMINVOKECOMMANDINFO, IContextMenu, IContextMenu3, IShellItem,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, TRACK_POPUP_MENU_FLAGS, TrackPopupMenu, WM_DRAWITEM, WM_INITMENUPOPUP,
    WM_MEASUREITEM, WM_MENUCHAR,
};

pub const APPS_FOLDER: &str = "shell:AppsFolder\\";

pub struct App {
    /// Caminho do .exe ou `shell:AppsFolder\<id>` (apps da Store).
    pub source: String,
    /// Identifica o app ao casar com as janelas abertas.
    pub key: String,
    pub pinned: bool,
    pub icon: Option<Pixmap>,
    /// Janelas abertas do app, na ordem em que apareceram.
    pub windows: Vec<HWND>,
    /// Separador depois deste app (linha `---` no pinned.txt; só nos fixados).
    pub sep_after: bool,
}

/// Linha do pinned.txt que põe um separador depois do app de cima.
pub const SEP_LINE: &str = "---";

const DEFAULT_CONFIG: &str = "\
# ArcDockWin - apps fixados no dock
# Um caminho de .exe por linha (ou shell:AppsFolder\\<id> para apps da Store).
# Linhas começando com # são ignoradas. Uma linha --- põe um separador depois do app de cima.
C:\\Windows\\explorer.exe
C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe
C:\\Windows\\System32\\notepad.exe
";

pub fn config_path() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();
    base.join("ArcDockWin").join("pinned.txt")
}

pub fn load_pinned(icon_size: i32) -> Vec<App> {
    let cfg = config_path();
    if !cfg.exists() {
        if let Some(dir) = cfg.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&cfg, DEFAULT_CONFIG);
    }
    let text = std::fs::read_to_string(&cfg).unwrap_or_else(|_| DEFAULT_CONFIG.to_string());

    let start = App {
        source: START.to_string(),
        key: START.to_string(),
        pinned: true,
        icon: load_icon(START, icon_size),
        windows: Vec::new(),
        sep_after: false,
    };
    // O botão Iniciar é sempre o primeiro item, antes dos fixados e dos abertos.
    let mut apps: Vec<App> = vec![start];
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        if line == SEP_LINE {
            // O Iniciar já tem separador depois dele.
            if let Some(last) = apps.last_mut().filter(|a| a.source != START) {
                last.sep_after = true;
            }
            continue;
        }
        if !(line.starts_with(APPS_FOLDER) || Path::new(line).exists()) {
            continue;
        }
        apps.push(App {
            key: source_key(line),
            icon: load_icon(line, icon_size),
            source: line.to_string(),
            pinned: true,
            windows: Vec::new(),
            sep_after: false,
        });
    }
    apps
}

/// Grava os fixados na ordem do dock: comentários e linhas de apps que não estão no dock
/// ficam como estão, os fixados (com os separadores `---` depois deles) vão para o fim e
/// `gone` (desafixado) sai.
pub fn save_pinned(list: &[App], gone: Option<&str>) {
    let pinned: Vec<&App> = list.iter().filter(|a| a.pinned && a.source != START).collect();
    let cfg = config_path();
    let text = std::fs::read_to_string(&cfg).unwrap_or_default();
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = String::new();
    for line in text.lines() {
        let l = line.trim();
        if l != SEP_LINE && !pinned.iter().any(|a| a.source == l) && gone != Some(l) {
            out.push_str(line);
            out.push_str(nl);
        }
    }
    for a in pinned {
        out.push_str(&a.source);
        out.push_str(nl);
        if a.sep_after {
            out.push_str(SEP_LINE);
            out.push_str(nl);
        }
    }
    if let Some(dir) = cfg.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&cfg, out);
}

/// Põe ou tira o separador depois do app fixado `key` e grava.
pub fn toggle_separator(apps: &mut [App], key: &str) {
    let Some(app) = apps.iter_mut().find(|a| a.key == key && a.pinned && a.source != START) else { return };
    app.sep_after = !app.sep_after;
    save_pinned(apps, None);
}

/// Item especial do botão Iniciar.
pub const START: &str = "::start";

/// Nome do app para o balão do dock: descrição do .exe ("Brave Browser") ou nome do app da Store.
pub fn display_name(source: &str) -> String {
    thread_local! {
        static CACHE: std::cell::RefCell<HashMap<String, String>> = std::cell::RefCell::new(HashMap::new());
    }
    match source {
        // O botão abre o lançador ou o menu Iniciar do Windows, conforme a opção.
        START if crate::config::win_key_launcher() => return crate::i18n::tr("Lançador"),
        START => return crate::i18n::tr("Iniciar"),
        crate::stacks::DOWNLOADS => return crate::i18n::tr("Downloads"),
        crate::stacks::TRASH => return crate::i18n::tr("Lixeira"),
        _ => {}
    }
    if let Some(name) = CACHE.with_borrow(|c| c.get(source).cloned()) {
        return name;
    }
    let name = shell_name(source).unwrap_or_else(|| {
        Path::new(source).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| source.to_string())
    });
    CACHE.with_borrow_mut(|c| c.insert(source.to_string(), name.clone()));
    name
}

fn shell_name(source: &str) -> Option<String> {
    use windows::Win32::Storage::EnhancedStorage::PKEY_FileDescription;
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{IShellItem2, SIGDN_NORMALDISPLAY};
    let take = |p: PWSTR| unsafe {
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    };
    let parse = store_app_id(source).map(|id| format!("{APPS_FOLDER}{id}")).unwrap_or_else(|| source.to_string());
    unsafe {
        let item: IShellItem2 = SHCreateItemFromParsingName(&HSTRING::from(&parse), None).ok()?;
        if !parse.starts_with(APPS_FOLDER)
            && let Some(desc) = item.GetString(&PKEY_FileDescription).ok().and_then(take)
        {
            return Some(desc);
        }
        item.GetDisplayName(SIGDN_NORMALDISPLAY).ok().and_then(take)
    }
}

/// Logo do Windows: quatro quadrados com degradê azul.
fn start_icon(size: u32) -> Option<Pixmap> {
    use tiny_skia::{GradientStop, LinearGradient, Paint, Point, Rect, SpreadMode, Transform};
    let mut pm = Pixmap::new(size, size)?;
    let s = size as f32;
    let gap = s * 0.04;
    let half = (s - gap) / 2.0;
    let shader = LinearGradient::new(
        Point::from_xy(0.0, 0.0),
        Point::from_xy(s, s),
        vec![
            GradientStop::new(0.0, tiny_skia::Color::from_rgba8(0x2A, 0xC0, 0xFA, 255)),
            GradientStop::new(1.0, tiny_skia::Color::from_rgba8(0x00, 0x5F, 0xC4, 255)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )?;
    let paint = Paint { shader, anti_alias: true, ..Default::default() };
    for (x, y) in [(0.0, 0.0), (half + gap, 0.0), (0.0, half + gap), (half + gap, half + gap)] {
        if let Some(r) = Rect::from_xywh(x, y, half, half) {
            pm.fill_rect(r, &paint, Transform::identity(), None);
        }
    }
    Some(pm)
}

fn source_key(source: &str) -> String {
    match source.strip_prefix(APPS_FOLDER) {
        Some(id) => format!("aumid:{}", id.to_lowercase()),
        None => match packaged_aumid(source) {
            Some(id) => format!("aumid:{}", id.to_lowercase()),
            None => exe_key(source),
        },
    }
}

/// AUMID do app empacotado (Store) dono deste .exe, com cache por caminho: o mesmo app
/// fixado pelo AUMID ou pelo .exe, e as janelas dele, ficam com a mesma chave.
fn packaged_aumid(path: &str) -> Option<String> {
    thread_local! {
        static CACHE: std::cell::RefCell<HashMap<String, Option<String>>> = std::cell::RefCell::new(HashMap::new());
    }
    let lower = path.to_lowercase();
    if !lower.contains("\\windowsapps\\") || lower.contains("\\microsoft\\windowsapps\\") {
        return None;
    }
    CACHE.with(|c| {
        c.borrow_mut()
            .entry(lower)
            .or_insert_with(|| store_app_id(path).and_then(|s| s.strip_prefix(APPS_FOLDER).map(str::to_string)))
            .clone()
    })
}

fn exe_key(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Ícones trocados por fora (`nome.exe=C:\caminho\icone.ico`), escrito pelo IconesTahoe.
pub fn icons_path() -> PathBuf {
    config_path().with_file_name("icons.txt")
}

fn icon_override(source: &str) -> Option<String> {
    let key = if source == START { "start".to_string() } else { exe_key(source) };
    std::fs::read_to_string(icons_path())
        .ok()?
        .lines()
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case(&key))
        .map(|(_, v)| v.trim().to_string())
        .filter(|p| Path::new(p).exists())
}

/// Recarrega os ícones de todos os apps (o arquivo de ícones trocados mudou).
pub fn reload_icons(apps: &mut [App], size: i32) {
    for app in apps.iter_mut() {
        app.icon = load_icon(&app.source, size);
    }
}

/// Ícone em alta resolução, como pixmap RGBA pré-multiplicado.
pub fn load_icon(source: &str, size: i32) -> Option<Pixmap> {
    let path = icon_override(source);
    if source == START && path.is_none() {
        return start_icon(size as u32);
    }
    let source = path.unwrap_or_else(|| source.to_string());
    unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(&HSTRING::from(source), None).ok()?;
        let hbmp = factory
            .GetImage(SIZE { cx: size, cy: size }, SIIGBF_ICONONLY)
            .ok()?;
        let pixmap = hbitmap_to_pixmap(hbmp);
        let _ = DeleteObject(HGDIOBJ(hbmp.0));
        pixmap.and_then(trim_transparent)
    }
}

/// Corta as margens transparentes do ícone, para todos ocuparem a caixa por igual.
fn trim_transparent(pm: Pixmap) -> Option<Pixmap> {
    let (w, h) = (pm.width(), pm.height());
    let alpha = |x: u32, y: u32| pm.data()[((y * w + x) * 4 + 3) as usize];
    let (mut l, mut t, mut r, mut b) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if alpha(x, y) > 8 {
                l = l.min(x);
                t = t.min(y);
                r = r.max(x + 1);
                b = b.max(y + 1);
            }
        }
    }
    if l >= r || t >= b {
        return Some(pm);
    }
    let rect = tiny_skia::IntRect::from_ltrb(l as i32, t as i32, r as i32, b as i32)?;
    pm.clone_rect(rect)
}

unsafe fn hbitmap_to_pixmap(hbmp: HBITMAP) -> Option<Pixmap> {
    unsafe {
        let mut bm = BITMAP::default();
        if GetObjectW(
            HGDIOBJ(hbmp.0),
            size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut _),
        ) == 0
        {
            return None;
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // de cima para baixo
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let hdc = GetDC(None);
        let lines = GetDIBits(
            hdc,
            hbmp,
            0,
            h as u32,
            Some(buf.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, hdc);
        if lines == 0 {
            return None;
        }

        // BGRA -> RGBA. Se algum canal passar do alfa, a imagem não está pré-multiplicada.
        let straight = buf
            .chunks_exact(4)
            .any(|p| p[0] > p[3] || p[1] > p[3] || p[2] > p[3]);
        for p in buf.chunks_exact_mut(4) {
            p.swap(0, 2);
            if straight {
                let a = p[3] as u16;
                for c in &mut p[..3] {
                    *c = ((*c as u16 * a + 127) / 255) as u8;
                }
            }
        }
        Pixmap::from_vec(buf, tiny_skia::IntSize::from_wh(w as u32, h as u32)?)
    }
}

/// Atualiza as janelas de cada app e a lista de apps abertos não fixados.
/// Retorna true se algo mudou.
pub fn refresh(apps: &mut Vec<App>, icon_size: i32) -> bool {
    let mut hwnds: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut hwnds as *mut _ as isize));
    }

    let mut procs: HashMap<u32, String> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, (String, Vec<HWND>)> = HashMap::new();
    for hwnd in hwnds {
        let Some((key, source)) = window_app(hwnd, &mut procs) else { continue };
        groups
            .entry(key.clone())
            .or_insert_with(|| {
                order.push(key);
                (source, Vec::new())
            })
            .1
            .push(hwnd);
    }

    let mut changed = false;
    for app in apps.iter_mut() {
        let found = groups.remove(&app.key).map(|g| g.1).unwrap_or_default();
        // Ordem estável: as que já estavam ficam no lugar, as novas vão para o fim.
        let mut new: Vec<HWND> = app.windows.iter().copied().filter(|h| found.contains(h)).collect();
        new.extend(found.into_iter().filter(|h| !app.windows.contains(h)));
        if new != app.windows {
            changed = true;
            app.windows = new;
        }
    }

    let before = apps.len();
    apps.retain(|a| a.pinned || !a.windows.is_empty());
    changed |= apps.len() != before;

    for key in order {
        if let Some((source, windows)) = groups.remove(&key) {
            let icon = load_icon(&source, icon_size);
            // Abertos não fixados vão para o fim (o Iniciar é sempre o primeiro).
            apps.push(App {
                icon,
                source,
                key,
                pinned: false,
                windows,
                sep_after: false,
            });
            changed = true;
        }
    }
    changed
}

/// (key, source) do app dono da janela.
fn window_app(hwnd: HWND, procs: &mut HashMap<u32, String>) -> Option<(String, String)> {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let path = procs.entry(pid).or_insert_with(|| host_path(pid)).clone();
    let key = exe_key(&path);
    if key.is_empty() {
        return None;
    }
    if key == "applicationframehost.exe" {
        // Apps da Store rodam dentro do ApplicationFrameHost; o app real vem do AppUserModelID.
        let id = window_aumid(hwnd)?;
        return Some((format!("aumid:{}", id.to_lowercase()), format!("{APPS_FOLDER}{id}")));
    }
    // App da Store com .exe próprio (Claude, Slack, WhatsApp): chave pelo AUMID, a mesma do
    // fixado por `shell:AppsFolder\...`.
    if let Some(id) = packaged_aumid(&path) {
        return Some((format!("aumid:{}", id.to_lowercase()), format!("{APPS_FOLDER}{id}")));
    }
    Some((key, path))
}

fn window_aumid(hwnd: HWND) -> Option<String> {
    unsafe {
        let store: IPropertyStore = SHGetPropertyStoreForWindow(hwnd).ok()?;
        let value = store.GetValue(&PKEY_AppUserModel_ID).ok()?;
        let id = value.to_string();
        (!id.is_empty()).then_some(id)
    }
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
    unsafe {
        if is_app_window(hwnd) {
            (*(lparam.0 as *mut Vec<HWND>)).push(hwnd);
        }
    }
    BOOL(1)
}

/// Janelas que apareceriam na barra de tarefas.
unsafe fn is_app_window(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) == 0 {
            return false;
        }
        if GetWindow(hwnd, GW_OWNER).is_ok_and(|o| !o.is_invalid()) {
            return false;
        }
        // As janelas do próprio dock (topbar, painéis...) não são apps; a de Configurações é.
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == windows::Win32::System::Threading::GetCurrentProcessId() && hwnd != crate::settings::hwnd() {
            return false;
        }
        if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
            return false;
        }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as *mut _,
            size_of::<u32>() as u32,
        );
        if cloaked != 0 {
            return false;
        }
        let mut class = [0u16; 64];
        let len = GetClassNameW(hwnd, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..len]);
        !matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd")
    }
}

/// Caminho do .exe dono do processo. Janelas do WebView2 (msedgewebview2.exe) ficam com o app
/// que hospeda o WebView2 (ex.: WhatsApp.Root.exe), para o dock mostrar o ícone certo.
fn host_path(pid: u32) -> String {
    let path = process_path(pid);
    if exe_key(&path) != "msedgewebview2.exe" {
        return path;
    }
    let parents = parent_pids();
    let mut cur = pid;
    // Sobe a árvore: renderizador -> navegador do WebView2 -> app.
    for _ in 0..4 {
        let Some(&parent) = parents.get(&cur) else { break };
        let p = process_path(parent);
        if p.is_empty() {
            break;
        }
        if exe_key(&p) != "msedgewebview2.exe" {
            return p;
        }
        cur = parent;
    }
    path
}

/// pid -> pid do pai, de todos os processos.
fn parent_pids() -> HashMap<u32, u32> {
    let mut map = HashMap::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return map };
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            map.insert(e.th32ProcessID, e.th32ParentProcessID);
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    map
}

fn process_path(pid: u32) -> String {
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        if ok { String::from_utf16_lossy(&buf[..len as usize]) } else { String::new() }
    }
}

/// `shell:AppsFolder\<família>!<id>` de um .exe dentro de `WindowsApps` (app da Store), lido do
/// AppxManifest.xml do pacote. Abrir o .exe direto não funciona para esses apps: com o app
/// fechado na bandeja (ex.: WhatsApp), o clique no dock não fazia nada.
fn store_app_id(path: &str) -> Option<String> {
    const DIR: &str = "\\windowsapps\\";
    let i = path.to_lowercase().find(DIR)? + DIR.len();
    let (folder, exe) = path[i..].split_once('\\')?;
    // Pasta do pacote: <Nome>_<Versão>_<Arquitetura>_<Recurso>_<IdDoEditor>.
    let parts: Vec<&str> = folder.split('_').collect();
    let [name, _, _, _, publisher] = parts[..] else { return None };
    let manifest = std::fs::read_to_string(Path::new(&path[..i]).join(folder).join("AppxManifest.xml")).ok()?;
    manifest.split("<Application ").skip(1).find_map(|tag| {
        let tag = &tag[..tag.find('>')?];
        let executable = xml_attr(tag, "Executable")?.replace('/', "\\");
        executable
            .eq_ignore_ascii_case(exe)
            .then(|| xml_attr(tag, "Id").map(|id| format!("{APPS_FOLDER}{name}_{publisher}!{id}")))
            .flatten()
    })
}

/// Valor do atributo `name="..."` dentro de uma tag XML.
fn xml_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let start = format!(" {tag}").find(&key)? + key.len() - 1;
    let len = tag[start..].find('"')?;
    Some(&tag[start..start + len])
}

/// Clique no ícone: abre o app, alterna entre as janelas dele ou minimiza a única em foco.
pub fn activate(source: &str, windows: &[HWND]) {
    if source == START {
        open_start();
        return;
    }
    unsafe {
        if windows.is_empty() {
            // App da Store fixado pelo .exe: abre pelo AppUserModelID, como a barra do Windows.
            let target = store_app_id(source).unwrap_or_else(|| source.to_string());
            ShellExecuteW(None, w!("open"), &HSTRING::from(target), None, None, SW_SHOWNORMAL);
            return;
        }
        let fg = GetForegroundWindow();
        // Minimizada pode continuar "em primeiro plano" (sem outra janela para ativar, o
        // Windows deixa o foco nela): não conta como em foco, senão minimizaria de novo.
        let focused = if IsIconic(fg).as_bool() { None } else { windows.iter().position(|&h| h == fg) };
        match focused {
            Some(_) if windows.len() == 1 => {
                crate::minimize::capture(fg); // foto antes de sumir (animação até o ícone)
                let _ = ShowWindow(fg, SW_MINIMIZE);
                // App rodando como administrador: o UIPI nega o ShowWindow, mas deixa passar
                // o WM_SYSCOMMAND, e a própria janela se minimiza.
                if !IsIconic(fg).as_bool() {
                    let _ = PostMessageW(Some(fg), WM_SYSCOMMAND, WPARAM(SC_MINIMIZE as usize), LPARAM(0));
                }
            }
            // Várias janelas: a próxima na ordem estável, em ciclo.
            Some(i) => focus_window(windows[(i + 1) % windows.len()]),
            // Nenhuma em foco: a mais recente não minimizada, ou a primeira se todas estiverem.
            None => {
                let z = z_order(windows);
                let top = z.iter().copied().find(|&h| !IsIconic(h).as_bool());
                focus_window(top.or(z.first().copied()).unwrap_or(windows[0]));
            }
        }
    }
}

/// Botão Iniciar: o lançador (se a tecla Windows abre o lançador) ou o menu Iniciar do Windows.
pub fn open_start() {
    if crate::config::win_key_launcher() {
        crate::launcher::toggle();
    } else {
        send_keys(&[VK_LWIN]);
    }
}

/// Clique direito no Iniciar: o menu do Win+X (Link rápido).
pub fn open_quick_link() {
    send_keys(&[VK_LWIN, VK_X]);
}

/// Pressiona as teclas em ordem e solta na ordem inversa.
fn send_keys(keys: &[VIRTUAL_KEY]) {
    let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                dwFlags: match (vk == VK_LWIN, up) {
                    (true, true) => KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP,
                    (true, false) => KEYEVENTF_EXTENDEDKEY,
                    (false, true) => KEYEVENTF_KEYUP,
                    (false, false) => Default::default(),
                },
                ..Default::default()
            },
        },
    };
    let inputs: Vec<INPUT> =
        keys.iter().map(|&k| key(k, false)).chain(keys.iter().rev().map(|&k| key(k, true))).collect();
    unsafe {
        SendInput(&inputs, size_of::<INPUT>() as i32);
    }
}

/// Scroll through one app's open windows, restoring a minimized target when needed.
pub fn cycle_window(windows: &[HWND], forward: bool) {
    if windows.len() < 2 {
        return;
    }
    unsafe {
        let fg = GetForegroundWindow();
        let current = if let Some(i) = windows.iter().position(|&h| h == fg) {
            Some(i)
        } else {
            let order = z_order(windows);
            order
                .iter()
                .copied()
                .find(|&h| !IsIconic(h).as_bool())
                .or_else(|| order.first().copied())
                .and_then(|h| windows.iter().position(|&w| w == h))
        };
        let Some(i) = current else { return };
        let next = if forward { (i + 1) % windows.len() } else { (i + windows.len() - 1) % windows.len() };
        focus_window(windows[next]);
    }
}

/// Traz a janela para frente, restaurando se estiver minimizada.
pub fn focus_window(hwnd: HWND) {
    unsafe {
        // Minimizada: SwitchToThisWindow restaura e ativa como o clique na barra de tarefas do
        // Windows. O ShowWindow(SW_RESTORE) de fora deixava apps WinUI 3 maximizados (ex.:
        // WhatsApp) com a interface presa no tamanho de janela minimizada (tela branca), e
        // com app rodando como administrador o UIPI nega o ShowWindow.
        if IsIconic(hwnd).as_bool() {
            SwitchToThisWindow(hwnd, true);
            return;
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

/// As janelas dadas, da mais à frente para a mais atrás.
fn z_order(windows: &[HWND]) -> Vec<HWND> {
    let mut all: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut all as *mut _ as isize));
    }
    all.retain(|h| windows.contains(h));
    all
}

/// Título da janela para o menu, cortado em ~60 caracteres.
pub fn menu_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) }.max(0) as usize;
    let title = String::from_utf16_lossy(&buf[..len]);
    let mut out: String = title.chars().take(60).collect();
    if title.chars().count() > 60 {
        out.pop();
        out.push('…');
    }
    // & no menu vira atalho de teclado: dobra para aparecer como texto.
    out.replace('&', "&&")
}

/// Traz para o pinned.txt os apps fixados na barra de tarefas do Windows, na ordem dela, sem
/// repetir os que já estão no dock. Atalhos `.lnk` viram o .exe de destino (assim casam com as
/// janelas abertas); apps da Store viram `shell:AppsFolder\<AUMID>`. Retorna quantos entraram.
pub fn import_taskbar_pins() -> usize {
    let cfg = config_path();
    if !cfg.exists() {
        let _ = load_pinned(16); // cria o arquivo padrão
    }
    let mut text = std::fs::read_to_string(&cfg).unwrap_or_default();
    let mut keys: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(source_key)
        .collect();
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut added = 0;
    // Separador só depois de um app que acabou de entrar (nada de --- solto ou repetido).
    let mut last_added = false;
    for source in taskbar_pins() {
        if source == SEP_LINE {
            if last_added {
                text.push_str(SEP_LINE);
                text.push_str(nl);
                last_added = false;
            }
            continue;
        }
        let key = source_key(&source);
        if keys.contains(&key) {
            last_added = false;
            continue;
        }
        last_added = true;
        if !text.is_empty() && !text.ends_with('\n') {
            text.push_str(nl);
        }
        text.push_str(&source);
        text.push_str(nl);
        keys.push(key);
        added += 1;
    }
    if added > 0 {
        let _ = std::fs::write(&cfg, text);
    }
    added
}

/// Fixados da barra do Windows, na ordem dela. A ordem fica em `Taskband\Favorites` (binário
/// com os itens serializados): dele saem os nomes dos atalhos e os AUMIDs dos apps da Store.
/// Sem o registro, vale a pasta de atalhos fixados (em ordem alfabética).
fn taskbar_pins() -> Vec<String> {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();
    let pinned = base.join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned");
    let mut out: Vec<String> = Vec::new();
    // `---` (separador) pode repetir, mas nunca no começo nem dois seguidos.
    let mut push = |s: String| {
        let ok = if s == SEP_LINE { out.last().is_some_and(|l| l != SEP_LINE) } else { !out.contains(&s) };
        if ok {
            out.push(s);
        }
    };
    let tokens = favorites_strings();
    if tokens.is_empty() {
        if let Ok(dir) = std::fs::read_dir(pinned.join("TaskBar")) {
            let mut files: Vec<PathBuf> = dir.filter_map(|e| e.ok().map(|e| e.path())).collect();
            files.sort();
            files.iter().filter_map(|f| resolve_lnk(f)).for_each(&mut push);
        }
        return out;
    }
    let aumids: Vec<&str> = tokens.iter().map(String::as_str).filter(|t| is_aumid(t)).collect();
    for t in &tokens {
        if t.to_ascii_lowercase().ends_with(".lnk") {
            // Separadores do Windhawk na barra do Windows viram separadores do dock.
            if t.contains("WindhawkSeparator") {
                push(SEP_LINE.to_string());
                continue;
            }
            if let Some(source) = find_lnk(&pinned, t).and_then(|p| resolve_lnk(&p)) {
                push(source);
            }
        } else if is_aumid(t) {
            if let Some(id) = valid_aumid(t) {
                push(format!("{APPS_FOLDER}{id}"));
            }
        } else if let Some(family) = package_family(t) {
            // App da Store gravado só pela família do pacote: o AUMID vem do manifesto.
            if aumids.iter().any(|a| a.starts_with(&format!("{family}!"))) {
                continue;
            }
            if let Some(id) = tokens.iter().find_map(|p| manifest_app_id(p, family)) {
                push(format!("{APPS_FOLDER}{family}!{id}"));
            }
        }
    }
    out
}

/// Textos UTF-16 dentro do valor `Favorites`, na ordem em que aparecem (nas duas
/// alinhagens de byte, pois os itens não começam sempre em posição par).
fn favorites_strings() -> Vec<String> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_BINARY, RegGetValueW};
    let key = w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Taskband");
    let mut size = 0u32;
    unsafe {
        if RegGetValueW(HKEY_CURRENT_USER, key, w!("Favorites"), RRF_RT_REG_BINARY, None, None, Some(&mut size)).is_err() {
            return Vec::new();
        }
    }
    let mut blob = vec![0u8; size as usize];
    unsafe {
        if RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            w!("Favorites"),
            RRF_RT_REG_BINARY,
            None,
            Some(blob.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
        .is_err()
        {
            return Vec::new();
        }
    }
    blob.truncate(size as usize);
    let mut found: Vec<(usize, String)> = Vec::new();
    for start in 0..2 {
        let units: Vec<u16> = blob[start..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let mut run = String::new();
        let mut at = 0;
        for (i, &u) in units.iter().enumerate() {
            match char::from_u32(u as u32).filter(|c| u < 0x2000 && !c.is_control()) {
                Some(c) => {
                    if run.is_empty() {
                        at = start + i * 2;
                    }
                    run.push(c);
                }
                None => {
                    if run.chars().count() >= 4 {
                        found.push((at, std::mem::take(&mut run)));
                    }
                    run.clear();
                }
            }
        }
    }
    found.sort_by_key(|f| f.0);
    found.into_iter().map(|f| f.1).collect()
}

/// AUMID que existe na pasta de apps. O registro às vezes guarda o id com um sufixo
/// (`MSTeams_...!MSTeams.Work`): tira os sufixos `.x` até achar o app.
fn valid_aumid(id: &str) -> Option<String> {
    let mut id = id.to_string();
    loop {
        let item: windows::core::Result<windows::Win32::UI::Shell::IShellItem> =
            unsafe { SHCreateItemFromParsingName(&HSTRING::from(format!("{APPS_FOLDER}{id}")), None) };
        if item.is_ok() {
            return Some(id);
        }
        let bang = id.find('!')?;
        let dot = id.rfind('.').filter(|&d| d > bang)?;
        id.truncate(dot);
    }
}

/// `Nome_editor13!IdDoApp` (AUMID de app empacotado).
fn is_aumid(t: &str) -> bool {
    let Some((family, app)) = t.split_once('!') else { return false };
    package_family(family).is_some()
        && !app.is_empty()
        && app.chars().all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c))
}

/// Família de pacote `Nome_editor13` (13 letras/dígitos minúsculos depois do último `_`).
fn package_family(t: &str) -> Option<&str> {
    let (name, publisher) = t.rsplit_once('_')?;
    let ok = !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || ".-".contains(c))
        && publisher.len() == 13
        && publisher.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    ok.then_some(t)
}

/// Id do primeiro app no manifesto do pacote `family`, se `path` for a pasta dele.
fn manifest_app_id(path: &str, family: &str) -> Option<String> {
    let (name, publisher) = family.rsplit_once('_')?;
    let folder = path.strip_prefix(r"C:\Program Files\WindowsApps\")?;
    if folder.contains('\\') || !folder.starts_with(&format!("{name}_")) || !folder.ends_with(&format!("__{publisher}")) {
        return None;
    }
    let manifest = std::fs::read_to_string(Path::new(path).join("AppxManifest.xml")).ok()?;
    let tag = manifest.split("<Application ").nth(1)?;
    xml_attr(&tag[..tag.find('>')?], "Id").map(str::to_string)
}

/// Arquivo do atalho `token` (que pode vir com um caractere a mais na frente, do cabeçalho
/// do item): na pasta dos fixados ou nas subpastas de ImplicitAppShortcuts.
fn find_lnk(pinned: &Path, token: &str) -> Option<PathBuf> {
    let mut dirs = vec![pinned.join("TaskBar")];
    if let Ok(rd) = std::fs::read_dir(pinned.join("ImplicitAppShortcuts")) {
        dirs.extend(rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_dir()));
    }
    for (i, _) in token.char_indices().take(3) {
        let name = &token[i..];
        if let Some(p) = dirs.iter().map(|d| d.join(name)).find(|p| p.is_file()) {
            return Some(p);
        }
    }
    None
}

/// Destino do atalho: o .exe; atalho sem caminho (app da Store, Explorador) pelo AUMID.
/// Separadores do Windhawk (apontam para systray.exe) ficam de fora.
fn resolve_lnk(lnk: &Path) -> Option<String> {
    use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile, STGM_READ};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    if !lnk.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")) {
        return None;
    }
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        link.cast::<IPersistFile>().ok()?.Load(&HSTRING::from(lnk.as_os_str()), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        let mut fd = WIN32_FIND_DATAW::default();
        let _ = link.GetPath(&mut buf, &mut fd, 0);
        let len = buf.iter().position(|&c| c == 0).unwrap_or(0);
        let target = String::from_utf16_lossy(&buf[..len]);
        let exe = exe_key(&target);
        if exe == "systray.exe" {
            return None;
        }
        // Navegadores abrindo apps web (PWAs): o próprio atalho, que guarda o app.
        if exe == "chrome_proxy.exe" || exe == "msedge_proxy.exe" {
            return Some(lnk.to_string_lossy().into_owned());
        }
        // Alias de execução da Store (`...\Microsoft\WindowsApps\x.exe`): vale o AUMID do atalho.
        let alias = target.to_lowercase().contains("\\microsoft\\windowsapps\\");
        if !alias && !target.is_empty() && Path::new(&target).exists() {
            return Some(target);
        }
        let store: IPropertyStore = link.cast().ok()?;
        let id = store.GetValue(&PKEY_AppUserModel_ID).map(|v| v.to_string()).unwrap_or_default();
        match id.as_str() {
            "Microsoft.Windows.Explorer" => Some(r"C:\Windows\explorer.exe".to_string()),
            "" => (!alias).then(|| lnk.to_string_lossy().into_owned()),
            id => match valid_aumid(id) {
                Some(id) => Some(format!("{APPS_FOLDER}{id}")),
                None => (!alias).then(|| lnk.to_string_lossy().into_owned()),
            },
        }
    }
}

/// Fixa ou desafixa o app no dock e no pinned.txt (comentários e demais linhas ficam como estão).
/// Fixados ficam antes dos abertos: o recém-fixado vai para o fim do grupo dos fixados.
pub fn set_pinned(apps: &mut Vec<App>, key: &str, pin: bool) {
    let Some(i) = apps.iter().position(|a| a.key == key) else { return };
    if apps[i].pinned == pin || apps[i].source == START {
        return;
    }
    let cfg = config_path();
    let mut text = std::fs::read_to_string(&cfg).unwrap_or_default();
    let source = apps[i].source.clone();
    if pin {
        let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
        if !text.is_empty() && !text.ends_with('\n') {
            text.push_str(nl);
        }
        text.push_str(&source);
        text.push_str(nl);
    } else {
        // Sai a linha do app e o separador logo depois dele, se houver.
        let mut out = String::new();
        let mut skip_sep = false;
        for l in text.split_inclusive('\n') {
            if l.trim() == source {
                skip_sep = true;
                continue;
            }
            if skip_sep && l.trim() == SEP_LINE {
                skip_sep = false;
                continue;
            }
            skip_sep = false;
            out.push_str(l);
        }
        text = out;
    }
    if let Some(dir) = cfg.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&cfg, text);

    let mut app = apps.remove(i);
    app.pinned = pin;
    app.sep_after = false;
    if pin || !app.windows.is_empty() {
        // Depois do Iniciar (índice 0) e dos outros fixados.
        let at = 1 + apps.iter().filter(|a| a.pinned && a.source != START).count();
        apps.insert(at, app);
    }
}

thread_local! {
    /// Menu do Windows aberto por "Mais opções…": recebe as mensagens dos submenus dinâmicos.
    static SHELL_MENU: std::cell::RefCell<Option<IContextMenu3>> = const { std::cell::RefCell::new(None) };
}

/// Mostra o menu de contexto do próprio Windows para o app (o mesmo do clique direito no
/// Explorer) e executa a opção escolhida.
pub fn shell_menu(hwnd: HWND, source: &str, pt: POINT, flags: TRACK_POPUP_MENU_FLAGS) {
    unsafe {
        let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(source), None) else { return };
        let Ok(cm) = item.BindToHandler::<_, IContextMenu>(None, &BHID_SFUIObject) else { return };
        let Ok(menu) = CreatePopupMenu() else { return };
        const FIRST: u32 = 1;
        if cm.QueryContextMenu(menu, 0, FIRST, 0x7FFF, CMF_NORMAL).is_ok() {
            SHELL_MENU.with(|m| *m.borrow_mut() = cm.cast::<IContextMenu3>().ok());
            let _ = SetForegroundWindow(hwnd);
            let cmd = TrackPopupMenu(menu, flags, pt.x, pt.y, None, hwnd, None).0 as u32;
            SHELL_MENU.with(|m| *m.borrow_mut() = None);
            if cmd >= FIRST {
                let info = CMINVOKECOMMANDINFO {
                    cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
                    hwnd,
                    lpVerb: PCSTR((cmd - FIRST) as usize as *const u8),
                    nShow: SW_SHOWNORMAL.0,
                    ..Default::default()
                };
                let _ = cm.InvokeCommand(&info);
            }
        }
        let _ = DestroyMenu(menu);
    }
}

/// Repassa ao menu do Windows aberto as mensagens que montam/desenham os submenus dele.
pub fn shell_menu_msg(msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
    if !matches!(msg, WM_INITMENUPOPUP | WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR) {
        return None;
    }
    SHELL_MENU.with(|m| {
        let m = m.borrow();
        let cm = m.as_ref()?;
        let mut r = LRESULT(0);
        unsafe { cm.HandleMenuMsg2(msg, wp, lp, Some(&mut r)) }.ok()?;
        Some(r)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_app_id_reads_the_package_manifest() {
        let root = std::env::temp_dir().join(format!("arcdock-test-{}", std::process::id())).join("WindowsApps");
        let pkg = root.join("5319275A.WhatsAppDesktop_2.2645.101.0_x64__cv1g1gvanyjgm");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(
            pkg.join("AppxManifest.xml"),
            r#"<Applications><Application Id="Other" Executable="Tool.exe"></Application><Application Id="App" Executable="WhatsApp.Root.exe" EntryPoint="Windows.FullTrustApplication"></Application></Applications>"#,
        )
        .unwrap();
        let exe = pkg.join("WhatsApp.Root.exe").display().to_string();
        assert_eq!(
            store_app_id(&exe).as_deref(),
            Some("shell:AppsFolder\\5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App")
        );
        assert_eq!(store_app_id(&pkg.join("Missing.exe").display().to_string()), None);
        assert_eq!(store_app_id(r"C:\Program Files\App\app.exe"), None);
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }
}