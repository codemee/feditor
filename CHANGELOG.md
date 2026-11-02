[English](CHANGELOG.en.md) | **繁體中文**

# 更新紀錄

## [0.1.2] - 2026-11-02

### 新增
- **配色**：淺色與深色各自選一套。內建深色 11 套（EVA、Dracula、Monokai、Solarized Dark、One Dark、Nord、Gruvbox Dark、Tokyo Night、Catppuccin Mocha／Macchiato／Frappé）、淺色 7 套（預設淺色、Solarized Light、GitHub Light、One Light、Gruvbox Light、VS Code Light+、Catppuccin Latte）。
- **自訂配色**：設定面板的「編輯目前配色…」可以逐一調整 25 種顏色並即時預覽，另存成 `themes\*.conf`；也可以自己寫配色檔，打開設定面板時重新讀取。
- **語法上色**：
  - 可在狀態列選語言；沒有檔名的文件會依內容判斷（shebang、`<?php`、HTML／XML、JSON）。
  - 可用 `syntax\*.conf` 自訂語言。
  - 字串裡的內嵌運算式（JavaScript 樣板字串、Python f-string、Shell）會當成程式碼上色。
- **拖放開檔**：把檔案從檔案總管拖進視窗，多個檔案各開一個分頁。
- **雙按分頁重新命名檔案**（沒存過的分頁則開「另存新檔」）。
- **錄製巨集後先問名稱**；巨集面板可以重新命名巨集（改檔頭的 `@name`）。
- **輸出面板可收合**，預設收合，只顯示最後一則訊息；錯誤或巨集輸出出現時暫時彈開。
- 編碼清單把 BOM 併入成獨立的選項（UTF-8 BOM、UTF-16LE BOM…），不再有單獨的 BOM 按鈕。
- 對話框（儲存、重新載入、刪除、改名）改成程式內的扁平對話框，不再用系統的。

### 修正
- Windows 版不再在背後多開一個黑色的主控台視窗。
- 用快捷鍵（例如 Alt+M）執行巨集時，不會多打出那個字母。
- 「在檔案中尋找」的圖示補上資料夾底邊。
- 分頁的 × 整合進分頁、滑鼠移上去才出現；分頁、狀態列、下拉清單 hover 時不再畫外框，清單的捲軸不會壓到選項底色。

## [0.1.1] - 2026-11-01

只改了 macOS 的打包方式，程式本身的功能與 0.1.0 完全相同。

- macOS 改成 `.dmg`：`feditor-macos.dmg` 打開後，把 `FEditor.app` 拖到「應用程式」捷徑即可安裝（0.1.0 只是裸的執行檔）。
- `FEditor.app` 有自己的圖示與 `Info.plist`，並做了 ad-hoc 簽章（沒有 Apple 公證，只有 Apple Silicon 版）。
- CI 會在 macOS 上檢查 `.app` 與 `.dmg` 是否打包完整。
- README 新增「下載」一節。

## [0.1.0] - 2026-11-01

第一個公開版本。

- 編輯：自繪編輯區（測過 346 MB、400 萬行）、復原／重做、自動換行、行號、目前行底色、顯示空白字元、行操作、多重游標、矩形選取。
- 多分頁與工作階段還原；外部程式修改檔案時偵測並提示。
- 檔案格式：自動偵測編碼；存檔可選 UTF-8（含／不含 BOM）、Big5、GBK、Shift_JIS、EUC-KR、UTF-16；換行格式 LF／CRLF／CR。
- 搜尋：尋找／取代（規則表達式為線性時間）、背景執行的「在檔案中尋找」。
- 語法上色：14 類語言、字串內嵌運算式、狀態列選語言、沒有檔名時依內容判斷、可用 `syntax/*.conf` 自訂語言。
- JavaScript 巨集：QuickJS 引擎（有時間與記憶體上限）、錄製、巨集面板、編輯器 API。
- 介面：圖示工具列、淺色／深色（EVA 配色）、只列等寬字型、輸入法支援（Windows 驗證過）。

[0.1.2]: https://github.com/codemee/feditor/releases/tag/v0.1.2
[0.1.1]: https://github.com/codemee/feditor/releases/tag/v0.1.1
[0.1.0]: https://github.com/codemee/feditor/releases/tag/v0.1.0
