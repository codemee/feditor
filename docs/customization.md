[English](customization.en.md) | **繁體中文**

[← 回到 README](../README.md)

# 自訂與設定檔

## 介面語言

**介面語言**是設定面板裡的三態按鈕：「跟隨系統」（地球）、「中文」（「中」）、「English」（「En」），點一下依序切換，立刻生效，不用重開。「跟隨系統」依作業系統的語言決定：中文系統用中文，其他一律用英文。設定存在設定檔的 `language`（`system`／`zh`／`en`）。目前翻譯的是介面文字、提示與訊息；README 之類的文件與錄製出來的巨集檔內的註解不受影響。

## 配色

- **外觀**：一個三態狀態鈕，圖示隨目前的狀態變化（◐ 跟隨系統、☀ 淺色、☾ 深色），點一下依序切換（跟隨系統 → 淺色 → 深色 → 跟隨系統）。
- **配色**：**淺色與深色各自選一套**，互不影響（例如淺色用 GitHub Light、深色用 Dracula），外觀切換或跟隨系統時就用對應那一套。選擇會記住。內建配色：
  - 深色：EVA（預設，《新世紀福音戰士》的色票：帶紫調的近黑底、初號機紫、螢光綠、NERV 橘）、Dracula、Monokai、Solarized Dark、One Dark、Nord、Gruvbox Dark、Tokyo Night、Catppuccin Mocha／Macchiato／Frappé。
  - 淺色：預設淺色（沿用原本的外觀）、Solarized Light、GitHub Light、One Light、Gruvbox Light、VS Code Light+、Catppuccin Latte。
  - 內建配色是依這些知名配色的色票調整的，並確認文字與各種底色的對比（一般文字至少 4.5:1），所以不一定和原版逐字相同。

## 自訂配色

- **自訂配色**：設定面板的「編輯目前配色…」會開啟編輯視窗，編輯的是**目前正在使用的主題**（深色或淺色，要編輯另一邊請先切換外觀）所選的那一套。每種顏色（背景、文字、按鈕與邊線、強調與狀態、語法上色、終端機的 16 色，共 41 種）一個色塊，改了立刻預覽，「還原」放棄修改。內建配色不能覆寫，用「另存為…」取名存成自訂配色（會自動設為這個主題使用的配色）；自訂配色可以直接「儲存」。
  自訂配色存在設定資料夾下的 `themes` 資料夾（Windows `%APPDATA%\feditor\themes\`，設定面板的「配色資料夾」按鈕可以直接開啟），一個 `*.conf` 檔一套，**打開設定面板時會重新讀取**，所以也可以自己用文字編輯器寫、或從別人那裡拿來放進去。格式是 `key = value`（`#` 開頭的整行是註解，顏色後面可以接「  # 說明」）：

  | 設定 | 說明 |
  |---|---|
  | `name` | 配色名稱（必填）。與內建配色同名時，自訂的會蓋過內建的 |
  | `mode` | `dark` 或 `light`：這套配色出現在深色還是淺色的清單（沒寫就是 `dark`） |
  | `base` | 可省略：沒寫到的顏色從這套現成的配色繼承（內建或別的自訂配色都可以）；沒有 `base` 就用該主題預設配色補齊 |
  | `editor_bg`、`panel_bg`、`popup_bg`、`faint_bg`、`code_bg` | 編輯區、面板、浮動視窗、淡色（交錯）、程式碼的背景 |
  | `text`、`text_strong`、`text_weak` | 一般、強調、次要文字 |
  | `button`、`button_hover`、`button_active`、`selection`、`line`、`line_hover` | 按鈕（一般、滑鼠移過、按下）、選取範圍、邊線（一般、滑鼠移過） |
  | `accent`、`cursor`、`link`、`warning`、`error` | 強調色、游標、連結、警告（也用在搜尋結果的高亮）、錯誤 |
  | `syn_comment`、`syn_string`、`syn_number`、`syn_keyword`、`syn_type`、`syn_function` | 語法上色：註解、字串、數字、關鍵字、型別、函式 |
  | `term_black`、`term_red`、`term_green`、`term_yellow`、`term_blue`、`term_magenta`、`term_cyan`、`term_white`，以及加上 `bright_` 的亮色版本（如 `term_bright_red`） | 終端機窗格的 16 色（ANSI 色號 0–15）；預設文字色用 `text`，底色用 `editor_bg` |

  顏色寫成 `#RRGGBB`（或 `#RGB`）。壞掉的檔案（缺 `name`、不認得的設定、顏色寫錯、`base` 找不到）會略過，並在輸出面板顯示原因。範例見 `examples/themes/midnight.conf`。設定檔裡記的配色不存在（例如檔案被刪了）時退回該主題的預設配色。

## 字型

- **字型**：只列出**等寬字型**。判斷方式是比對 `i`、`m`、`W`、`.` 等字元的字寬是否一致，不看字型檔裡常常標錯的等寬旗標。預設是**作業系統預設的等寬字型**（向系統查詢，不寫死名稱；Windows 上是 Consolas）。
  有些老字型（細明體、MS Gothic、Lucida Console 等）的拉丁字母是內嵌的彩色點陣，egui 不會依文字顏色上色，畫出來看不見，所以這類字型不會出現在清單裡；設定檔若存了這種字型，會自動退回系統預設，面板顯示的也是實際生效的字型。
  清單要逐一開啟字型檔檢查，所以在背景掃描，掃完前面板會顯示「正在找出系統上的等寬字型…」。中文字形用微軟正黑體補。每個字型的上下緣比例不同，egui 預設會讓拉丁字母和中文字的基準線錯開，所以啟動與換字型時會用 egui 的實際排版量出落差並補償；測試對清單裡的每一個字型都驗證過兩者落在同一條基準線上。
- **字級**：面板的字級欄與快捷鍵共用同一個設定值，所以兩邊永遠同步。快捷鍵：**Ctrl ＋／－** 放大縮小、**Ctrl＋滾輪**、**Ctrl＋0** 還原預設（範圍 8–48 點）。這取代了 egui 內建的「Ctrl ＋／－ 縮放整個介面」。

## 設定檔

- **設定檔**位置：Windows `%APPDATA%\feditor\settings.conf`、macOS `~/Library/Application Support/feditor/settings.conf`、Linux `$XDG_CONFIG_HOME/feditor/settings.conf`（沒設定時是 `~/.config/feditor/settings.conf`）。內容是簡單的 `key = value`：`theme`（`system`／`light`／`dark`）、`font_family`（沒有這行表示系統預設）、`ruler`（是否啟用安全界線，`true`／`false`，預設 `true`）、`ruler_column`（安全界線的欄數，1 到 500，預設 80；沒啟用時也會記著。舊版寫成 0 的視為沒啟用）、`tab_width`（Tab 欄數，1 到 16，預設 4）、`tab_width.<語言名稱>`（某個檔案類型自己的 Tab 欄數，例如 `tab_width.Python = 4`；語言名稱就是語法上色的名稱）、`tab_spaces`（輸入 Tab 時是否改用空白，`true`／`false`，預設 `false`，所有檔案類型共用）、`font_size`（編輯區）、`preview_font_size`（預覽）、`terminal_font_size`（終端機）、`terminal_min_contrast`（終端機文字與底色的最低對比度，1 是不調整，最大 7，預設 3）、`terminal_shell`（預設終端機：工具列的終端機鈕新開的終端機要執行的程式與參數，設定面板的清單選的就是寫在這裡的指令，例如 `pwsh -NoLogo`；路徑有空白要用雙引號包起來；沒有這行表示系統預設）、`show_line_numbers`／`highlight_line`／`show_whitespace`／`syntax`／`word_wrap`（`true`／`false`）。檔案不存在、壞掉、或值不合理時都退回預設，不會讓程式起不來。字級連續變動時會等停下來 0.4 秒才寫入，關閉視窗時一定會寫。

## 自訂語法

- **自訂語言**：在設定資料夾下的 `syntax` 資料夾（Windows `%APPDATA%\feditor\syntax\`）放 `*.conf`，一個檔案一種語言，重新啟動後生效；自訂語言優先於內建語言（可以用來改掉內建的關鍵字）。壞掉的檔案會略過，並在輸出面板顯示原因。範例見 `examples/syntax/php.conf`。內容是 `key = value`，`#` 開頭的整行是註解：

  | 設定 | 說明 |
  |---|---|
  | `name` | 語言名稱（必填） |
  | `extensions` | 副檔名，逗號或空白分隔；也可以寫完整檔名，例如 `makefile`（必填） |
  | `line_comment` | 行註解符號，可以多個，例如 `//, #` |
  | `block_comment` | 區塊註解的開頭與結尾，例如 `/*, */` |
  | `quotes` | 字串的引號字元，例如 `", '`（沒寫就是 `"` 與 `'`） |
  | `keywords` | 關鍵字，逗號或空白分隔，可以寫多行（會合併） |
  | `capital_types` | `true` 時，大寫開頭的識別字標成型別 |
  | `case_insensitive` | `true` 時，關鍵字不分大小寫（SQL） |
  | `triple_quotes`／`backtick_strings` | 三引號字串（Python）、反引號字串（JavaScript），可跨行 |
  | `interpolation` | 字串內嵌運算式的開頭與結尾，例如 `${, }`（JavaScript）、`#{, }`（Ruby）、`{, }`（Python f-string） |
  | `interpolation_in` | 哪些字串裡有內嵌運算式，逗號分隔：`backtick`（反引號字串）、`double`（雙引號字串）、`fstring`（帶 `f` 或 `$` 前綴的字串，`{{` 視為跳脫）。沒寫就是 `backtick, double` |
  | `indent` | 縮排規則（Enter 自動縮排與排列格式用）：`none`（預設，只沿用上一行的縮排）、`brace`（依 `{ ( [` 與 `} ) ]`）、`python`（行尾 `:` 也開啟一層） |
  | `mode` | `code`（預設）、`markup`（HTML/XML）、`markdown`、`config`（ini／toml 的 `[區段]` 與「鍵 =」） |
