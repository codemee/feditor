[English](CHANGELOG.en.md) | **繁體中文**

# 更新紀錄

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

[0.1.1]: https://github.com/codemee/feditor/releases/tag/v0.1.1
[0.1.0]: https://github.com/codemee/feditor/releases/tag/v0.1.0
