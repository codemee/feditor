[English](README.en.md) | **繁體中文**

# FEditor

以 EmEditor 為觀摩對象、支援 scripting 巨集的跨平台文字編輯器。以 Rust 實作。

## 專案結構

| 路徑 | 說明 |
|---|---|
| `core/` | `feditor-core`：文字緩衝區（ropey）、undo／redo、檔案編碼、搜尋、折行、跨檔搜尋。與 UI、巨集引擎無關 |
| `script/` | `feditor-script`：與語言無關的巨集 API（`EditorApi`、`ScriptEngine`） |
| `script-js/` | `feditor-script-js`：JavaScript 巨集引擎（QuickJS，經由 rquickjs） |
| `ui/` | `feditor`：egui 前端，含自繪的編輯區（`ui/src/editor/`）、分頁、巨集庫、語法上色、工作階段 |
| `examples/` | 範例巨集（`macros/`）與不同編碼的測試檔（`files/`） |
| `proto/` | 獨立的 IME 驗證原型（egui、iced、Slint），不屬於主 workspace |

## 功能

操作以**工具列按鈕**為主，不使用功能表。按鈕只顯示圖示，文字說明（含快捷鍵）放在 tooltip。圖示是在 `ui/src/icons.rs` 以向量自繪的，不依賴圖示字型，會跟隨深淺色主題。

| 按鈕 | 快捷鍵 | 說明 |
|---|---|---|
| 新增／開啟／儲存／另存新檔 | Ctrl+N／O／S／Shift+S | 基本檔案操作；關閉或切換文件前，有未儲存的修改會先詢問 |
| 尋找與取代 | Ctrl+F（取代 Ctrl+H）、F3／Shift+F3 | 開啟尋找列，可選規則表達式、區分大小寫、完整單字 |
| 在檔案中尋找 | Ctrl+Shift+F | 在資料夾底下的所有文字檔裡搜尋，結果點一下就開啟並選取（見下） |
| 設定（⋯，最右邊） | | 外觀（跟隨系統／淺色／深色）、淺色與深色各自的配色、字型（只列等寬字型）、字級、顯示選項（行號、目前行、空白字元、語法上色、自動換行） |
| 字級 | Ctrl ＋／－、Ctrl＋滾輪、Ctrl＋0 | 調整編輯區字級，與設定面板同步 |
| 巨集面板／重跑 | F5（重跑） | 巨集面板列出你的巨集：執行、設快捷鍵、編輯、刪除；失敗時自動還原所有修改 |
| 錄製巨集 | | 開始／停止錄製：記下之後的按鍵與輸入，存成巨集，可重複播放 |
| 分頁列 | Ctrl+W 關閉、Ctrl+Tab／Ctrl+Shift+Tab 切換 | 開啟兩個以上的檔案時才顯示；點擊切換、中鍵或 × 關閉（× 在滑鼠移到分頁上時才出現）、**雙按分頁重新命名檔案**；未儲存的分頁名稱後有「●」 |
| 清除輸出（輸出窗格右上角） | | 清除巨集輸出與訊息 |
| 語言、編碼（含 BOM）、換行（狀態列右側） | | 顯示並可變更存檔格式。編碼清單裡 UTF-8 與 UTF-16 的「帶 BOM」是獨立的選項：UTF-8、UTF-8 BOM、Big5、GBK、Shift_JIS、EUC-KR、UTF-16LE BOM、UTF-16LE、UTF-16BE BOM、UTF-16BE |
| 重新載入（狀態列右側） | | 自動偵測編碼錯誤時，改以指定編碼重新讀取目前的檔案 |

狀態列左側顯示檔案路徑與是否未儲存，右側是這個檔案的格式設定，兩者合在一起就是檔案狀態。也可以用命令列參數開啟檔案：`feditor 檔案路徑`，或把檔案從檔案總管拖進視窗（可一次拖多個，各開一個分頁；已經開著的會切過去；資料夾會略過）。

狀態列上方是**輸出面板**，放開檔訊息、警告、錯誤與巨集的輸出。預設收合成一條（只顯示最後一則訊息），按左邊的箭頭展開或收合，展開的高度可以拖曳調整，展開／收合的狀態會記住。收合時如果出現錯誤或巨集輸出，面板會暫時彈開，讓你不會錯過。

### 分頁、工作階段與外部變更

- 新增或開啟檔案會開在新分頁；目前是空白未修改的文件就直接用它，已經開著的檔案會切過去而不是重複開。每個分頁各有游標、選取與復原歷史。關閉視窗時，只要有分頁未儲存就會逐一詢問。
- **雙按分頁重新命名檔案**：會跳出對話框，預設只選取主檔名（副檔名不會被蓋掉），按 Enter 或「重新命名」就真的把磁碟上的檔案改名（留在同一個資料夾）。檔名不合法（空白、含 `\ / : * ? " < > |`、Windows 保留名稱）、資料夾裡已經有同名檔案、或別的分頁已經開著那個檔案時，會在對話框裡說明原因，不會覆蓋。有未儲存修改的分頁也可以改名，內容與未儲存狀態不變。還沒存成檔案的分頁，雙按會開「另存新檔」。
- **工作階段**：不帶檔名啟動時，會把上次開著的檔案（與游標位置）開回來。記在設定資料夾的 `session.conf`；沒存過的空白文件不記。
- **外部變更偵測**：檔案被別的程式改了（視窗拿回焦點時，以及每秒檢查一次作用中的檔案）；沒有未儲存的修改就自動重新載入並保留游標位置，有的話詢問是否重新載入，選擇保留就不會一直再問。

### 外觀

工具列最右邊的「⋯」是**設定**，展開後有外觀與配色、字型、字級。

- **外觀**：一個三態狀態鈕，圖示隨目前的狀態變化（◐ 跟隨系統、☀ 淺色、☾ 深色），點一下依序切換（跟隨系統 → 淺色 → 深色 → 跟隨系統）。
- **配色**：**淺色與深色各自選一套**，互不影響（例如淺色用 GitHub Light、深色用 Dracula），外觀切換或跟隨系統時就用對應那一套。選擇會記住。內建配色：
  - 深色：EVA（預設，《新世紀福音戰士》的色票：帶紫調的近黑底、初號機紫、螢光綠、NERV 橘）、Dracula、Monokai、Solarized Dark、One Dark、Nord、Gruvbox Dark、Tokyo Night、Catppuccin Mocha／Macchiato／Frappé。
  - 淺色：預設淺色（沿用原本的外觀）、Solarized Light、GitHub Light、One Light、Gruvbox Light、VS Code Light+、Catppuccin Latte。
  - 內建配色是依這些知名配色的色票調整的，並確認文字與各種底色的對比（一般文字至少 4.5:1），所以不一定和原版逐字相同。
- **自訂配色**：設定面板的「編輯目前配色…」會開啟編輯視窗，編輯的是**目前正在使用的主題**（深色或淺色，要編輯另一邊請先切換外觀）所選的那一套。每種顏色（背景、文字、按鈕與邊線、強調與狀態、語法上色，共 25 種）一個色塊，改了立刻預覽，「還原」放棄修改。內建配色不能覆寫，用「另存為…」取名存成自訂配色（會自動設為這個主題使用的配色）；自訂配色可以直接「儲存」。
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

  顏色寫成 `#RRGGBB`（或 `#RGB`）。壞掉的檔案（缺 `name`、不認得的設定、顏色寫錯、`base` 找不到）會略過，並在輸出面板顯示原因。範例見 `examples/themes/midnight.conf`。設定檔裡記的配色不存在（例如檔案被刪了）時退回該主題的預設配色。
- **字型**：只列出**等寬字型**。判斷方式是比對 `i`、`m`、`W`、`.` 等字元的字寬是否一致，不看字型檔裡常常標錯的等寬旗標。預設是**作業系統預設的等寬字型**（向系統查詢，不寫死名稱；Windows 上是 Consolas）。
  有些老字型（細明體、MS Gothic、Lucida Console 等）的拉丁字母是內嵌的彩色點陣，egui 不會依文字顏色上色，畫出來看不見，所以這類字型不會出現在清單裡；設定檔若存了這種字型，會自動退回系統預設，面板顯示的也是實際生效的字型。
  清單要逐一開啟字型檔檢查，所以在背景掃描，掃完前面板會顯示「正在找出系統上的等寬字型…」。中文字形用微軟正黑體補。每個字型的上下緣比例不同，egui 預設會讓拉丁字母和中文字的基準線錯開，所以啟動與換字型時會用 egui 的實際排版量出落差並補償；測試對清單裡的每一個字型都驗證過兩者落在同一條基準線上。
- **字級**：面板的字級欄與快捷鍵共用同一個設定值，所以兩邊永遠同步。快捷鍵：**Ctrl ＋／－** 放大縮小、**Ctrl＋滾輪**、**Ctrl＋0** 還原預設（範圍 8–48 點）。這取代了 egui 內建的「Ctrl ＋／－ 縮放整個介面」。
- **顯示選項**：設定面板裡四個＋一個開關鈕（圖示加 tooltip）：行號、標示目前這一行、顯示空白字元（空格畫小點、Tab 畫箭頭、全形空白畫方框、行尾畫 ↵）、語法上色、自動換行。
- **設定檔**位置：Windows `%APPDATA%\feditor\settings.conf`、macOS `~/Library/Application Support/feditor/settings.conf`、Linux `$XDG_CONFIG_HOME/feditor/settings.conf`（沒設定時是 `~/.config/feditor/settings.conf`）。內容是簡單的 `key = value`：`theme`（`system`／`light`／`dark`）、`font_family`（沒有這行表示系統預設）、`font_size`、`show_line_numbers`／`highlight_line`／`show_whitespace`／`syntax`／`word_wrap`（`true`／`false`）。檔案不存在、壞掉、或值不合理時都退回預設，不會讓程式起不來。字級連續變動時會等停下來 0.4 秒才寫入，關閉視窗時一定會寫。
- 未儲存的標示在標題列（檔名後的「●」），狀態列不重複顯示。

### 編輯區

編輯區是**自己繪製**的（`ui/src/editor/`），不使用 egui 內建的文字框：文字放在 rope 緩衝區，每一幀只排版、繪製看得到的那幾行，所以檔案再大，成本也只和視窗高度有關（測試以 100 萬行的文件驗證）。

- 行號固定在左邊；目前這一行有淡色底；Tab 以 4 欄為一格展開顯示（檔案裡仍是 Tab 字元）。
- **依欄位排版**：每個字元固定佔「欄寬 × 半形字寬」，中文與全形字一律剛好兩欄，不管中文字型本身的字寬是多少，上下行的英數字都會對齊（只靠等寬的拉丁字型做不到這點，因為中文字來自另一個字型）。半形英數字整串排、其餘字元逐字排並置中在自己的格子裡。
- 游標以「字」為單位移動：組合字元、emoji（含 ZWJ 序列）算一個字；中日韓全形字佔兩欄。上下移動會記住原本的欄位。
- 快捷鍵：方向鍵、Home／End（第一次按到第一個非空白字元，再按到行首）、Ctrl＋Home／End、Page Up／Down、Ctrl＋方向鍵（以詞為單位）、Shift＋以上（選取）、Ctrl+A、Ctrl+Z、Ctrl+Y（或 Ctrl+Shift+Z）、Ctrl+X／C／V、Tab／Shift+Tab。Enter 會沿用上一行的縮排。滑鼠：點擊、拖曳選取、雙擊選詞、三擊選行、Shift＋點擊延伸選取。
- **行操作**：Alt＋↑／↓ 上下移動行、Shift＋Alt＋↑／↓ 複製行、Ctrl+Shift+K 刪除行、Ctrl+/ 註解切換（依副檔名決定符號，全部已註解就取消）、Ctrl+] ／ Ctrl+[ 縮排／反縮排；選取跨多行時 Tab／Shift+Tab 整段縮排。每個操作是一個復原步驟，選取會跟著文字移動。
- **多重游標與矩形選取**：Ctrl＋點擊加上（或移除）游標、Ctrl+Alt＋↑／↓ 在上／下一行加游標、Ctrl+D 選取游標下的詞並逐次加入下一個相同的文字、**Alt＋拖曳**矩形選取（依顯示欄位，中文算兩欄）、Esc 回到單一游標。輸入、刪除、Enter、貼上、移動、複製與剪下都套用到每個游標，一次動作是一個復原步驟；貼上時剪貼簿的行數剛好等於游標數就一個游標一行。復原、取代、巨集等外部改動會讓多重游標作廢。
- **自動換行**：設定面板的開關。依顯示欄位折行（英文單字不切開、中日韓字元之間可折、避頭點不落在行首），行號只標在每一行的第一列，上下鍵、Home／End、滑鼠都以畫面上的列為單位；折行處的游標可以停在上一列的尾端或下一列的開頭（點在列尾或按 End 會停在上一列）。超過 100,000 行的檔案不提供自動換行（每次修改都要重算整份文件的列數）。
- **語法上色**：依副檔名（Rust、C/C++、Java/C#、JavaScript/TypeScript、Python、Go、Shell、SQL、CSS、Lua、JSON、TOML/INI/YAML、HTML/XML、Markdown），標出註解、字串、數字、關鍵字、型別、函式名稱；區塊註解、三引號字串、樣板字串可跨行。字串裡的內嵌運算式也會當成程式碼上色：JavaScript／TypeScript 樣板字串的 `${…}`、Python f-string 的 `{…}`（`{{` 是跳脫，不算）、Shell 雙引號字串的 `${…}`；運算式裡的字串與巢狀樣板字串都會配對，運算式也可以跨行。三引號的 f-string 目前不處理。這是輕量的分詞，不是完整的語法分析。超過 20,000 行的檔案改成只往回看 1000 行估計跨行狀態。
- **選擇語言**：狀態列（編碼的左邊）有「語言」下拉清單，列出內建與自訂語言，另有「自動」與「純文字」。選擇只屬於這個分頁，不寫進檔案；重新載入不會改變它。「自動」的規則：有檔名就依副檔名；**還沒存檔、沒有檔名**時看開頭內容猜：`#!` 直譯器行（Python、Node、Lua、各種 shell）、`<?php`、`<!DOCTYPE html`／`<html`／`<?xml`／`<svg`、以 `{`／`[` 開頭的 JSON；猜不出來就不上色（例如貼上一段沒有特徵的 Rust 程式碼，要自己從清單選）。手動選了語言，Ctrl+/ 的註解符號也跟著語言走。
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
  | `mode` | `code`（預設）、`markup`（HTML/XML）、`markdown`、`config`（ini／toml 的 `[區段]` 與「鍵 =」） |
- **復原**以「一段操作」為單位：連續打字、連續刪除各算一步；單次取代、全部取代、整支巨集各是一步。
- **輸入法**：組字中的文字畫在游標處並加底線，**不會進入緩衝區**，所以組字時不會污染復原歷史；候選窗貼在組字處。組字中的方向鍵由輸入法自己處理（選字），滑鼠點到別處會結束組字；點到視窗裡不能輸入的地方（空白處、按鈕）會維持組字，焦點被別的輸入框取走時，組字的內容會直接寫進編輯區。游標位置依輸入法的行為推算（Windows 內建注音只回報「被標示的一段」，游標要由文字變化與重複事件推得）。輸入法行為異常時，可設定環境變數 `FEDITOR_IME_LOG=記錄檔路徑` 再啟動，編輯區會把輸入法事件與當時的游標、組字狀態追加寫入該檔案（沒設定就不會寫任何東西）。
- 目前的限制：單行極長（數十萬字元）時該行會變慢；行數超過約 1,000,000 時垂直捲動的位置有 f32 精度限制（實測 400 萬行、346 MB 的檔案開啟約 4.5 秒、佔用約 840 MB，可以操作，但行與行之間的間距會出現肉眼可見的抖動）；Windows 的輸入法有焦點時，部分 Ctrl+Alt＋字母的組合會被輸入法吃掉而到不了編輯器。
### 尋找與取代

- 三個選項：規則表達式、區分大小寫（預設不區分）、完整單字。輸入時即時搜尋，顯示「第幾筆／總筆數」；找不到會繞回另一端並提示「已繞回」。
- 滑鼠停在「規則表達式」按鈕上會出現語法速查表與完整說明的連結（<https://docs.rs/regex/latest/regex/#syntax>）；滑鼠移到提示框上它也不會消失，可以直接點連結。
- **規則表達式**使用 Rust `regex` 語法（保證線性時間，不會被寫壞的規則卡死）；`^`、`$` 以「行」為單位。取代字串可用 `$1`、`${名稱}` 代入群組，`\n`、`\t` 代表換行與跳位，字面的 `$` 請寫 `$$`。
- **完整單字**：比對結果的前後不可緊鄰字母、數字或底線。中文沒有空格分詞，相鄰的漢字會互相阻擋，所以對中文詞通常不適合開這個選項。
- 取代與巨集造成的修改都可以用 Ctrl+Z 復原、Ctrl+Y 重做：單次取代、全部取代、整支巨集各是一個復原步驟。
- 檔案超過 8 MiB 時不統計總筆數（顯示「—」），避免每次輸入都掃描整份大檔案；統計上限 9999 筆。
- 找到的那一筆會直接成為編輯區的選取範圍（接著打字就是取代它）；「下一筆」從選取的尾端、「上一筆」從選取的開頭算起，所以自己點一下游標再按 F3，會從游標位置繼續找。Esc 關閉尋找列後，游標回到編輯區。

### 在檔案中尋找

Ctrl+Shift+F 展開面板（會帶入選取的文字與目前檔案所在的資料夾）：搜尋文字、三個選項（區分大小寫、完整單字、規則表達式）、資料夾、檔名過濾（例如 `*.rs;*.toml`）。搜尋在背景執行，可隨時停止；結果點一下就開啟（已開著就切過去）並選取符合的文字。

- 讀檔時自動偵測編碼（Big5、UTF-16 等都能搜）；含 NUL 的二進位檔、超過 20 MiB 的檔案、`.git`／`target`／`node_modules` 資料夾會略過。
- 一行只列第一個符合的位置；最多 5000 筆，超過會提示縮小範圍。

應用程式圖示（文字文件加鉛筆）由 `ui/assets/make_icon.py` 產生：`uv run ui/assets/make_icon.py`。

- **編碼偵測**：BOM → 嚴格 UTF-8 → `chardetng`（偏向繁體中文 Big5）。
- **換行**：記憶原檔的 CRLF／LF／CR，存檔時還原；混用時會警告並統一。
- **存檔安全**：先寫入暫存檔再改名；目標編碼無法表示的字元會讓存檔**失敗**，不會靜默替換；載入時有解碼失敗的位元組，則禁止覆寫原檔。

## 設計決策

- **核心用 Rust**：效能、單一執行檔、跨平台發佈。
- **UI 用 egui 自繪**：繁體中文 IME 已在 egui 上驗證通過（見 `proto/IME-TEST.md`）。編輯區不用 egui 的 `TextEdit`，而是自繪（rope 緩衝區＋只繪製看得到的行），原因是 `TextEdit` 對大檔案不夠快、復原與輸入法組字的行為也無法細部控制。
- **巨集語言先選 JavaScript**（`rquickjs`）：輕量、可靜態連結、容易設定執行時間與記憶體上限。透過 `ScriptEngine` 抽象，日後要加 Python 等其他語言不需改動核心。
- **檔案大小目標**：幾百 MB 以內。若日後需要更大，再優化緩衝區與渲染。

## 巨集

### 巨集庫、錄製與快捷鍵

- 你的巨集放在設定資料夾下的 `macros` 資料夾（Windows `%APPDATA%\feditor\macros`），每個 `.js` 檔一支。工具列的巨集按鈕展開巨集面板：▶ 執行、鍵盤圖示設快捷鍵、鉛筆圖示在新分頁編輯原始碼、I 形圖示重新命名（雙按名稱也可以）、垃圾桶刪除；面板底下可以從任意檔案執行，或開啟巨集資料夾。
- 檔案開頭的註解可以帶標記：`// @name 顯示名稱`、`// @shortcut Ctrl+Alt+U`。設快捷鍵時按下組合鍵，程式會改寫檔頭的 `@shortcut`（Esc 取消、Delete 清除）。要有 Ctrl 或 Alt（功能鍵 F1–F12 除外）；和內建快捷鍵或別支巨集重複會被拒絕。用快捷鍵（例如 Alt+M）執行巨集時，不會多打出那個字母。重新命名巨集改的是檔頭的 `@name`，檔名不動，快捷鍵等其他標記保留。
- **錄製**：按工具列的錄製鈕開始，之後的**按鍵與輸入**（打字、輸入法確定的文字、貼上、方向鍵、刪除、行操作…）會被記下來，再按一次停止，會跳出對話框請你**為巨集命名**（預設是「錄製的巨集」，與現有名稱重複時自動加編號；Enter 或「儲存」就存進 `macros` 資料夾，名稱原樣寫在檔頭的 `@name`，檔名則換掉不能用的字元；「放棄」就不存，Esc 不會關掉對話框，免得錄好的東西不小心丟掉），存好後成為 F5 重跑的對象。滑鼠操作不會錄。錄好的是可以編輯的 JavaScript，連續的打字會併成一句、連續重複的指令用次數表示：

```js
// 由錄製產生
editor.run("docEnd");
editor.paste(" world");
editor.run("selectRight", 2);
```

- 巨集執行是**一個復原步驟**；失敗時文字與選取都還原成執行前的樣子。

### 巨集 API（JavaScript）

| API | 說明 |
|---|---|
| `editor.text()` | 完整文字 |
| `editor.length()` | 總字元數 |
| `editor.lineCount()` | 行數 |
| `editor.line(i)` | 第 `i` 行（含換行），超出範圍為 `undefined` |
| `editor.slice(start, end)` | `[start, end)` 的文字 |
| `editor.lineStart(i)`／`editor.lineOf(pos)` | 第 `i` 行的起點／位置 `pos` 在第幾行 |
| `editor.insert(at, text)` | 在位置 `at` 插入（選取會跟著文字移動） |
| `editor.remove(start, end)` | 刪除 `[start, end)` |
| `editor.replace(start, end, text)` | 取代 `[start, end)` |
| `editor.selection()` | `[錨點, 游標]` |
| `editor.setSelection(anchor, head)` | 設定選取 |
| `editor.selectedText()` | 選取的文字 |
| `editor.path()` | 目前檔案的完整路徑，沒存過為 `undefined` |
| `editor.run(name, count = 1)` | 執行編輯指令，和按鍵相同；名稱見下 |
| `editor.type(text)`／`editor.paste(text)` | 像使用者打字／貼上（取代選取、游標跟著移動） |
| `editor.find(query, from = 0, opts)` | 找到回傳 `[start, end]`，否則 `undefined`；`opts`：`regex`、`caseSensitive`、`wholeWord`、`backward` |
| `editor.alert(msg)`／`editor.confirm(msg)` | 對話框；`confirm` 回傳是否按了「是」 |
| `console.log(...)` | 輸出 |

編輯指令（`editor.run`）：`left` `right` `up` `down` `wordLeft` `wordRight` `pageUp` `pageDown` `home` `end` `docStart` `docEnd`，加上 `select` 前綴的選取版本（`selectLeft`…`selectDocEnd`、`selectAll`）；`backspace` `delete` `deleteWordLeft` `deleteWordRight` `enter` `tab` `shiftTab` `undo` `redo`；`moveLineUp` `moveLineDown` `duplicateLine` `deleteLine` `toggleComment` `indent` `unindent`；`addCaretUp` `addCaretDown` `selectNextOccurrence` `clearCarets`。不認得的名稱會丟出錯誤。

- 位置單位是 **Unicode 碼點**，不是 JavaScript 字串的 UTF-16 單位。`"😀".length` 是 2，但 `editor.length()` 只算 1。
- 位置必須是非負整數，否則丟出 `RangeError`。
- 每次執行都使用全新的環境，巨集之間不共用全域變數。
- 預設執行時間上限 5 秒、記憶體上限 64 MiB，死循環與記憶體炸彈會被中止。

## 已知問題

- 這台開發機器的 wgpu **Vulkan 後端會崩潰**（`0xC0000005`）。Windows 上預設改用 DX12，可用環境變數 `WGPU_BACKEND` 覆寫。
- IME 只在 Windows + DX12 驗證過，macOS 與 Linux 尚未驗證。
- 跨平台建置與發佈的 GitHub Actions 設定在 `.github/workflows/`（Windows、macOS、Linux 都跑過測試與建置）；Linux 需要的系統套件列在 `ci.yml` 裡，其他發行版可能要調整。
- macOS 的 `FEditor.app` 沒有 Apple 公證，也還沒有在真的 Mac 上實測過；從 Finder 對檔案按「打開方式」目前不會開檔（只支援命令列參數）。

## 下載

到 [Releases](https://github.com/codemee/feditor/releases) 下載對應平台的壓縮檔：

- **Windows**：解壓縮後執行 `feditor.exe`。
- **macOS**（Apple Silicon）：下載 `feditor-macos.dmg`，開啟後把 `FEditor.app` 拖到旁邊的「應用程式」捷徑。沒有公證，第一次開啟要在 Finder 對它按右鍵 →「打開」，或執行 `xattr -dr com.apple.quarantine FEditor.app`。
- **Linux**：解壓縮後執行 `./feditor`。

每個版本改了什麼，請看[更新紀錄](CHANGELOG.md)。

## 開發

```powershell
cargo test --workspace --exclude feditor       # 單元測試（不含 UI）
cargo run --release -p feditor                 # 啟動編輯器
```
