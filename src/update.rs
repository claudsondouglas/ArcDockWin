//! Atualização pelo GitHub Releases. Com a opção ligada (padrão), uma thread consulta o último
//! release pouco depois de abrir e a cada 6 h; achando versão mais nova, o menu do dock e as
//! Configurações oferecem instalar. Instalar baixa o exe do release, confere tamanho e SHA-256,
//! troca o arquivo (o Windows deixa renomear o exe em uso: o atual vira `.old`) e abre a versão
//! nova com `--after <pid>`, que espera esta sair antes de seguir.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;

use serde_json::Value;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::Security::Cryptography::{BCRYPT_SHA256_ALG_HANDLE, BCryptHash};
use windows::Win32::System::Threading::{GetCurrentProcessId, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SW_SHOWNORMAL, WM_APP};
use windows::core::{HSTRING, PCWSTR, w};

use crate::config;

/// Estado mudou (postada para o dock principal); wParam = 1: exe novo no lugar, reabrir.
pub const WM_UPDATE: u32 = WM_APP + 52;
const REPO: &str = "claudsondouglas/ArcDockWin";
const FIRST_CHECK: Duration = Duration::from_secs(30);
const EVERY: Duration = Duration::from_secs(6 * 3600);

#[derive(Clone, PartialEq)]
pub enum Status {
    Idle,
    Checking,
    UpToDate,
    /// Versão nova (sem o "v").
    Available(String),
    Downloading,
    Failed,
}

#[derive(Clone)]
struct Asset {
    url: String,
    size: u64,
    sha256: Option<String>,
}

static STATUS: Mutex<Status> = Mutex::new(Status::Idle);
static ASSET: Mutex<Option<Asset>> = Mutex::new(None);
static HWND_MAIN: AtomicIsize = AtomicIsize::new(0);

pub fn status() -> Status {
    STATUS.lock().map(|s| s.clone()).unwrap_or(Status::Idle)
}

fn set_status(s: Status) {
    if let Ok(mut g) = STATUS.lock() {
        *g = s;
    }
    notify(0);
}

fn notify(ready: usize) {
    let hwnd = HWND(HWND_MAIN.load(Ordering::Relaxed) as _);
    if !hwnd.is_invalid() {
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_UPDATE, WPARAM(ready), LPARAM(0));
        }
    }
}

fn exe_sibling(suffix: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut name = exe.file_name()?.to_os_string();
    name.push(suffix);
    Some(exe.with_file_name(name))
}

/// Antes da instância única: aberto por uma atualização (`--after <pid>`), espera a versão
/// anterior fechar (até 15 s). Também apaga o exe antigo deixado pela troca.
pub fn after_update() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--after")
        && let Some(pid) = args.next().and_then(|p| p.parse::<u32>().ok())
    {
        unsafe {
            if let Ok(process) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
                WaitForSingleObject(process, 15_000);
                let _ = CloseHandle(process);
            }
        }
    }
    for suffix in [".old", ".new"] {
        if let Some(p) = exe_sibling(suffix) {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Dock principal, que recebe [`WM_UPDATE`] (muda quando os docks são recriados).
pub fn set_window(main: HWND) {
    HWND_MAIN.store(main.0 as isize, Ordering::Relaxed);
}

/// Começa a procurar atualizações em segundo plano (se a opção estiver ligada).
pub fn start(main: HWND) {
    set_window(main);
    std::thread::spawn(|| {
        std::thread::sleep(FIRST_CHECK);
        loop {
            if config::auto_update() && matches!(status(), Status::Idle | Status::UpToDate | Status::Failed) {
                check();
            }
            std::thread::sleep(EVERY);
        }
    });
}

/// "Procurar agora" das Configurações.
pub fn check_now() {
    if matches!(status(), Status::Checking | Status::Downloading) {
        return;
    }
    std::thread::spawn(check);
}

fn check() {
    set_status(Status::Checking);
    let found = latest();
    set_status(match found {
        Some(Some((version, asset))) => {
            if let Ok(mut a) = ASSET.lock() {
                *a = Some(asset);
            }
            Status::Available(version)
        }
        Some(None) => Status::UpToDate,
        None => Status::Failed,
    });
}

/// `None`: sem conexão ou resposta estranha; `Some(None)`: já na última versão.
fn latest() -> Option<Option<(String, Asset)>> {
    let path = HSTRING::from(format!("/repos/{REPO}/releases/latest"));
    let (code, body) = crate::usage::http_get(w!("api.github.com"), PCWSTR(path.as_ptr()), "Accept: application/vnd.github+json\r\n")?;
    if code != 200 {
        return None;
    }
    let v: Value = serde_json::from_slice(&body).ok()?;
    let tag = v["tag_name"].as_str()?.trim_start_matches('v').to_string();
    if !newer(&tag, env!("CARGO_PKG_VERSION")) {
        return Some(None);
    }
    let asset = v["assets"]
        .as_array()?
        .iter()
        .find(|a| a["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case("arcdockwin.exe")))?;
    let asset = Asset {
        url: asset["browser_download_url"].as_str()?.to_string(),
        size: asset["size"].as_u64()?,
        sha256: asset["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).map(str::to_ascii_lowercase),
    };
    Some(Some((tag, asset)))
}

fn parse(v: &str) -> Option<(u32, u32, u32)> {
    let mut it = v.split('.').map(|p| p.parse::<u32>().ok());
    Some((it.next()??, it.next()??, it.next().unwrap_or(Some(0))?))
}

fn newer(latest: &str, current: &str) -> bool {
    matches!((parse(latest), parse(current)), (Some(l), Some(c)) if l > c)
}

/// Baixa e troca o exe numa thread; pronto, o dock principal reabre (ver [`relaunch`]).
pub fn install() {
    if !matches!(status(), Status::Available(_)) {
        return;
    }
    let Some(asset) = ASSET.lock().ok().and_then(|a| a.clone()) else { return };
    set_status(Status::Downloading);
    std::thread::spawn(move || {
        if swap(&asset).is_some() {
            notify(1);
        } else {
            set_status(Status::Failed);
        }
    });
}

fn swap(asset: &Asset) -> Option<()> {
    let url = asset.url.strip_prefix("https://")?;
    let (host, path) = url.split_once('/')?;
    let (host, path) = (HSTRING::from(host), HSTRING::from(format!("/{path}")));
    let (code, body) = crate::usage::http_get(PCWSTR(host.as_ptr()), PCWSTR(path.as_ptr()), "")?;
    if code != 200 || body.len() as u64 != asset.size || !body.starts_with(b"MZ") {
        return None;
    }
    if let Some(expected) = &asset.sha256
        && sha256_hex(&body)? != *expected
    {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let new = exe_sibling(".new")?;
    let old = exe_sibling(".old")?;
    std::fs::write(&new, &body).ok()?;
    let _ = std::fs::remove_file(&old);
    if std::fs::rename(&exe, &old).is_err() {
        let _ = std::fs::remove_file(&new);
        return None;
    }
    if std::fs::rename(&new, &exe).is_err() {
        let _ = std::fs::rename(&old, &exe);
        let _ = std::fs::remove_file(&new);
        return None;
    }
    Some(())
}

fn sha256_hex(data: &[u8]) -> Option<String> {
    let mut out = [0u8; 32];
    unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut out) }.ok().ok()?;
    Some(out.iter().map(|b| format!("{b:02x}")).collect())
}

/// No dock principal, com o exe novo no lugar: abre a versão nova e fecha esta.
pub fn relaunch(main: HWND) {
    let Ok(exe) = std::env::current_exe() else { return };
    let pid = unsafe { GetCurrentProcessId() };
    if std::process::Command::new(exe).arg("--after").arg(pid.to_string()).spawn().is_ok() {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(main);
        }
    } else {
        set_status(Status::Failed);
    }
}

/// Falhou: abre a página do último release para baixar à mão.
pub fn open_release_page() {
    let url = HSTRING::from(format!("https://github.com/{REPO}/releases/latest"));
    unsafe {
        ShellExecuteW(None, w!("open"), &url, None, None, SW_SHOWNORMAL);
    }
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    fn compares_versions() {
        assert!(newer("0.4.0", "0.3.0"));
        assert!(newer("0.10.0", "0.9.9"));
        assert!(newer("1.0", "0.9.9"));
        assert!(!newer("0.3.0", "0.3.0"));
        assert!(!newer("0.2.9", "0.3.0"));
        assert!(!newer("lixo", "0.3.0"));
    }
}
