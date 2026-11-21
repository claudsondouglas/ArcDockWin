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
- Real blurred glass (what is behind the dock shows through blurred), rounded corners and a soft shadow.
- Icon magnification on hover and a small dot under running apps.
- **Bouncing icon** when an app asks for attention, with a red dot until you open the app.
- **Drag to reorder**: drop a running app between the pinned ones to pin it, or drag it upwards to unpin it.
  The launcher is always the last app.
- **Trash and Downloads** at the end of the dock, with a panel of recent files.
- Hides itself when a window touches it.
- Follows the Windows **Dark/Light mode**.

### Top bar
- Shown on **every monitor** (the dock stays on the main one only).
- **Takes the colour of the focused app** (10% darker, to separate the bar from the app), the moment you switch windows.
- An ArcDockWin app menu on the left for the launcher, Control Center, Task Manager and quit; its command list supports arrow-key navigation and Enter.
- Clock, CPU/RAM and notification icons are grouped on the right; clicking the clock opens the calendar and status panels stay highlighted while open.
- Tray icons collapse into an overflow panel when space is tight, and you can choose which icons stay visible.

### Control Center
macOS Tahoe look, with rounded cards:
- Wi‑Fi / Network, Bluetooth, Dark Mode and Power (battery, shut down, restart…).
- **Now playing** (cover, title and buttons for any player that talks to Windows: Spotify, browser…).
- **Brightness** through DDC/CI for the monitor of the bar you clicked, and **volume**.

### Languages
ArcDockWin follows the Windows display language. Portuguese (Brazil) is the default; when Windows is set to
Chinese, the built-in Traditional Chinese (Taiwan) pack is used. The language is re-checked while the app runs, so
changing it needs no restart. The translation table is [`locales/zh-TW.tsv`](locales/zh-TW.tsv). Other languages
are welcome through pull requests.

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

Requirement: **Windows 11**.

### Build from source

Requirements: **Windows 11** and [Rust](https://rustup.rs) (MSVC toolchain).

```powershell
git clone https://github.com/claudsondouglas/ArcDockWin.git
cd ArcDockWin
cargo build --release
.\target\release\arcdockwin.exe
```

On the first run ArcDockWin registers itself to **start with Windows** (in `HKCU\...\Run`), pointing to the `.exe`
where it currently is, so do not leave it in the Downloads folder.

### How to quit
Right-click an empty spot on the dock → **Sair do ArcDockWin** (Quit ArcDockWin). The Windows taskbar comes back
immediately.

> If something goes wrong and the Windows taskbar does not come back, open ArcDockWin again and quit from the menu,
> or restart Explorer (Task Manager → Windows Explorer → Restart).

### Configuration
Everything lives in `%APPDATA%\ArcDockWin\`:

| File | What it stores |
| --- | --- |
| `pinned.txt` | Apps pinned to the dock, one path per line (you can edit it by hand). |
| `icons.txt` | Per-app icon overrides (`name.exe=C:\path\icon.ico`), see below. |
| `colors.db` | Each app's colour on the top bar and which tray icons are visible (SQLite). |

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
