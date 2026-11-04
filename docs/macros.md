[English](macros.en.md) | **繁體中文**

[← 回到 README](../README.md)

# 巨集

## 巨集庫、錄製與快捷鍵

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
| `editor.find(query, from = 0, opts)` | 找到回傳 `[start, end]`，否則 `undefined`；`opts`：`regex`、`caseSensitive`、`wholeWord`、`backward` |
| `editor.alert(msg)`／`editor.confirm(msg)` | 對話框；`confirm` 回傳是否按了「是」 |
| `console.log(...)` | 輸出 |

編輯指令（`editor.run`）：`left` `right` `up` `down` `wordLeft` `wordRight` `pageUp` `pageDown` `home` `end` `docStart` `docEnd`，加上 `select` 前綴的選取版本（`selectLeft`…`selectDocEnd`、`selectAll`）；`backspace` `delete` `deleteWordLeft` `deleteWordRight` `enter` `tab` `shiftTab` `undo` `redo`；`moveLineUp` `moveLineDown` `duplicateLine` `deleteLine` `toggleComment` `indent` `unindent` `tabsToSpaces` `spacesToTabs` `reformat`（排列格式）；`addCaretUp` `addCaretDown` `selectNextOccurrence` `clearCarets`。不認得的名稱會丟出錯誤。

- 位置單位是 **Unicode 碼點**，不是 JavaScript 字串的 UTF-16 單位。`"😀".length` 是 2，但 `editor.length()` 只算 1。
- 位置必須是非負整數，否則丟出 `RangeError`。
- 每次執行都使用全新的環境，巨集之間不共用全域變數。
- 預設執行時間上限 5 秒、記憶體上限 64 MiB，死循環與記憶體炸彈會被中止。
