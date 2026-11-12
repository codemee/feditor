[English](macros.en.md) | **繁體中文**

[← 回到 README](../README.md)

# 巨集操作手冊

巨集可以把重複的編輯動作變成一個按鈕或一組快捷鍵。你可以**錄製**自己的操作，也可以用 JavaScript **自己寫**。

**目錄**

1. [快速上手：錄製第一支巨集](#1-快速上手錄製第一支巨集)
2. [執行巨集](#2-執行巨集)
3. [巨集面板](#3-巨集面板)
4. [設定快捷鍵](#4-設定快捷鍵)
5. [限定適用的檔案類型](#5-限定適用的檔案類型)
6. [管理巨集檔案](#6-管理巨集檔案)
7. [自己寫巨集](#7-自己寫巨集)
8. [巨集 API 參考](#8-巨集-api-參考)
9. [限制與注意事項](#9-限制與注意事項)
10. [疑難排解](#10-疑難排解)

---

## 1. 快速上手：錄製第一支巨集

1. 按工具列的**錄製**鈕 <img src="images/icons/record.svg" width="18" height="18" align="absmiddle" alt="錄製"> 開始錄製。
2. 照平常的方式操作：打字、貼上、方向鍵、刪除、行操作…
3. 再按一次（錄製中這顆鈕會變成 <img src="images/icons/stop.svg" width="18" height="18" align="absmiddle" alt="停止錄製">）停止。
4. 在跳出的對話框替巨集取名（預設「錄製的巨集」），按 **Enter** 或「儲存」。
5. 按 **F5**，就會重跑剛才錄好的巨集。

> **錄得到的**：按鍵與輸入，包括打字、輸入法確定的文字、貼上、方向鍵、刪除、行操作。
> **錄不到的**：滑鼠操作（點擊、拖曳選取）。需要選取文字時請改用鍵盤（例如 Shift+方向鍵）。

對話框裡：

| 按鈕／按鍵 | 作用 |
|---|---|
| Enter、「儲存」 | 存進巨集資料夾 |
| 「放棄」 | 不存 |
| Esc | **不會**關閉對話框（免得錄好的東西不小心丟掉） |

名稱重複時會自動加編號。錄製開始或結束後，鍵盤焦點會回到編輯區，原本的選取不會被清掉，可以直接接著打字。

## 2. 執行巨集

| 方式 | 說明 |
|---|---|
| **F5** | 重跑最近一次執行（或剛錄好）的巨集；工具列的 <img src="images/icons/rerun.svg" width="18" height="18" align="absmiddle" alt="重新執行上一個巨集"> 也一樣 |
| **巨集面板的 <img src="images/icons/run.svg" width="18" height="18" align="absmiddle" alt="執行">** | 執行那一支（見下一節） |
| **快捷鍵** | 設好快捷鍵的巨集，按組合鍵就執行（見第 4 節） |
| **從檔案執行…** | 面板底下的按鈕，執行任何位置的 `.js` 檔，不必放進巨集資料夾 |

**執行是一個復原步驟**：一次 Ctrl+Z 就能把整支巨集做的事全部還原。**失敗時會自動還原**：巨集中途出錯，文字與選取都會回到執行前的樣子。

## 3. 巨集面板

按工具列的**巨集**鈕 <img src="images/icons/run.svg" width="18" height="18" align="absmiddle" alt="巨集"> 展開面板。每一列是一支巨集，列上的圖示：

| 圖示 | 作用 |
|---|---|
| <img src="images/icons/run.svg" width="18" height="18" align="absmiddle" alt="執行"> | 執行 |
| <img src="images/icons/keyboard.svg" width="18" height="18" align="absmiddle" alt="鍵盤"> | 設定快捷鍵 |
| <img src="images/icons/edit.svg" width="18" height="18" align="absmiddle" alt="鉛筆"> | 在新分頁開啟原始碼編輯 |
| <img src="images/icons/dropdown.svg" width="18" height="18" align="absmiddle" alt="下拉"> | 每列的下拉清單：設定適用的檔案類型（見第 5 節） |
| <img src="images/icons/rename.svg" width="18" height="18" align="absmiddle" alt="重新命名"> | 重新命名（雙按名稱也可以） |
| <img src="images/icons/trash.svg" width="18" height="18" align="absmiddle" alt="刪除"> | 刪除 |

面板最下面一排按鈕：

| 按鈕 | 作用 |
|---|---|
| 從檔案執行… | 選一個 `.js` 檔直接執行 |
| 開啟巨集資料夾 | 用檔案管理員開啟存放巨集的資料夾 |
| 查看快捷鍵 | 打開設定面板的「快捷鍵」頁，列出內建的與已設給巨集的快捷鍵 |
| 巨集說明文件 | 在瀏覽器開啟這份手冊 |

面板只列出**適用於目前檔案**的巨集；限定給別種檔案類型的，收在底下的「其他檔案類型的巨集」（預設收合）。

## 4. 設定快捷鍵

1. 按巨集那一列的鍵盤圖示 <img src="images/icons/keyboard.svg" width="18" height="18" align="absmiddle" alt="鍵盤">。
2. 按下你要的組合鍵。
3. 程式會把它寫進巨集檔開頭的 `@shortcut`。

| 按鍵 | 作用 |
|---|---|
| 組合鍵 | 設定 |
| **Esc** | 取消設定 |
| **Delete** | 清除這支巨集的快捷鍵 |

**規則**

- 必須包含 **Ctrl** 或 **Alt**（F1 到 F12 功能鍵例外）。
- 不能和**內建快捷鍵**重複，會被拒絕並說明原因。
- 不能和**適用範圍重疊**的別支巨集重複（見下一節）。
- 用快捷鍵（例如 Alt+M）執行時，不會多打出那個字母。
- 想看現在有哪些快捷鍵：面板的「查看快捷鍵」。

## 5. 限定適用的檔案類型

預設每支巨集適用於**所有檔案**。你可以限定它只在某些檔案類型生效，例如只給 Rust 或 Python。

**設定方式**

- 在面板每一列的下拉清單，點開後勾選檔案類型，可以多選，**勾選後立刻生效**。
- 清單直接顯示目前的選擇；沒限定就顯示「所有檔案」。
- 錄製結束的命名對話框，如果目前檔案有語言，會多一個選擇：「所有檔案」（預設）或「只有 Rust 檔案」。
- 也可以直接在檔頭寫 `// @lang Rust, Python`（名稱和狀態列「語言」清單相同，逗號分隔、不分大小寫）。

**限定之後的行為**

| 情況 | 結果 |
|---|---|
| 目前檔案的語言符合 | 面板列出、可以執行、快捷鍵與 F5 有效 |
| 目前檔案的語言不符 | 收在「其他檔案類型的巨集」；仍可改名、改適用範圍、刪除，但**不能執行**；快捷鍵與 F5 不觸發（F5 會說明原因） |
| 純文字、認不出來的檔案 | 只適用通用（沒限定）的巨集 |

比對的是目前文件**實際生效的語言**，在狀態列手動改語言，巨集的適用性也會跟著變。

**共用快捷鍵**：適用範圍沒有重疊的巨集可以用同一個快捷鍵。例如 Rust 專用的和 Python 專用的都用 Alt+R。通用的巨集和任何範圍都重疊，所以不能與別的巨集共用。

## 6. 管理巨集檔案

巨集就是普通的 `.js` 檔，每支一個。

| 系統 | 巨集資料夾 |
|---|---|
| Windows | `%APPDATA%\feditor\macros` |
| macOS | `~/Library/Application Support/feditor/macros` |
| Linux | `$XDG_CONFIG_HOME/feditor/macros`（沒設定時是 `~/.config/feditor/macros`） |

最快的開法：巨集面板的「**開啟巨集資料夾**」。

**檔頭標記**：檔案開頭的註解可以帶這些標記，面板操作時程式會自動維護，你也可以手動寫。

```js
// @name 顯示名稱
// @shortcut Ctrl+Alt+U
// @lang Rust, Python
```

| 標記 | 作用 |
|---|---|
| `@name` | 面板上顯示的名稱 |
| `@shortcut` | 快捷鍵 |
| `@lang` | 適用的檔案類型 |

**重新命名**改的是 `@name`，檔名不動，快捷鍵等其他標記都保留。錄製時存檔的檔名會把不能用的字元換掉，名稱則原樣寫在 `@name`。

## 7. 自己寫巨集

### 範例

`examples/macros/` 有三支可以直接複製到巨集資料夾試的範例：

| 檔案 | 作用 |
|---|---|
| `uppercase.js` | 把全文轉成大寫 |
| `line-numbers.js` | 在每一行開頭加上行號 |
| `fail-demo.js` | 先修改再故意出錯，用來看「失敗時自動還原」 |

### 錄製出來的也是 JavaScript

錄好的巨集可以按鉛筆圖示 <img src="images/icons/edit.svg" width="18" height="18" align="absmiddle" alt="鉛筆"> 打開來改。連續的打字會合併成一句，連續重複的指令用次數表示：

```js
// 由錄製產生
editor.run("docEnd");
editor.paste(" world");
editor.run("selectRight", 2);
```

### 一個簡單的例子

在選取的文字外面加上括號：

```js
// @name 加括號
// @shortcut Alt+9
editor.wrap("(", ")");
```

### 觀念

- 巨集透過全域物件 `editor` 讀寫目前的文件；`console.log(...)` 的輸出顯示在視窗下方的輸出窗格。
- **位置**以字元為單位，從 0 開始（見第 9 節）。
- 要在多處插入文字時，**由後往前**插入，前面的位置才不會因為插入而位移（見 `line-numbers.js`）。

## 8. 巨集 API 參考

### 讀取文件

| API | 說明 |
|---|---|
| `editor.text()` | 完整文字 |
| `editor.length()` | 總字元數 |
| `editor.lineCount()` | 行數 |
| `editor.line(i)` | 第 `i` 行（含換行），超出範圍為 `undefined` |
| `editor.slice(start, end)` | `[start, end)` 的文字 |
| `editor.lineStart(i)` | 第 `i` 行的起點位置 |
| `editor.lineOf(pos)` | 位置 `pos` 在第幾行 |
| `editor.path()` | 目前檔案的完整路徑，沒存過為 `undefined` |

### 修改文件

| API | 說明 |
|---|---|
| `editor.insert(at, text)` | 在位置 `at` 插入（選取會跟著文字移動） |
| `editor.remove(start, end)` | 刪除 `[start, end)` |
| `editor.replace(start, end, text)` | 取代 `[start, end)` |
| `editor.type(text)` | 像使用者打字（取代選取、游標跟著移動） |
| `editor.paste(text)` | 像使用者貼上（取代選取、游標跟著移動） |
| `editor.wrap(prefix, suffix)` | 用兩段文字包住每個選取；沒有選取就插入兩者，游標停在中間；是一個復原步驟 |

`wrap` 包完之後選取仍留在原本的文字上，可以再包一層。

### 選取

| API | 說明 |
|---|---|
| `editor.selection()` | `[錨點, 游標]` |
| `editor.setSelection(anchor, head)` | 設定選取 |
| `editor.selectedText()` | 選取的文字 |

### 搜尋

| API | 說明 |
|---|---|
| `editor.find(query, from = 0, opts)` | 找到回傳 `[start, end]`，否則 `undefined` |

`opts` 可用的選項：

| 選項 | 作用 |
|---|---|
| `regex` | 把 `query` 當規則運算式 |
| `caseSensitive` | 區分大小寫 |
| `wholeWord` | 整個單字 |
| `backward` | 往回找 |

### 對話框與輸出

| API | 說明 |
|---|---|
| `editor.alert(msg)` | 顯示訊息 |
| `editor.confirm(msg)` | 詢問，回傳是否按了「是」 |
| `console.log(...)` | 輸出到輸出窗格 |

### 編輯指令：`editor.run(name, count = 1)`

和按鍵的效果相同，`count` 是重複次數。不認得的名稱會丟出錯誤。

| 類別 | 指令 |
|---|---|
| 移動 | `left` `right` `up` `down` `wordLeft` `wordRight` `pageUp` `pageDown` `home` `end` `docStart` `docEnd` |
| 選取 | 移動指令加 `select` 前綴：`selectLeft`、`selectRight`…`selectDocEnd`；另有 `selectAll` |
| 刪除與輸入 | `backspace` `delete` `deleteWordLeft` `deleteWordRight` `enter` `tab` `shiftTab` |
| 復原 | `undo` `redo` |
| 行操作 | `moveLineUp` `moveLineDown` `duplicateLine` `deleteLine` |
| 程式碼 | `toggleComment` `indent` `unindent` `tabsToSpaces` `spacesToTabs` `reformat`（排列格式） |
| 多游標 | `addCaretUp` `addCaretDown` `selectNextOccurrence` `clearCarets` |

## 9. 限制與注意事項

- **位置單位是 Unicode 碼點**，不是 JavaScript 字串的 UTF-16 單位。例如 `"😀".length` 是 2，但 `editor.length()` 只算 1。
- 位置必須是**非負整數**，否則丟出 `RangeError`。
- **每次執行都是全新的環境**，巨集之間不共用全域變數。
- 執行時間上限 **5 秒**、記憶體上限 **64 MiB**，死循環與記憶體炸彈會被中止，並照一般失敗處理：還原所有修改。
- 滑鼠操作不會被錄製。

## 10. 疑難排解

| 狀況 | 可能原因與處理 |
|---|---|
| 面板裡找不到我的巨集 | 它限定了別種檔案類型，看看底下收合的「其他檔案類型的巨集」，或在下拉清單改成「所有檔案」 |
| 按快捷鍵沒反應 | 巨集不適用於目前檔案的語言；或快捷鍵被內建功能占用（設定時會被拒絕）。到「查看快捷鍵」確認 |
| F5 沒有執行，還跳出說明 | 上一次的巨集不適用於目前檔案，說明裡會寫原因 |
| 設快捷鍵被拒絕 | 沒有 Ctrl 或 Alt、和內建快捷鍵重複，或和適用範圍重疊的別支巨集重複 |
| 巨集跑到一半出錯 | 文字已自動還原；錯誤訊息在視窗下方的輸出窗格 |
| 巨集被中止 | 超過 5 秒或 64 MiB，檢查有沒有無窮迴圈 |
| 錄好的巨集重跑結果不一樣 | 滑鼠操作沒有被錄，改用鍵盤操作重錄 |
