<div align="center">

# ArcDockWin

[Português (Brasil)](README.pt-BR.md) · [繁體中文](README.zh-TW.md)

**A macOS-style dock and menu bar for Windows 11, written in Rust.**

Lightweight (a single `.exe`, no runtime), native (Win32 + Direct2D + Composition) and built to replace the
Windows taskbar in daily use.

![ArcDockWin](docs/screenshots/desktop.png)

</div>

---

> **A personal project.** ArcDockWin is a hobby project I build for my own use and share in case it is useful to
> someone else. It is not a commercial product: there is no support or warranty, and things may change or break
> between versions. Issues and pull requests are welcome, but I answer on my own time.

## ✨ Features

### Dock
- Shown on the main monitor by default; can be shown on **every monitor** in Settings → Dock.
- Real blurred glass (what is behind the dock shows through blurred), rounded corners and a soft shadow.
- Icon magnification on hover and a small dot under running apps.
- Smooth magnification follows pointer movement; use the mouse wheel over an app with multiple windows to cycle through them.
- **Bouncing icon** when an app asks for attention, with a red dot until you open the app.
- **Drag to reorder**: drop a running app between the pinned ones to pin it, or drag it upwards to unpin it.
  The launcher is always the last app.
- **Trash and Downloads** at the end of the dock, with a panel of recent files.
- Hides itself when a window touches it; with auto-hide on, moving the mouse to any point of the bottom edge
  brings it back. It stays visible while the Downloads panel is open, and clicking the stack again closes the panel.
- Apps that run inside WebView2 (such as WhatsApp) show their own icon instead of the Edge WebView one.
- Clicking an app whose only window is minimized restores it.
- Follows the Windows **Dark/Light mode**.

### Minimize animation
Windows shrink into their app's icon on the dock with a **Genie** or **Scale** effect, fading out as they get close
to the icon, at 60 fps. It works when you click a window's minimize button, press **Win+↓**, or click the app's
icon on the dock to minimize it. It also works on Windows 11 File Explorer windows (the ones with tabs).

To avoid a double animation, ArcDockWin turns off the native Windows minimize animation for the short moment its own
animation runs. The original value is restored afterwards, when ArcDockWin quits, and by the watchdog if the dock
crashes.

### Top bar
- Shown on **every monitor** by default; can be limited to the main monitor in Settings → Top bar.
- **Takes the colour of the focused app** (10% darker, to separate the bar from the app), the moment you switch windows.
- Built from **modules**: CPU/RAM, Clock, Tray, Control Center and the optional Claude and Codex usage. Each one can
  be turned off, placed in the left, centre or right zone, and ordered inside its zone.
- Clock with a calendar when clicked; 24 h or 12 h, optional seconds and date.
- Tray icons with a panel for hidden icons and a choice of which ones stay on the bar. When the bar runs out of
  room, tray icons fold into the arrow on their own (your choice of visible icons is kept) and CPU/RAM is the last
  thing to go.
- The item whose panel is open stays highlighted.
- Tray app menus open next to their icon on the top bar, and double-clicking an icon in the hidden-icons panel works
  like on the Windows taskbar.
- If Explorer restarts, the top bar reserves its space again, so maximized windows do not slide under it.

### Performance panel
Click CPU/RAM on the top bar to see CPU, memory and GPU usage (with video memory) and the CPU and GPU temperatures,
plus a button to open Task Manager. GPU and temperature readings only run while the panel is open.

Temperatures depend on the hardware: the CPU temperature comes from the AMD library and only works on Ryzen APUs;
the GPU temperature works on Radeon (AMD) and NVIDIA cards. Anything that cannot be read shows "—".

![Performance panel](docs/screenshots/performance.png)

### Claude and Codex usage
Two optional top bar modules, off by default (turn them on in Settings → Top bar), for people who use
[Claude Code](https://claude.com/claude-code) or [Codex](https://openai.com/codex). The bar shows how much of the
5-hour session has been used, and a click opens a panel with:
- **Limits**: the 5-hour session and the weekly limits, with the time left until each one resets.
- **Tokens per day** over the last 7 days and **tokens per model**, read from the local session logs.
- A link to the usage page on claude.ai or chatgpt.com.

The limits come from the same service the official apps query, using the login they already saved on the PC
(`~/.claude/.credentials.json` and `~/.codex/auth.json`). ArcDockWin never renews that login: when it expires, the
panel asks you to open the app. The numbers refresh every 5 minutes.

| Claude | Codex |
| :---: | :---: |
| ![Claude usage panel](docs/screenshots/usage-claude.png) | ![Codex usage panel](docs/screenshots/usage-codex.png) |

### Control Center
macOS Tahoe look, with rounded cards:
- Wi‑Fi / Network, Bluetooth, Dark Mode and Power (battery, shut down, restart…).
- **Now playing** (cover, title and buttons for any player that talks to Windows: Spotify, browser…).
- **Brightness** through DDC/CI for the monitor of the bar you clicked, and **volume**.

### Settings
Right-click an empty spot on the dock → **Settings…** It opens as a normal window (title bar, Alt+Tab, icon on the
dock) with a Mica background that follows the Windows light/dark theme. Changes apply immediately.

| Page | Options |
| --- | --- |
| Dock | Icon size (32–72 px), magnification on hover on/off and its size (1.1×–2.0×), auto-hide, show Downloads and Trash, show on all monitors |
| Animations | Minimize animation on/off, Genie or Scale effect |
| Top bar | Live preview of the bar; each module on/off, zone (Left/Centre/Right) and order (▲▼); clock format (24 h, seconds, date); show on all monitors |
| General | Start with Windows; About |

| Dock page (light) | Top bar page (dark) |
| :---: | :---: |
| ![Settings, Dock page](docs/screenshots/settings-dock.png) | ![Settings, Top bar page](docs/screenshots/settings-topbar.png) |

The same bar with the clock moved to the left and CPU/RAM to the right:

![Top bar with rearranged modules](docs/screenshots/topbar-modules.png)

### Languages
ArcDockWin follows the Windows display language: Portuguese (Brazil), English and Traditional Chinese (Taiwan) are
built in. Windows in Portuguese gets Portuguese, Windows in Chinese gets Traditional Chinese, and any other language
falls back to English. The language is re-checked while the app runs, so changing it needs no restart. The
translation tables are [`locales/en.tsv`](locales/en.tsv) and [`locales/zh-TW.tsv`](locales/zh-TW.tsv). Other
languages are welcome through pull requests.

### Safety
- The Windows taskbar stays hidden while ArcDockWin runs, even when Windows tries to show it again (when you take a
  screenshot, for example).
- A **watchdog** process restores the Windows taskbar if ArcDockWin crashes or is killed.

## 📸 Screenshots

| Control Center | Dock |
| :---: | :---: |
| ![Control Center](docs/screenshots/central.png) | ![Dock](docs/screenshots/dock.png) |

| Top bar | Launcher |
| :---: | :---: |
| ![Top bar](docs/screenshots/topbar.png) | ![Launcher](docs/screenshots/launcher.png) |

## 🚀 Installation

### Download a build

1. Download `arcdockwin.exe` from the [latest release](https://github.com/claudsondouglas/ArcDockWin/releases/latest)
   (or the `.zip`, which also contains the icon scripts).
2. Put it in a permanent folder (for example `C:\Programs\ArcDockWin`) and run it.

> The `.exe` is not digitally signed, so Windows may show **"Windows protected your PC"**.
> Click **More info → Run anyway**.

### Updates

ArcDockWin checks GitHub for new versions on its own. When one is out, **Update ArcDockWin** shows up in the dock's
right-click menu and in Settings → General → About. One click downloads the new version, verifies the file and
restarts the app. Nothing is installed until you ask, and you can turn the check off in Settings → General.

Requirement: **Windows 11**. Windows 10 is not officially supported, but users report that it works; the menu bar and control panel fall back to the Segoe MDL2 Assets icon font there.

### Build from source

Requirements: **Windows 11** and [Rust](https://rustup.rs) (MSVC toolchain).

```powershell
git clone https://github.com/claudsondouglas/ArcDockWin.git
cd ArcDockWin
cargo build --release
.\target\release\arcdockwin.exe
```

On the first run ArcDockWin registers itself to **start with Windows** (in `HKCU\...\Run`), pointing to the `.exe`
where it currently is, so do not leave it in the Downloads folder. You can turn this off in Settings → General.

### How to quit
Right-click an empty spot on the dock → **Quit ArcDockWin**. The Windows taskbar comes back immediately.

> If something goes wrong and the Windows taskbar does not come back, open ArcDockWin again and quit from the menu,
> or restart Explorer (Task Manager → Windows Explorer → Restart).

### Configuration
Everything lives in `%APPDATA%\ArcDockWin\`:

| File | What it stores |
| --- | --- |
| `pinned.txt` | Apps pinned to the dock, one path per line (you can edit it by hand). |
| `icons.txt` | Per-app icon overrides (`name.exe=C:\path\icon.ico`), see below. |
| `colors.db` | Each app's colour on the top bar and which tray icons are visible (SQLite). |
| `settings.txt` | Options from the Settings window (`key=value`, e.g. `icon_size=48`, `mod.clock=center,0,on`). Missing keys use the defaults. `tray_color=1` (no UI yet) keeps the app colors in tray icons instead of grayscale. |
| `animation.txt` | Minimize animation: `enabled=1` or `0`, `effect=genie` or `scale`. |

### Known limitations
- Apps that draw their own title bar (no standard minimize button) are detected by a heuristic, so clicking their
  "_" may minimize without the animation. Win+↓ and the dock icon still animate.
- Temperatures depend on the hardware and drivers (see the Performance panel above).
- On a very narrow bar, tray icons and then CPU/RAM are hidden to make room.

## 🎨 macOS icons (MacTahoe)

The dock uses each app's own icon by default. To give everything a Mac look, the [`icons/`](icons) folder has two
scripts that apply the [**MacTahoe**](https://github.com/vinceliuice/MacTahoe-icon-theme) theme by
[vinceliuice](https://github.com/vinceliuice) to the dock **and to all of Windows**: desktop and Start menu
shortcuts, well-known Explorer folders (Desktop, Documents, Downloads, Pictures, Music and Videos), drives, Recycle
Bin and This PC. The script uses Windows Shell customisation points and saves a backup so the original associations
can be restored.

![MacTahoe icons](docs/screenshots/icones.png)

### 1. Install ImageMagick

```powershell
winget install ImageMagick.ImageMagick
```

Then **close and reopen the terminal** so the `magick` command exists.

### 2. Download and convert the icons

```powershell
powershell -ExecutionPolicy Bypass -File .\icons\Converter.ps1
```

The script downloads the theme, converts the ~1400 icons from SVG to `.ico` (about 1 minute) and builds everything
in `C:\Icones\MacTahoe`:

```
C:\Icones\MacTahoe\
├── apps\          app icons (google-chrome.ico, spotify.ico, …)
├── sistema\       folders, Recycle Bin, drives and This PC
├── aliases.txt    alternative names for each icon
└── IconesTahoe.ps1
```

> Use that folder: the dock's Trash and Downloads icons are read from `C:\Icones\MacTahoe\sistema`.

### 3. Apply

```powershell
cd C:\Icones\MacTahoe
powershell -ExecutionPolicy Bypass -File .\IconesTahoe.ps1 -Testar     # only shows which icon each shortcut will get
powershell -ExecutionPolicy Bypass -File .\IconesTahoe.ps1 -Instalar   # applies and stays in the tray
```

`-Instalar` asks for administrator permission (folders and drives are system settings) and creates a scheduled task
that opens **Ícones Tahoe** in the tray at every logon. It re-applies the icons from time to time, to catch new apps
and shortcuts that updates recreate. From the tray menu you can:

- **Apply now** (or double-click the icon);
- choose the interval under **Apply automatically** (1 h to 24 h, or off);
- **Restore Windows icons**.

The dock gets the icons through `%APPDATA%\ArcDockWin\icons.txt`, which the script writes. The Start button gets
the app-grid icon.

### Wrong or missing icon?

The theme was made for Linux, so the script looks for the icon by the `.exe` name, by the shortcut name and by the
theme's aliases. When a program's name on Windows does not match, add a line to the `$Extra` table at the top of
`IconesTahoe.ps1`:

```powershell
$Extra = @{
  msedge = 'microsoft-edge'   # .exe name (without .exe) = icon name in apps\ (without .ico)
  code   = 'visual-studio-code'
  # ...
}
```

To change the icon of a single app on the dock, without the script, edit `%APPDATA%\ArcDockWin\icons.txt`:

```
spotify.exe=C:\Icones\MacTahoe\apps\spotify.ico
```

**Ícones Tahoe** rewrites `icons.txt` every time it applies the icons. To keep your own changes, put the same lines
in `C:\Icones\MacTahoe\icons-custom.txt` instead: they take priority over the theme and are merged in on every run.

### Undo

```powershell
powershell -ExecutionPolicy Bypass -File C:\Icones\MacTahoe\IconesTahoe.ps1 -Desinstalar
```

This brings back all the original icons (the script keeps a backup of what it changed) and removes the scheduled
task.

## 🛠️ How it works

| Part | Technology |
| --- | --- |
| Drawing | [tiny-skia](https://github.com/RazrFalcon/tiny-skia) + Direct2D/DirectWrite, layered windows (`UpdateLayeredWindow`) |
| Glass | `Windows.UI.Composition` (DWM backdrop) |
| Space reservation | AppBar (`SHAppBarMessage`) |
| Windows and focus | `SetWinEventHook` and `RegisterShellHookWindow` |
| Media | `Windows.Media.Control` (GSMTC) |
| Brightness | DDC/CI (`dxva2`) |
| Colours and tray | Windows' own SQLite (`winsqlite3.dll`) |
| Minimize animation | Low-level mouse/keyboard hooks to grab the window before it minimizes (no DLL injection) |
| Performance | PDH counters, DXGI, AMD ADL (`atiadlxx.dll`) and NVIDIA NVML (`nvml.dll`), loaded only while the panel is open |

## 🙏 Credits

- Icons: [MacTahoe icon theme](https://github.com/vinceliuice/MacTahoe-icon-theme) by vinceliuice (GPL-3.0). They
  are not part of this repository: `Converter.ps1` downloads them straight from the original project.
- Traditional Chinese translation and Explorer folder icons: [popuchoco](https://github.com/popuchoco)
  ([#1](https://github.com/claudsondouglas/ArcDockWin/pull/1)). Thank you!
- Idea for the taskbar safety net: [Lemxy/WinRust](https://github.com/Lemxy/WinRust).
- Inspired by macOS. Not affiliated with Apple.

## 📄 License

[MIT](LICENSE). The MacTahoe icons, downloaded separately by `Converter.ps1`, follow the GPL-3.0 license of the
original project.
