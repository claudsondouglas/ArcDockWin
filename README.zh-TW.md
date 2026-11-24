# ArcDockWin（繁體中文）

**為 Windows 11 打造的 macOS 風格 Dock 與選單列，使用 Rust 撰寫。**

ArcDockWin 以原生 Win32、Direct2D 與 Windows Composition 實作，無須額外執行階段。它可取代日常使用的 Windows 工作列，提供 Dock、頂端選單列與控制中心。

## 繁體中文與語言切換

ArcDockWin 會讀取 Windows 使用者的 UI 顯示語言，內建葡萄牙文（巴西）、英文與繁體中文（台灣）。Windows 使用中文介面時套用繁體中文，使用葡萄牙文時套用葡萄牙文，其他語言一律使用英文。程式每秒重新檢查一次 Windows 語言，偵測到變更後會更新選單列、已開啟的面板、日曆和選單，不必重新啟動。翻譯表位於 [`locales/zh-TW.tsv`](locales/zh-TW.tsv) 與 [`locales/en.tsv`](locales/en.tsv)。

## 功能

### Dock
- 預設只顯示在主螢幕；可在「設定 → Dock」改為顯示於**所有螢幕**。

- 半透明模糊玻璃底板、圓角與柔和陰影。
- 指標移過圖示時會放大；執行中的 App 會顯示 macOS 風格的小型狀態點。
- 放大效果會平滑跟隨指標，減少微小抖動；在有多個視窗的 App 圖示上滾動滑鼠滾輪可切換視窗。
- App 要求注意時圖示會跳動；開啟右鍵選單可固定／取消固定 App 或關閉視窗。
- 可拖曳圖示排序；Launcher 永遠位於 Dock 項目末端。
- Dock 尾端附有「下載項目」堆疊與垃圾桶，支援自動隱藏，並跟隨 Windows 淺色／深色模式。
- 自動隱藏時，將指標移到螢幕底部邊緣的任何位置即可叫出 Dock；「下載項目」面板開啟時 Dock 不會隱藏，再點一次堆疊即可關閉面板。
- 在 WebView2 中執行的 App（例如 WhatsApp）會顯示自己的圖示，而不是 Edge WebView 的圖示。
- 點選唯一視窗已最小化的 App 會還原該視窗。
- 可透過 `icons.txt` 將 Dock 圖示換成 MacTahoe 圖示主題中的 `.ico`。

### 最小化動畫

視窗最小化時會縮進 Dock 上對應的 App 圖示，可選「精靈」或「縮放」效果，接近圖示時逐漸淡出（60 fps）。點選視窗的最小化按鈕、按 **Win+↓**，或點選 Dock 上的 App 圖示來最小化時都會播放，Windows 11 檔案總管（含索引標籤的視窗）也支援。動畫進行期間會暫時關閉 Windows 原生的最小化動畫，結束後、程式結束時，以及 Dock 當機時由守護程序還原。

### 頂端選單列與控制中心

- 預設每個螢幕都有選單列，也可只顯示在主螢幕。
- 選單列由模組組成：CPU／記憶體、時鐘、系統匣、控制中心，以及選用的 Claude 與 Codex 用量。每個模組可開關、放在左／中／右區，並調整區內順序。
- 時鐘可選 24／12 小時制、秒數與日期；點選時鐘會開啟日曆。
- 系統匣圖示可選擇固定在選單列或收納到溢位面板；空間不足時會自動收進箭頭，CPU／記憶體最後才隱藏。
- 系統匣 App 的選單會在選單列上的圖示旁開啟；在隱藏圖示面板中按兩下圖示，行為與 Windows 工作列相同。
- 檔案總管重新啟動後，選單列會重新保留空間，最大化的視窗不會被它遮住。
- 控制中心提供 Wi‑Fi、藍牙、淺色／深色模式、電源、亮度、音量與媒體控制。

### 效能面板

點選選單列上的 CPU／記憶體，可查看 CPU、記憶體與 GPU（含顯示記憶體）使用率，以及 CPU 與 GPU 溫度，並可開啟工作管理員。GPU 與溫度只在面板開啟時讀取。CPU 溫度透過 AMD 程式庫讀取，僅支援 Ryzen APU；GPU 溫度支援 Radeon 與 NVIDIA 顯示卡；無法讀取時顯示「—」。

### Claude 與 Codex 用量

兩個選用的選單列模組，預設關閉（在「設定 → 頂端選單列」開啟），適合使用 [Claude Code](https://claude.com/claude-code) 或 [Codex](https://openai.com/codex) 的人。選單列顯示 5 小時工作階段已用的比例；點選後的面板包含：

- **限制**：5 小時工作階段與每週限制，以及各自重設前的剩餘時間。
- 最近 7 天的**每日 Token** 與**各模型 Token**，從本機的工作階段記錄讀取。
- 前往 claude.ai 或 chatgpt.com 用量頁面的連結。

限制資料來自官方 App 查詢的同一服務，使用它們已存在電腦上的登入資訊（`~/.claude/.credentials.json` 與 `~/.codex/auth.json`）。ArcDockWin 不會更新登入；登入過期時，面板會請你開啟該 App。數字每 5 分鐘更新一次。

| Claude | Codex |
| :---: | :---: |
| ![Claude 用量面板](docs/screenshots/usage-claude.png) | ![Codex 用量面板](docs/screenshots/usage-codex.png) |

### 設定

在 Dock 空白處按右鍵 → **設定…**。設定是一般視窗（Mica 背景，跟隨 Windows 淺色／深色主題），變更立即生效：

- **Dock**：圖示大小（32–72 px）、滑鼠移過時放大及放大倍率（1.1×–2.0×）、自動隱藏、顯示下載項目與垃圾桶、是否顯示於所有螢幕。
- **動畫**：開關最小化動畫，選擇精靈或縮放。
- **頂端選單列**：選單列預覽、各模組的開關／區域／順序、時鐘格式、顯示在所有螢幕。
- **一般**：隨 Windows 啟動、關於。

| Dock（淺色） | 頂端選單列（深色） |
| :---: | :---: |
| ![設定：Dock](docs/screenshots/settings-dock.png) | ![設定：頂端選單列](docs/screenshots/settings-topbar.png) |

### 檔案總管的 Tahoe 圖示

圖示套用程式會將 MacTahoe 主題延伸到 Windows 檔案總管：一般與開啟中的資料夾、桌面、文件、下載、圖片、音樂、影片、磁碟、此電腦和垃圾桶都可使用相應的主題圖示。它透過 Windows Shell 圖示登錄設定套用，並保留備份供還原；不會注入或修補 Explorer 程序。檔案總管仍保留 Windows 原生的視窗與操作方式。

## 安裝與編譯

### 使用已編譯版本

從 [GitHub Releases](https://github.com/claudsondouglas/ArcDockWin/releases/latest) 下載 `arcdockwin.exe`。建議放在固定資料夾，例如 `C:\Programas\ArcDockWin`，再啟動程式。需求：Windows 11。Windows 10 未獲正式支援，但有使用者回報可以運作；在 Windows 10 上，選單列與控制中心會改用 Segoe MDL2 Assets 圖示字型。

ArcDockWin 會自動到 GitHub 檢查新版本。有新版本時，Dock 的右鍵選單與「設定 → 一般 → 關於」會出現「更新 ArcDockWin」；按下後會下載新版本、驗證檔案並重新開啟程式。不會在你沒同意時安裝，也可以在「設定 → 一般」關閉自動檢查。

### 從原始碼編譯

需求：Windows 11、Rust MSVC 工具鏈與 Visual Studio C++ Build Tools。

```powershell
git clone https://github.com/claudsondouglas/ArcDockWin.git
cd ArcDockWin
cargo build --release
.\target\release\arcdockwin.exe
```

程式首次執行時會登錄為使用者登入後啟動。退出方式：在 Dock 空白處按右鍵，選擇「結束 ArcDockWin」；Windows 工作列會同步還原。

## 安裝 MacTahoe 圖示

1. 安裝 ImageMagick：

   ```powershell
   winget install ImageMagick.ImageMagick
   ```

2. 在新開啟的 PowerShell 執行轉換腳本：

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\icons\Converter.ps1
   ```

   預設輸出在 `C:\Icones\MacTahoe`，包含 `apps\`、`sistema\`、`aliases.txt` 和套用腳本。

3. 以系統管理員 PowerShell 套用檔案總管及桌面圖示：

   ```powershell
   cd C:\Icones\MacTahoe
   powershell -ExecutionPolicy Bypass -File .\IconesTahoe.ps1 -Testar
   powershell -ExecutionPolicy Bypass -File .\IconesTahoe.ps1 -Instalar
   ```

   `-Testar` 只預覽捷徑的圖示配對；`-Instalar` 會要求 UAC 權限，並建立登入後執行的排程工作。右下角系統匣選單可立即重新套用、設定自動套用間隔或還原 Windows 圖示。

個別 Dock 圖示也可在 `%APPDATA%\ArcDockWin\icons.txt` 指定，例如：

```text
spotify.exe=C:\Icones\MacTahoe\apps\spotify.ico
```

**Ícones Tahoe** 每次套用時都會重寫 `icons.txt`。若要保留自己的修改，請改寫在 `C:\Icones\MacTahoe\icons-custom.txt`：其中的設定優先於主題，並會在每次套用時合併進去。

圖示主題不會隨此專案一併散布；轉換腳本會從 [MacTahoe icon theme](https://github.com/vinceliuice/MacTahoe-icon-theme) 下載，並依原專案 GPL-3.0 授權。

## 設定與還原

ArcDockWin 的使用者設定位於 `%APPDATA%\ArcDockWin\`：`pinned.txt` 管理 Dock 固定項目，`icons.txt` 管理自訂圖示，`colors.db` 儲存選單列 App 色彩與系統匣顯示選項，`settings.txt` 儲存設定視窗的選項（`tray_color=1` 目前沒有介面選項，可讓系統匣圖示保留 App 原本的顏色而非灰階），`animation.txt` 儲存最小化動畫（`enabled=1|0`、`effect=genie|scale`）。

已知限制：自繪標題列的 App 以啟發式方式偵測最小化按鈕，可能沒有動畫（Win+↓ 與 Dock 圖示仍有動畫）；溫度讀取取決於硬體與驅動程式。

要還原 Explorer、捷徑及磁碟的原始圖示：

```powershell
powershell -ExecutionPolicy Bypass -File C:\Icones\MacTahoe\IconesTahoe.ps1 -Desinstalar
```

圖示腳本會從備份復原原始登錄值與捷徑圖示，並移除排程工作。

## 授權

ArcDockWin 使用 MIT License。MacTahoe 圖示主題由其作者另行提供，依 GPL-3.0 授權。ArcDockWin 受 macOS 設計啟發，與 Apple 無隸屬關係。
