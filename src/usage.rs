//! Módulos "Claude" e "Codex" da topbar: quanto do plano já foi usado (sessão de 5 h e limites
//! semanais) e os tokens gastos nos últimos 7 dias, por dia e por modelo.
//!
//! - Claude: limites pela mesma API que o `/usage` do Claude Code consulta, com o login de
//!   `~/.claude/.credentials.json` (ou `CLAUDE_CONFIG_DIR`); tokens dos logs em
//!   `~/.claude/projects`.
//! - Codex: limites pela API do ChatGPT que o Codex consulta, com o login de
//!   `~/.codex/auth.json` (ou `CODEX_HOME`); tokens dos logs em `~/.codex/sessions`.
//!
//! Nenhum login é renovado aqui: com o token vencido, o painel pede para abrir o app. A leitura
//! roda numa thread e avisa a topbar com [`WM_USAGE`] (wParam = índice do provedor).

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
};
use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SW_SHOWNORMAL, WM_APP};
use windows::core::{HSTRING, PCWSTR, w};

use crate::i18n::tr;
use crate::panel::{self, Event, PanelContent, Widget};

/// Leitura terminou (postada para a janela da topbar principal); wParam = [`Provider::idx`].
pub const WM_USAGE: u32 = WM_APP + 70;
/// Releitura dos limites para o número da barra.
pub const REFRESH_MS: u32 = 5 * 60 * 1000;
/// Logos (Simple Icons, viewBox 24×24).
const LOGO_CLAUDE: &str = "m4.7144 15.9555 4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956-.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797-.3218.686.0608 1.5179.1032 2.2767.1578 1.6514.0972 2.4468.255h.3886l.0546-.1579-.1336-.0971-.1032-.0972L6.973 9.8356l-2.55-1.6879-1.3356-.9714-.7225-.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 1.9064 1.4754 2.4893 1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539-2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285L6.287.1335 6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307-2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 1.6514-2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 1.5421-.2794 1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614-3.4364.8136-.0425.0304.0486.0607 1.5482.1457.6618.0364h1.621l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107-1.3113-.3279h-.1822v.1093l1.0929 1.0686 2.0035 1.8092 2.5075 2.3314.1275.5768-.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17l.4432.6496 2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721-1.9246L14.38 17.959l-1.1414-1.9428-.1397.079-.674 7.2552-.3156.3703-.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853-1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 1.9672-2.1796 2.9446-1.7243 1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 2.386-3.0357 1.4389-1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 4.1164-1.1293.1457-.4857-.4554.0608-.7467.2307-.2429 1.9064-1.3114Z";
const LOGO_OPENAI: &str = "M22.2819 9.8211a5.9847 5.9847 0 0 0-.5157-4.9108 6.0462 6.0462 0 0 0-6.5098-2.9A6.0651 6.0651 0 0 0 4.9807 4.1818a5.9847 5.9847 0 0 0-3.9977 2.9 6.0462 6.0462 0 0 0 .7427 7.0966 5.98 5.98 0 0 0 .511 4.9107 6.051 6.051 0 0 0 6.5146 2.9001A5.9847 5.9847 0 0 0 13.2599 24a6.0557 6.0557 0 0 0 5.7718-4.2058 5.9894 5.9894 0 0 0 3.9977-2.9001 6.0557 6.0557 0 0 0-.7475-7.0729zm-9.022 12.6081a4.4755 4.4755 0 0 1-2.8764-1.0408l.1419-.0804 4.7783-2.7582a.7948.7948 0 0 0 .3927-.6813v-6.7369l2.02 1.1686a.071.071 0 0 1 .038.052v5.5826a4.504 4.504 0 0 1-4.4945 4.4944zm-9.6607-4.1254a4.4708 4.4708 0 0 1-.5346-3.0137l.142.0852 4.783 2.7582a.7712.7712 0 0 0 .7806 0l5.8428-3.3685v2.3324a.0804.0804 0 0 1-.0332.0615L9.74 19.9502a4.4992 4.4992 0 0 1-6.1408-1.6464zM2.3408 7.8956a4.485 4.485 0 0 1 2.3655-1.9728V11.6a.7664.7664 0 0 0 .3879.6765l5.8144 3.3543-2.0201 1.1685a.0757.0757 0 0 1-.071 0l-4.8303-2.7865A4.504 4.504 0 0 1 2.3408 7.872zm16.5963 3.8558L13.1038 8.364 15.1192 7.2a.0757.0757 0 0 1 .071 0l4.8303 2.7913a4.4944 4.4944 0 0 1-.6765 8.1042v-5.6772a.79.79 0 0 0-.407-.667zm2.0107-3.0231l-.142-.0852-4.7735-2.7818a.7759.7759 0 0 0-.7854 0L9.409 9.2297V6.8974a.0662.0662 0 0 1 .0284-.0615l4.8303-2.7866a4.4992 4.4992 0 0 1 6.6802 4.66zM8.3065 12.863l-2.02-1.1638a.0804.0804 0 0 1-.038-.0567V6.0742a4.4992 4.4992 0 0 1 7.3757-3.4537l-.142.0805L8.704 5.459a.7948.7948 0 0 0-.3927.6813zm1.0976-2.3654l2.602-1.4998 2.6069 1.4998v2.9994l-2.5974 1.4997-2.6067-1.4997Z";
const GLYPH_TIMER: u16 = 0xE916;
const GLYPH_CALENDAR: u16 = 0xE787;
const GLYPH_OPEN: u16 = 0xE8A7;
const OPEN_USAGE: u32 = 1;
const DAYS: usize = 7;
/// Varrer os logs custa: no máximo uma vez a cada TOKENS_EVERY.
const TOKENS_EVERY: Duration = Duration::from_secs(60);
const MODELS_SHOWN: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Provider {
    Claude,
    Codex,
}

impl Provider {
    #[cfg(test)]
    pub const ALL: [Provider; 2] = [Provider::Claude, Provider::Codex];

    pub fn idx(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        match self {
            Provider::Claude => "Claude",
            Provider::Codex => "Codex",
        }
    }

    fn logo(self) -> &'static str {
        match self {
            Provider::Claude => LOGO_CLAUDE,
            Provider::Codex => LOGO_OPENAI,
        }
    }

    fn dir(self) -> PathBuf {
        let (env, sub) = match self {
            Provider::Claude => ("CLAUDE_CONFIG_DIR", ".claude"),
            Provider::Codex => ("CODEX_HOME", ".codex"),
        };
        if let Some(d) = std::env::var_os(env) {
            return d.into();
        }
        std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default().join(sub)
    }

    fn usage_url(self) -> &'static str {
        match self {
            Provider::Claude => "https://claude.ai/settings/usage",
            Provider::Codex => "https://chatgpt.com/codex/settings/usage",
        }
    }

    fn open_text(self) -> &'static str {
        match self {
            Provider::Claude => "Abrir uso no claude.ai",
            Provider::Codex => "Abrir uso no chatgpt.com",
        }
    }

    // Mensagens de erro (texto inteiro, para a tradução por tabela).
    fn no_login(self) -> &'static str {
        match self {
            Provider::Claude => "Entre no Claude Code para ver o uso.",
            Provider::Codex => "Entre no Codex para ver o uso.",
        }
    }

    fn expired(self) -> &'static str {
        match self {
            Provider::Claude => "Login vencido: abra o Claude Code para renovar.",
            Provider::Codex => "Login vencido: abra o Codex para renovar.",
        }
    }

    fn offline(self) -> &'static str {
        match self {
            Provider::Claude => "Sem conexão com o Claude.",
            Provider::Codex => "Sem conexão com o Codex.",
        }
    }

    fn bad_reply(self) -> &'static str {
        match self {
            Provider::Claude => "O Claude não respondeu agora.",
            Provider::Codex => "O Codex não respondeu agora.",
        }
    }
}

struct Limit {
    label: String,
    glyph: u16,
    percent: f32,
    /// Unix (s) da renovação.
    resets_at: Option<i64>,
    session: bool,
}

struct Tokens {
    /// Do mais antigo (6 dias atrás) para hoje.
    days: [u64; DAYS],
    /// Dia local de hoje (dias desde 1970).
    today: i64,
    /// Do mais usado para o menos usado.
    models: Vec<(String, u64)>,
}

struct State {
    plan: Option<String>,
    limits: Vec<Limit>,
    error: Option<&'static str>,
    tokens: Option<Tokens>,
    tokens_at: Option<Instant>,
    want_tokens: bool,
    busy: bool,
    loaded: bool,
}

impl State {
    const fn new() -> Self {
        State { plan: None, limits: Vec::new(), error: None, tokens: None, tokens_at: None, want_tokens: false, busy: false, loaded: false }
    }
}

static STATE: [Mutex<State>; 2] = [Mutex::new(State::new()), Mutex::new(State::new())];

/// Uso da sessão de 5 h (o número da barra).
pub fn session_percent(p: Provider) -> Option<u8> {
    let s = STATE[p.idx()].lock().ok()?;
    s.limits.iter().find(|l| l.session).map(|l| l.percent.round().clamp(0.0, 100.0) as u8)
}

/// Relê os limites numa thread (e os tokens, se `with_tokens` e a última varredura já é velha).
/// Uma leitura por vez por provedor; ao terminar, posta [`WM_USAGE`] para a topbar.
pub fn refresh(p: Provider, with_tokens: bool) {
    let hwnd = crate::topbar::hwnd().0 as isize;
    let state = &STATE[p.idx()];
    {
        let Ok(mut s) = state.lock() else { return };
        if with_tokens && s.tokens_at.is_none_or(|t| t.elapsed() >= TOKENS_EVERY) {
            s.want_tokens = true;
        }
        if s.busy {
            return;
        }
        s.busy = true;
    }
    std::thread::spawn(move || {
        let dir = p.dir();
        let (plan, limits) = match p {
            Provider::Claude => fetch_claude(&dir),
            Provider::Codex => fetch_codex(&dir),
        };
        let scan_now = state.lock().map(|mut s| std::mem::take(&mut s.want_tokens)).unwrap_or(false);
        let tokens = scan_now.then(|| match p {
            Provider::Claude => scan_claude(&dir),
            Provider::Codex => scan_codex(&dir),
        });
        if let Ok(mut s) = state.lock() {
            if plan.is_some() {
                s.plan = plan;
            }
            match limits {
                Ok(l) => {
                    s.limits = l;
                    s.error = None;
                }
                Err(e) => s.error = Some(e),
            }
            if let Some(t) = tokens {
                s.tokens = Some(t);
                s.tokens_at = Some(Instant::now());
            }
            s.busy = false;
            s.loaded = true;
        }
        unsafe {
            let _ = PostMessageW(Some(HWND(hwnd as _)), WM_USAGE, WPARAM(p.idx()), LPARAM(0));
        }
    });
}

type Fetched = (Option<String>, Result<Vec<Limit>, &'static str>);

fn read_json(path: &Path) -> Option<Value> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok())
}

/// Resposta da API → limites, com os erros comuns aos dois provedores.
fn reply(p: Provider, resp: Option<(u32, Vec<u8>)>, parse: impl FnOnce(&Value) -> Vec<Limit>) -> Result<Vec<Limit>, &'static str> {
    match resp {
        None => Err(p.offline()),
        Some((401 | 403, _)) => Err(p.expired()),
        Some((200, body)) => serde_json::from_slice::<Value>(&body).map(|v| parse(&v)).map_err(|_| p.bad_reply()),
        Some(_) => Err(p.bad_reply()),
    }
}

/// Plano ("Max 5x", "Pro") e limites atuais do Claude.
fn fetch_claude(dir: &Path) -> Fetched {
    let p = Provider::Claude;
    let Some(creds) = read_json(&dir.join(".credentials.json")) else { return (None, Err(p.no_login())) };
    let oauth = &creds["claudeAiOauth"];
    let plan = claude_plan(oauth["subscriptionType"].as_str(), oauth["rateLimitTier"].as_str());
    let Some(token) = oauth["accessToken"].as_str() else { return (plan, Err(p.no_login())) };
    if oauth["expiresAt"].as_i64().is_some_and(|ms| ms / 1000 < now()) {
        return (plan, Err(p.expired()));
    }
    let headers = format!("Authorization: Bearer {token}\r\nanthropic-beta: oauth-2025-04-20\r\nAccept: application/json\r\n");
    let resp = http_get(w!("api.anthropic.com"), w!("/api/oauth/usage"), &headers);
    (plan, reply(p, resp, claude_limits))
}

fn claude_plan(kind: Option<&str>, tier: Option<&str>) -> Option<String> {
    let mut name = capitalize(kind?);
    if let Some(mult) = tier.and_then(|t| t.rsplit('_').next()).filter(|m| m.ends_with('x') && m[..m.len() - 1].parse::<u32>().is_ok()) {
        name = format!("{name} {mult}");
    }
    Some(name)
}

fn claude_limits(v: &Value) -> Vec<Limit> {
    let weekly = tr("Semanal");
    let mut out = Vec::new();
    if let Some(list) = v["limits"].as_array() {
        for l in list {
            let Some(percent) = l["percent"].as_f64() else { continue };
            let resets_at = l["resets_at"].as_str().and_then(parse_iso);
            let model = l["scope"]["model"]["display_name"].as_str();
            let (label, glyph, session) = match (l["kind"].as_str(), model) {
                (Some("session"), _) => (tr("Sessão (5 h)"), GLYPH_TIMER, true),
                (Some("weekly_all"), _) => (weekly.clone(), GLYPH_CALENDAR, false),
                (_, Some(m)) => (format!("{weekly} · {m}"), GLYPH_CALENDAR, false),
                _ => continue,
            };
            out.push(Limit { label, glyph, percent: percent as f32, resets_at, session });
        }
    }
    if out.is_empty() {
        // Formato antigo: um objeto por janela.
        for (key, label, glyph, session) in [
            ("five_hour", tr("Sessão (5 h)"), GLYPH_TIMER, true),
            ("seven_day", weekly.clone(), GLYPH_CALENDAR, false),
            ("seven_day_opus", format!("{weekly} · Opus"), GLYPH_CALENDAR, false),
            ("seven_day_sonnet", format!("{weekly} · Sonnet"), GLYPH_CALENDAR, false),
        ] {
            let w = &v[key];
            if let Some(p) = w["utilization"].as_f64() {
                out.push(Limit { label, glyph, percent: p as f32, resets_at: w["resets_at"].as_str().and_then(parse_iso), session });
            }
        }
    }
    out
}

/// Plano ("Plus", "Pro") e limites atuais do Codex.
fn fetch_codex(dir: &Path) -> Fetched {
    let p = Provider::Codex;
    let Some(auth) = read_json(&dir.join("auth.json")) else { return (None, Err(p.no_login())) };
    let t = &auth["tokens"];
    let Some(token) = t["access_token"].as_str() else { return (None, Err(p.no_login())) };
    let mut headers = format!("Authorization: Bearer {token}\r\nAccept: application/json\r\n");
    if let Some(acc) = t["account_id"].as_str() {
        headers += &format!("ChatGPT-Account-Id: {acc}\r\n");
    }
    let resp = http_get(w!("chatgpt.com"), w!("/backend-api/wham/usage"), &headers);
    let plan = resp
        .as_ref()
        .filter(|r| r.0 == 200)
        .and_then(|r| serde_json::from_slice::<Value>(&r.1).ok())
        .and_then(|v| v["plan_type"].as_str().map(capitalize));
    (plan, reply(p, resp, codex_limits))
}

fn codex_limits(v: &Value) -> Vec<Limit> {
    let mut out = Vec::new();
    for (key, primary) in [("primary_window", true), ("secondary_window", false)] {
        let w = &v["rate_limit"][key];
        let Some(percent) = w["used_percent"].as_f64() else { continue };
        let secs = w["limit_window_seconds"].as_i64().unwrap_or(0);
        let (label, glyph) = match secs {
            18000 => (tr("Sessão (5 h)"), GLYPH_TIMER),
            604800 => (tr("Semanal"), GLYPH_CALENDAR),
            s if s > 0 && s < 86400 => (format!("{} h", s / 3600), GLYPH_TIMER),
            s => (format!("{}d", s / 86400), GLYPH_CALENDAR),
        };
        let resets_at = w["reset_at"].as_i64().or_else(|| w["reset_after_seconds"].as_i64().map(|s| now() + s));
        out.push(Limit { label, glyph, percent: percent as f32, resets_at, session: primary });
    }
    out
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// GET HTTPS; (status, corpo) ou `None` sem conexão.
pub(crate) fn http_get(host: PCWSTR, path: PCWSTR, headers: &str) -> Option<(u32, Vec<u8>)> {
    struct Handle(*mut core::ffi::c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                let _ = unsafe { WinHttpCloseHandle(self.0) };
            }
        }
    }
    let ok = |h: *mut core::ffi::c_void| (!h.is_null()).then_some(Handle(h));
    unsafe {
        let session = ok(WinHttpOpen(w!("ArcDockWin"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0))?;
        let _ = WinHttpSetTimeouts(session.0, 5000, 5000, 10000, 10000);
        let conn = ok(WinHttpConnect(session.0, host, INTERNET_DEFAULT_HTTPS_PORT, 0))?;
        let req = ok(WinHttpOpenRequest(conn.0, w!("GET"), path, PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE))?;
        let headers: Vec<u16> = headers.encode_utf16().collect();
        WinHttpSendRequest(req.0, (!headers.is_empty()).then_some(&headers[..]), None, 0, 0, 0).ok()?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut()).ok()?;
        let mut status = 0u32;
        let mut len = 4u32;
        WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as _),
            &mut len,
            std::ptr::null_mut(),
        )
        .ok()?;
        let mut body = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let mut read = 0u32;
            WinHttpReadData(req.0, buf.as_mut_ptr() as _, buf.len() as u32, &mut read).ok()?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&buf[..read as usize]);
        }
        Some((status, body))
    }
}

/// Soma tokens por dia e por modelo nos últimos 7 dias. `each` lê os `.jsonl` de `dir` e chama
/// `add(timestamp unix, modelo, tokens)` para cada uso.
fn tally(dir: &Path, mut each: impl FnMut(&Path, &mut dyn FnMut(i64, &str, u64))) -> Tokens {
    let offset = local_offset();
    let today = (now() + offset).div_euclid(86400);
    let first = today - (DAYS as i64 - 1);
    let cutoff = SystemTime::now() - Duration::from_secs(86400 * (DAYS as u64 + 1));
    let mut days = [0u64; DAYS];
    let mut models: HashMap<String, u64> = HashMap::new();
    let mut files = Vec::new();
    collect_logs(dir, cutoff, &mut files);
    for f in files {
        each(&f, &mut |ts, model, n| {
            let day = (ts + offset).div_euclid(86400);
            if day < first || day > today || n == 0 || model.is_empty() || model.starts_with('<') {
                return;
            }
            days[(day - first) as usize] += n;
            *models.entry(model_name(model)).or_default() += n;
        });
    }
    let mut models: Vec<_> = models.into_iter().collect();
    models.sort_by(|a, b| b.1.cmp(&a.1));
    Tokens { days, today, models }
}

fn lines(f: &Path) -> impl Iterator<Item = String> {
    std::fs::File::open(f).into_iter().flat_map(|file| BufReader::new(file).lines().map_while(Result::ok))
}

/// Claude Code: entrada + saída + cache de cada resposta, sem contar duas vezes a mesma (o
/// Claude Code repete a linha a cada bloco da mensagem).
fn scan_claude(dir: &Path) -> Tokens {
    let mut seen = HashSet::new();
    tally(&dir.join("projects"), |f, add| {
        for line in lines(f) {
            if !line.contains("\"usage\"") || !line.contains("\"assistant\"") {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            let Some(ts) = v["timestamp"].as_str().and_then(parse_iso) else { continue };
            let msg = &v["message"];
            if let (Some(id), Some(req)) = (msg["id"].as_str(), v["requestId"].as_str())
                && !seen.insert(format!("{id}:{req}"))
            {
                continue;
            }
            let u = &msg["usage"];
            let total: u64 = ["input_tokens", "output_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"]
                .iter()
                .filter_map(|k| u[*k].as_u64())
                .sum();
            add(ts, msg["model"].as_str().unwrap_or(""), total);
        }
    })
}

/// Codex: cada `token_count` traz o total acumulado da sessão; o uso é a diferença para o
/// anterior, no modelo do último `turn_context`.
fn scan_codex(dir: &Path) -> Tokens {
    tally(&dir.join("sessions"), |f, add| {
        let mut model = String::new();
        let mut prev = 0u64;
        for line in lines(f) {
            let ctx = line.contains("\"turn_context\"");
            if !ctx && !line.contains("\"token_count\"") {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            let pl = &v["payload"];
            if ctx {
                if let Some(m) = pl["model"].as_str() {
                    model = m.to_string();
                }
                continue;
            }
            let Some(total) = pl["info"]["total_token_usage"]["total_tokens"].as_u64() else { continue };
            let delta = if total >= prev { total - prev } else { total };
            prev = total;
            if let Some(ts) = v["timestamp"].as_str().and_then(parse_iso) {
                add(ts, &model, delta);
            }
        }
    })
}

/// `.jsonl` mexidos depois de `cutoff`, em qualquer subpasta.
fn collect_logs(dir: &Path, cutoff: SystemTime, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        let p = e.path();
        if meta.is_dir() {
            collect_logs(&p, cutoff, out);
        } else if p.extension().is_some_and(|x| x == "jsonl") && meta.modified().is_ok_and(|m| m >= cutoff) {
            out.push(p);
        }
    }
}

/// "claude-opus-5-5" → "Opus 5.5"; "claude-sonnet-4-5-20250929" → "Sonnet 4.5";
/// "gpt-6.2-sol" → "GPT 6.2 Sol".
fn model_name(id: &str) -> String {
    if let Some(rest) = id.strip_prefix("gpt-") {
        return std::iter::once("GPT".to_string()).chain(rest.split('-').map(capitalize)).collect::<Vec<_>>().join(" ");
    }
    let id = id.strip_prefix("claude-").unwrap_or(id);
    let mut parts: Vec<&str> = id.split('-').filter(|p| !(p.len() == 8 && p.chars().all(|c| c.is_ascii_digit()))).collect();
    let Some(first) = parts.first().copied().filter(|p| p.chars().next().is_some_and(|c| c.is_ascii_alphabetic())) else {
        return id.to_string();
    };
    parts.remove(0);
    let mut name = capitalize(first);
    if !parts.is_empty() {
        name = format!("{name} {}", parts.join("."));
    }
    name
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Dias desde 1970-01-01 (algoritmo de Howard Hinnant).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Fuso local em segundos (hora local − UTC), arredondado a 15 min.
fn local_offset() -> i64 {
    let (l, u) = unsafe { (GetLocalTime(), GetSystemTime()) };
    let secs = |t: windows::Win32::Foundation::SYSTEMTIME| {
        days_from_civil(t.wYear as i64, t.wMonth as i64, t.wDay as i64) * 86400 + t.wHour as i64 * 3600 + t.wMinute as i64 * 60
    };
    ((secs(l) - secs(u)) as f64 / 900.0).round() as i64 * 900
}

/// "2026-11-23T15:29:59.743317+00:00" ou "...Z" → unix (s).
fn parse_iso(s: &str) -> Option<i64> {
    let n = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0, 4)?, n(5, 7)?, n(8, 10)?, n(11, 13)?, n(14, 16)?, n(17, 19)?);
    let rest = s.get(19..)?.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let off = match rest.as_bytes().first() {
        None | Some(b'Z') => 0,
        Some(&sign) => {
            let h = rest.get(1..3)?.parse::<i64>().ok()?;
            let m = rest.get(4..6).and_then(|m| m.parse::<i64>().ok()).unwrap_or(0);
            let o = h * 3600 + m * 60;
            if sign == b'-' { -o } else { o }
        }
    };
    Some(days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + se - off)
}

/// "Reinicia em 4d 17h" / "2h 05m" / "12m".
fn resets_in(at: i64) -> String {
    let left = (at - now()).max(0);
    let (d, h, m) = (left / 86400, left % 86400 / 3600, left % 3600 / 60);
    let t = if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m:02}m")
    } else {
        format!("{m}m")
    };
    format!("{} {t}", tr("Reinicia em"))
}

/// "950", "110.0K", "3.3M", "1.2B".
fn compact(n: u64) -> String {
    let f = n as f64;
    if n < 1000 {
        n.to_string()
    } else if f < 1e6 {
        format!("{:.1}K", f / 1e3)
    } else if f < 1e9 {
        format!("{:.1}M", f / 1e6)
    } else {
        format!("{:.1}B", f / 1e9)
    }
}

pub struct UsagePanel(Provider);

impl UsagePanel {
    pub fn new(p: Provider) -> Self {
        refresh(p, true);
        Self(p)
    }
}

impl PanelContent for UsagePanel {
    fn build(&mut self) -> Vec<Widget> {
        let p = self.0;
        let Ok(s) = STATE[p.idx()].lock() else { return Vec::new() };
        let image: fn(u32, [u8; 4]) -> Option<tiny_skia::Pixmap> = match p {
            Provider::Claude => |px, c| logo(Provider::Claude, px, c),
            Provider::Codex => |px, c| logo(Provider::Codex, px, c),
        };
        let mut w = vec![
            Widget::Banner { glyph: 0, image: Some(image), text: p.name().into(), sub: s.plan.clone().unwrap_or_default() },
            Widget::Separator,
        ];
        if let Some(e) = s.error {
            w.push(Widget::Text(e.into()));
        } else if !s.loaded && s.limits.is_empty() {
            w.push(Widget::Text("Carregando…".into()));
        }
        if !s.limits.is_empty() {
            w.push(Widget::Label("Limites".into()));
            for l in &s.limits {
                let lvl = if l.percent >= 90.0 { 2 } else if l.percent >= 75.0 { 1 } else { 0 };
                w.push(Widget::Meter {
                    glyph: l.glyph,
                    label: l.label.clone(),
                    value: format!("{:.0}%", l.percent),
                    detail: l.resets_at.map(resets_in),
                    frac: Some(l.percent / 100.0),
                    level: lvl,
                });
            }
        }
        if let Some(t) = &s.tokens {
            w.push(Widget::Separator);
            w.push(Widget::Label("Tokens por dia".into()));
            const WEEK: [&str; 7] = ["Dom", "Seg", "Ter", "Qua", "Qui", "Sex", "Sáb"];
            let max = t.days.iter().copied().max().unwrap_or(0).max(1) as f32;
            for (i, &n) in t.days.iter().enumerate() {
                let day = t.today - (DAYS - 1 - i) as i64;
                let today = i == DAYS - 1;
                // 1970-01-01 foi quinta-feira.
                let label = if today { "Hoje" } else { WEEK[(day + 4).rem_euclid(7) as usize] };
                w.push(Widget::Bar { label: label.into(), value: compact(n), frac: n as f32 / max, strong: today });
            }
            if !t.models.is_empty() {
                w.push(Widget::Label("Tokens por modelo".into()));
                let max = t.models[0].1.max(1) as f32;
                for (m, n) in t.models.iter().take(MODELS_SHOWN) {
                    w.push(Widget::Bar { label: m.clone(), value: compact(*n), frac: *n as f32 / max, strong: false });
                }
            }
        }
        w.push(Widget::Separator);
        w.push(Widget::Row { id: OPEN_USAGE, glyph: Some(GLYPH_OPEN), text: p.open_text().into(), sub: None, selected: false });
        w
    }

    fn event(&mut self, ev: Event) -> bool {
        if let Event::Click(OPEN_USAGE) = ev {
            let url = HSTRING::from(self.0.usage_url());
            unsafe { ShellExecuteW(None, w!("open"), &url, None, None, SW_SHOWNORMAL) };
            panel::close();
        }
        false
    }

    fn notify(&mut self, msg: u32, wparam: usize) -> bool {
        msg == WM_USAGE && wparam == self.0.idx()
    }
}

/// Logo do provedor num quadrado de `px` px na cor `color` (RGBA; a tinta dos ícones),
/// pré-multiplicado.
pub fn logo(p: Provider, px: u32, color: [u8; 4]) -> Option<tiny_skia::Pixmap> {
    let path = svg_path(p.logo())?.transform(tiny_skia::Transform::from_scale(px as f32 / 24.0, px as f32 / 24.0))?;
    let mut pm = tiny_skia::Pixmap::new(px, px)?;
    let mut paint = tiny_skia::Paint::default();
    let [r, g, b, a] = color;
    paint.set_color_rgba8(r, g, b, a);
    paint.anti_alias = true;
    pm.fill_path(&path, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
    Some(pm)
}

/// Path do SVG com os comandos M L H V C A Z (absolutos e relativos), o que os logos usam.
fn svg_path(d: &str) -> Option<tiny_skia::Path> {
    let b = d.as_bytes();
    let mut i = 0;
    let skip = |i: &mut usize| {
        while *i < b.len() && (b[*i] == b' ' || b[*i] == b',') {
            *i += 1;
        }
    };
    // Próximo número: termina num separador, num sinal ou num segundo ponto ("-.0486-2.6" ou ".079.2").
    let num = |i: &mut usize| -> Option<f32> {
        skip(i);
        let start = *i;
        let mut dot = false;
        if *i < b.len() && (b[*i] == b'-' || b[*i] == b'+') {
            *i += 1;
        }
        while *i < b.len() && (b[*i].is_ascii_digit() || (b[*i] == b'.' && !dot)) {
            dot |= b[*i] == b'.';
            *i += 1;
        }
        d.get(start..*i)?.parse().ok()
    };
    // Flag de arco: um só caractere, podendo vir colado no seguinte ("011").
    let flag = |i: &mut usize| -> Option<bool> {
        skip(i);
        let f = *b.get(*i)?;
        *i += 1;
        match f {
            b'0' => Some(false),
            b'1' => Some(true),
            _ => None,
        }
    };
    let mut pb = tiny_skia::PathBuilder::new();
    let (mut x, mut y, mut sx, mut sy) = (0f32, 0f32, 0f32, 0f32);
    let mut cmd = b'M';
    loop {
        skip(&mut i);
        if i >= b.len() {
            break;
        }
        if b[i].is_ascii_alphabetic() {
            cmd = b[i];
            i += 1;
        }
        let rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if rel { (x, y) } else { (0.0, 0.0) };
        match cmd.to_ascii_uppercase() {
            b'M' => {
                (x, y) = (ox + num(&mut i)?, oy + num(&mut i)?);
                (sx, sy) = (x, y);
                pb.move_to(x, y);
                // Pares seguintes ao M são L.
                cmd = if rel { b'l' } else { b'L' };
            }
            b'L' => {
                (x, y) = (ox + num(&mut i)?, oy + num(&mut i)?);
                pb.line_to(x, y);
            }
            b'H' => {
                x = ox + num(&mut i)?;
                pb.line_to(x, y);
            }
            b'V' => {
                y = oy + num(&mut i)?;
                pb.line_to(x, y);
            }
            b'C' => {
                let (x1, y1, x2, y2) = (ox + num(&mut i)?, oy + num(&mut i)?, ox + num(&mut i)?, oy + num(&mut i)?);
                (x, y) = (ox + num(&mut i)?, oy + num(&mut i)?);
                pb.cubic_to(x1, y1, x2, y2, x, y);
            }
            b'A' => {
                let (rx, ry, rot) = (num(&mut i)?, num(&mut i)?, num(&mut i)?);
                let (large, sweep) = (flag(&mut i)?, flag(&mut i)?);
                let to = (ox + num(&mut i)?, oy + num(&mut i)?);
                arc_to(&mut pb, (x, y), rx, ry, rot, large, sweep, to);
                (x, y) = to;
            }
            b'Z' => {
                pb.close();
                (x, y) = (sx, sy);
            }
            _ => return None,
        }
    }
    pb.finish()
}

/// Arco elíptico do SVG em curvas cúbicas (até 90° cada), pela conversão para centro da
/// especificação (SVG 1.1, apêndice F.6).
#[allow(clippy::too_many_arguments)]
fn arc_to(pb: &mut tiny_skia::PathBuilder, (x1, y1): (f32, f32), rx: f32, ry: f32, rot: f32, large: bool, sweep: bool, (x2, y2): (f32, f32)) {
    use std::f32::consts::{FRAC_PI_2, TAU};
    if x1 == x2 && y1 == y2 {
        return;
    }
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx == 0.0 || ry == 0.0 {
        pb.line_to(x2, y2);
        return;
    }
    let (s, c) = rot.to_radians().sin_cos();
    let (dx, dy) = ((x1 - x2) / 2.0, (y1 - y2) / 2.0);
    let (xp, yp) = (c * dx + s * dy, -s * dx + c * dy);
    let lam = xp * xp / (rx * rx) + yp * yp / (ry * ry);
    if lam > 1.0 {
        rx *= lam.sqrt();
        ry *= lam.sqrt();
    }
    let num = rx * rx * ry * ry - rx * rx * yp * yp - ry * ry * xp * xp;
    let den = rx * rx * yp * yp + ry * ry * xp * xp;
    let mut k = (num / den).max(0.0).sqrt();
    if large == sweep {
        k = -k;
    }
    let (cxp, cyp) = (k * rx * yp / ry, -k * ry * xp / rx);
    let (cx, cy) = (c * cxp - s * cyp + (x1 + x2) / 2.0, s * cxp + c * cyp + (y1 + y2) / 2.0);
    let angle = |ux: f32, uy: f32, vx: f32, vy: f32| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let (ux, uy) = ((xp - cxp) / rx, (yp - cyp) / ry);
    let start = angle(1.0, 0.0, ux, uy);
    let mut sweep_a = angle(ux, uy, (-xp - cxp) / rx, (-yp - cyp) / ry);
    if !sweep && sweep_a > 0.0 {
        sweep_a -= TAU;
    } else if sweep && sweep_a < 0.0 {
        sweep_a += TAU;
    }
    let n = (sweep_a.abs() / FRAC_PI_2).ceil().max(1.0) as usize;
    let step = sweep_a / n as f32;
    let h = 4.0 / 3.0 * (step / 4.0).tan();
    let point = |t: f32| (cx + rx * t.cos() * c - ry * t.sin() * s, cy + rx * t.cos() * s + ry * t.sin() * c);
    let deriv = |t: f32| (-rx * t.sin() * c - ry * t.cos() * s, -rx * t.sin() * s + ry * t.cos() * c);
    let mut t = start;
    for _ in 0..n {
        let t2 = t + step;
        let (p1, p2, d1, d2) = (point(t), point(t2), deriv(t), deriv(t2));
        pb.cubic_to(p1.0 + h * d1.0, p1.1 + h * d1.1, p2.0 - h * d2.0, p2.1 - h * d2.1, p2.0, p2.1);
        t = t2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iso_and_names() {
        assert_eq!(parse_iso("1970-01-02T00:00:00Z"), Some(86400));
        assert_eq!(parse_iso("2026-11-23T15:29:59.743317+00:00"), parse_iso("2026-11-23T12:29:59-03:00"));
        assert_eq!(model_name("claude-opus-5-5"), "Opus 5.5");
        assert_eq!(model_name("claude-sonnet-4-5-20250929"), "Sonnet 4.5");
        assert_eq!(model_name("gpt-6.2-sol"), "GPT 6.2 Sol");
        assert_eq!(claude_plan(Some("max"), Some("default_claude_max_5x")).as_deref(), Some("Max 5x"));
        assert_eq!(claude_plan(Some("pro"), Some("default_claude_ai")).as_deref(), Some("Pro"));
        assert_eq!(compact(110_000), "110.0K");
        for p in Provider::ALL {
            assert!(logo(p, 32, [0, 0, 0, 255]).is_some_and(|m| m.pixels().iter().any(|c| c.alpha() > 0)), "{p:?}");
        }
    }
}
