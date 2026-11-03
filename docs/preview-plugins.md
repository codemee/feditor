[English](preview-plugins.en.md) | **繁體中文**

[← 回到 README](../README.md)

# 預覽外掛

預覽窗格內建支援 Markdown、CSV／TSV、JSON／TOML／YAML 與 SVG。其他檔案類型可以用 JavaScript 外掛預覽。

- **預覽外掛**：想預覽別的檔案類型，可以自己寫 JavaScript 外掛，放進設定資料夾的 `previews` 資料夾（Windows `%APPDATA%\feditor\previews\`；設定面板「預覽」群組的「預覽資料夾」按鈕可以直接開啟），一個 `*.js` 檔一個外掛，**打開設定面板時會重新讀取**。範例在 `examples/previews/`（記錄檔表格、INI 樹、待辦清單）。外掛跑在和巨集同樣的沙盒裡（有時間與記憶體上限，不能讀寫檔案或連網路，也拿不到 `editor`），所以裝別人寫的外掛是安全的。
  - 檔頭用 `//` 註解宣告：`@name` 名稱（可省略，預設用檔名）、`@extensions` 要處理的副檔名（逗號分隔，不分大小寫，必填）、`@output` 呈現方式（必填）。
  - 外掛要定義全域函式 `preview(text, info)`：`text` 是檔案全文，`info` 是 `{ path, name, ext }`（沒存過檔時是空的）。回傳值依 `@output` 而定：

    | `@output` | 回傳 | 顯示成 |
    |---|---|---|
    | `markdown` | Markdown 字串 | 同內建的 Markdown 預覽 |
    | `table` | `{ header: [...], rows: [[...], ...] }` | 表格 |
    | `tree` | 任何 JSON 值（物件、陣列…） | 可摺疊的樹 |
    | `svg` | SVG 字串 | 圖形 |
    | `text` | 字串 | 等寬文字 |

  - 同一個副檔名有外掛時，外掛優先於內建的預覽（所以也可以換掉內建的 Markdown 預覽）。外掛有錯（沒定義 `preview`、丟出例外、逾時、回傳格式不對…）會在預覽窗格裡顯示原因；檔頭有問題的外掛會略過，原因放進輸出面板。
  - 目前外掛只能選上面這幾種呈現方式，也不支援呼叫外部程式。
