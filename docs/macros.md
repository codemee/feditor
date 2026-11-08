[English](macros.en.md) | **繁體中文**

[← 回到 README](../README.md)

# 巨集

## 巨集庫、錄製與快捷鍵

- 你的巨集放在設定資料夾下的 `macros` 資料夾（Windows `%APPDATA%\feditor\macros`），每個 `.js` 檔一支。工具列的巨集按鈕展開巨集面板：▶ 執行、鍵盤圖示設快捷鍵、鉛筆圖示在新分頁編輯原始碼、每一列的下拉清單設定適用的檔案類型（直接顯示目前的選擇，沒限定就是「所有檔案」；點開勾選目前支援的檔案類型，可以多選，勾選後立刻生效）、I 形圖示重新命名（雙按名稱也可以）、垃圾桶刪除；面板底下可以從任意檔案執行、開啟巨集資料夾，或「查看快捷鍵」（打開設定面板的「快捷鍵」頁，列出內建的與已設給巨集的快捷鍵，不適用於目前檔案的巨集用淡色）。執行完巨集、錄製開始或結束之後，鍵盤焦點會回到編輯區，原本的選取不會被清掉，可以直接接著打字。
- 檔案開頭的註解可以帶標記：`// @name 顯示名稱`、`// @shortcut Ctrl+Alt+U`。設快捷鍵時按下組合鍵，程式會改寫檔頭的 `@shortcut`（Esc 取消、Delete 清除）。要有 Ctrl 或 Alt（功能鍵 F1–F12 除外）；和內建快捷鍵、或和適用範圍重疊的別支巨集重複會被拒絕（見下一項）。用快捷鍵（例如 Alt+M）執行巨集時，不會多打出那個字母。重新命名巨集改的是檔頭的 `@name`，檔名不動，快捷鍵等其他標記保留。
- **只在特定檔案類型生效**：檔頭加 `// @lang Rust, Python`（名稱同狀態列「語言」清單，逗號分隔、不分大小寫）就只適用於那些檔案類型；沒有 `@lang` 的巨集適用於所有檔案（舊的巨集不用改）。比對的是目前文件實際生效的語言，手動改語言也會跟著變；純文字和認不出來的檔案只適用通用的巨集。巨集面板只列適用於目前檔案的，限定了別種檔案類型的收在底下「其他檔案類型的巨集」（預設收合，照樣能改名、改適用範圍、刪除，但不能執行）；快捷鍵與 F5 在不適用的檔案上不觸發（F5 會說明原因）。**適用範圍沒有重疊的巨集可以共用同一個快捷鍵**（Rust 專用的和 Python 專用的都用 Alt+R），通用的巨集和任何範圍都重疊，所以不能共用。錄製結束的命名對話框，在目前的檔案有語言時多一個選擇：「所有檔案」（預設）或「只有 Rust 檔案」；之後在面板每一列的下拉清單勾選就能改。
- **錄製**：按工具列的錄製鈕開始，之後的**按鍵與輸入**（打字、輸入法確定的文字、貼上、方向鍵、刪除、行操作…）會被記下來，再按一次停止，會跳出對話框請你**為巨集命名**（預設是「錄製的巨集」，與現有名稱重複時自動加編號；Enter 或「儲存」就存進 `macros` 資料夾，名稱原樣寫在檔頭的 `@name`，檔名則換掉不能用的字元；「放棄」就不存，Esc 不會關掉對話框，免得錄好的東西不小心丟掉），存好後成為 F5 重跑的對象。滑鼠操作不會錄。錄好的是可以編輯的 JavaScript，連續的打字會併成一句、連續重複的指令用次數表示：

```js
// 由錄製產生
editor.run("docEnd");
editor.paste(" world");
editor.run("selectRight", 2);
```

- 巨集執行是**一個復原步驟**；失敗時文字與選取都還原成執行前的樣子。

## 巨集 API（JavaScript）

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
| `editor.wrap(prefix, suffix)` | 用兩段文字包住每個選取（前面加 `prefix`、後面加 `suffix`，選取留在原本的文字上，可以再包一層）；沒有選取就插入兩者，游標停在中間；是一個復原步驟 |
| `editor.find(query, from = 0, opts)` | 找到回傳 `[start, end]`，否則 `undefined`；`opts`：`regex`、`caseSensitive`、`wholeWord`、`backward` |
| `editor.alert(msg)`／`editor.confirm(msg)` | 對話框；`confirm` 回傳是否按了「是」 |
| `console.log(...)` | 輸出 |

編輯指令（`editor.run`）：`left` `right` `up` `down` `wordLeft` `wordRight` `pageUp` `pageDown` `home` `end` `docStart` `docEnd`，加上 `select` 前綴的選取版本（`selectLeft`…`selectDocEnd`、`selectAll`）；`backspace` `delete` `deleteWordLeft` `deleteWordRight` `enter` `tab` `shiftTab` `undo` `redo`；`moveLineUp` `moveLineDown` `duplicateLine` `deleteLine` `toggleComment` `indent` `unindent` `tabsToSpaces` `spacesToTabs` `reformat`（排列格式）；`addCaretUp` `addCaretDown` `selectNextOccurrence` `clearCarets`。不認得的名稱會丟出錯誤。

- 位置單位是 **Unicode 碼點**，不是 JavaScript 字串的 UTF-16 單位。`"😀".length` 是 2，但 `editor.length()` 只算 1。
- 位置必須是非負整數，否則丟出 `RangeError`。
- 每次執行都使用全新的環境，巨集之間不共用全域變數。
- 預設執行時間上限 5 秒、記憶體上限 64 MiB，死循環與記憶體炸彈會被中止。
