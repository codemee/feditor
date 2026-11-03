[English](development.en.md) | **繁體中文**

[← 回到 README](../README.md)

# 開發指南

## 專案結構

| 路徑 | 說明 |
|---|---|
| `core/` | `feditor-core`：文字緩衝區（ropey）、undo／redo、檔案編碼、搜尋、折行、跨檔搜尋、介面語言（中英文對照表）。與 UI、巨集引擎無關 |
| `script/` | `feditor-script`：與語言無關的巨集 API（`EditorApi`、`ScriptEngine`） |
| `script-js/` | `feditor-script-js`：JavaScript 巨集引擎（QuickJS，經由 rquickjs） |
| `ui/` | `feditor`：egui 前端，含自繪的編輯區（`ui/src/editor/`）、分頁與分割窗格（`panes.rs`、`pane_ops.rs`）、預覽（`preview*.rs`）、終端機（`terminal*.rs`）、檔案監看（`watcher.rs`）、巨集庫、語法上色、工作階段 |
| `examples/` | 範例巨集（`macros/`）與不同編碼的測試檔（`files/`） |
| `proto/` | 獨立的 IME 驗證原型（egui、iced、Slint），不屬於主 workspace |

## 設計決策

- **核心用 Rust**：效能、單一執行檔、跨平台發佈。
- **UI 用 egui 自繪**：繁體中文 IME 已在 egui 上驗證通過（見 `proto/IME-TEST.md`）。編輯區不用 egui 的 `TextEdit`，而是自繪（rope 緩衝區＋只繪製看得到的行），原因是 `TextEdit` 對大檔案不夠快、復原與輸入法組字的行為也無法細部控制。
- **巨集語言先選 JavaScript**（`rquickjs`）：輕量、可靜態連結、容易設定執行時間與記憶體上限。透過 `ScriptEngine` 抽象，日後要加 Python 等其他語言不需改動核心。
- **檔案大小目標**：幾百 MB 以內。若日後需要更大，再優化緩衝區與渲染。

## 建置與測試

```powershell
cargo test --workspace                          # 全部單元測試
cargo run --release -p feditor                  # 啟動編輯器
```

- **需要的工具**：Rust（stable）。Windows 需要 MSVC 工具集與 Windows SDK（安裝「Build Tools for Visual Studio」的 C++ 工作負載即可）；Linux 需要的系統套件列在 `.github/workflows/ci.yml` 裡，其他發行版可能要調整。
- **測試**：UI 的測試包含用無頭的 egui 驅動整個 `App`（分頁、窗格、拖放、工作階段、預覽、終端機…）；終端機有一個測試會真的開 shell。測試會掃描原始碼確認翻譯齊全（見[編輯器內部運作](internals.md)）。
- **雙語文件**：文件（README、CHANGELOG、docs 下的檔案）一律同時提供繁體中文與英文兩個版本，英文版加 `.en` 後綴，兩份檔案開頭放語言切換連結，修改其中一份要同步另一份。
- **應用程式圖示**（文字文件加鉛筆）由 `ui/assets/make_icon.py` 產生：`uv run ui/assets/make_icon.py`。
- **終端機顏色檢查**：在終端機裡執行 `examples/terminal-colors.ps1`（可用 `-Only 1,2` 只跑指定段落）。

## 發佈

- GitHub Actions 設定在 `.github/workflows/`：推到 `main` 時 CI 在 Windows、macOS、Linux 都跑測試與建置；推送 `vX.Y.Z` 標籤會觸發 Release 工作流程，產生 `feditor-windows-x64.zip`、`feditor-macos.dmg`、`feditor-linux-x64.tar.gz`。
- 發佈新版本：更新 `Cargo.toml` 的版本號（工作區與各 crate 之間的相依版本）、把 CHANGELOG 的「未發布」改成新版本與日期並補上連結、等 CI 通過、打標籤、再為 Release 補上中英文的版本說明。
- macOS 版沒有 Apple 公證，只有 ad-hoc 簽章，且只有 Apple Silicon 版；CI 會檢查 `.app` 與 `.dmg` 是否打包完整。

## 開發環境注意事項

- 有一台開發機器的 wgpu **Vulkan 後端會崩潰**（`0xC0000005`）。Windows 上預設改用 DX12，可用環境變數 `WGPU_BACKEND` 覆寫。
- 輸入法只在 Windows + DX12 驗證過。輸入法行為異常時，可設定環境變數 `FEDITOR_IME_LOG=記錄檔路徑` 再啟動，編輯區會把輸入法事件與當時的游標、組字狀態追加寫入該檔案（沒設定就不會寫任何東西）。
- Windows 的 PATH 上如果有別的 `link.exe`（例如 coreutils 的）排在 MSVC 前面，Rust 的連結會失敗；要讓 MSVC 的優先，或在「開發人員命令提示字元」（vcvars）裡編譯。
- `proto/` 是獨立的輸入法驗證原型（egui、iced、Slint），不屬於主 workspace，結果記在 `proto/IME-TEST.md`。
