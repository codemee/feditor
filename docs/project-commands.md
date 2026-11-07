[English](project-commands.en.md) | **繁體中文**

# 專案命令（設計草案）

> 狀態：第 10 節的第 1、2 階段已實作（Cargo、uv、poetry、`.venv` 腳本；程式在 `ui/src/project_cmd.rs`），第 3 階段（PEP 723、pytest、npm、Makefile、`go`）還沒做；實作與草案的差異在最後的「實作備註」。目的是讓「在終端機執行」除了「用哪個直譯器跑這個檔案」之外，還能列出專案設定檔提供的命令（`cargo run`、`uv run`…），並且只在對應的工具真的裝了的時候才提供可以按的項目。

## 1. 目標與不做的事

**目標**

- 開啟 `.py` 檔時，除了現在找到的直譯器，也從檔案所在資料夾往上找專案設定檔，把其中可以執行的命令列為選項。
- 開啟 `.rs` 檔時同樣：找到 `Cargo.toml` 就提供 `cargo run`、`cargo test` 等選項。
- **工具不在系統上時，不顯示按了也執行不了的項目**。只顯示一行說明（例如「找到使用 uv 的專案設定，但系統上沒有 uv」）。
- 探測階段只讀檔案、只查工具有沒有、版本多少；**不執行專案裡的任何命令**（沿用現在 `discover` 的原則）。

**不做的事（至少第一版不做）**

- 不做外掛式的提供者（理由見第 8 節）。
- 不自動執行任何專案命令；一定是使用者明確選了再按執行。
- 不替使用者安裝工具，只說明缺什麼、提示怎麼裝。
- 不改現有的直譯器搜尋（`Rule`、`discover`、`Resolver` 照舊）。

## 2. 名詞

| 名詞 | 意思 |
|---|---|
| 提供者（provider） | 懂一種專案設定檔的程式，例如 Cargo、Python（`pyproject.toml`）。輸入是檔案路徑，輸出是「發現」。 |
| 發現（finding） | 提供者在某個設定檔裡找到的東西：一組命令，加上這些命令需要的工具。 |
| 專案命令（`ProjectCommand`） | 一個可以執行的選項：顯示名稱、程式、參數、執行資料夾、來源設定檔。 |
| 工具狀態（`ToolStatus`） | 命令需要的工具在系統上的情況：可用（有路徑與版本）或缺少。 |

## 3. 資料結構

```rust
/// 一個專案命令（探測完、確認能執行之後才會變成可以按的項目）。
pub struct ProjectCommand {
    /// 設定裡記住手動選擇用的穩定識別，例如 `cargo:run`、`cargo:example:demo`、`uv:script:serve`。
    pub id: String,
    /// 選單上顯示的名稱，例如 `cargo run --example demo`。
    pub label: String,
    pub program: PathBuf,        // 已經解析成實際路徑
    pub args: Vec<String>,       // 固定的參數
    pub cwd: PathBuf,            // 設定檔所在的資料夾
    pub source: PathBuf,         // 來源設定檔，顯示用（Cargo.toml）
    /// 命令後面是否接目前的檔案（`uv run 檔案.py` 要，`cargo run` 不要）。
    pub takes_file: bool,
}

pub enum ToolStatus {
    Available { path: PathBuf, version: String },
    /// 找不到，或找到了但跑不起來（例如 rustup 的 `cargo` 沒有安裝任何工具鏈）。
    Missing { tool: &'static str, reason: MissingReason },
}

pub enum MissingReason { NotFound, Broken(String) }

/// 一個提供者對一個設定檔的發現。
pub struct Finding {
    pub provider: &'static str,  // "Cargo"、"uv"、"poetry"
    pub source: PathBuf,
    pub tool: &'static str,      // 需要的工具名稱
    pub commands: Vec<CommandSpec>, // 還沒綁定工具路徑的命令（只有名稱與參數）
}
```

提供者只負責「讀設定檔、產生 `CommandSpec`」，**不碰工具偵測**；工具偵測由共用的一個函式做。這樣提供者可以用假的檔案系統測，工具偵測也可以用假的 `PATH` 測（和現在 `discover` 注入 `env`、`path_var` 的做法一致）。

## 4. 探測流程

```
開啟檔案 / 切換分頁
   └─ Resolver（背景執行緒，沿用現有的快取與重新搜尋）
        ├─ 1. 直譯器：照現在的 discover（不變）
        └─ 2. 專案命令（新）
             a. 依語言挑提供者（Python → pyproject；Rust → Cargo）
             b. 從檔案所在資料夾往上找設定檔
                （停止條件同 project_root：有 .git、家目錄、磁碟根目錄）
             c. 提供者讀設定檔 → Finding（讀不了或解析失敗就略過，不報錯）
             d. 對每個 Finding 的 tool 做工具偵測 → ToolStatus
             e. 組成結果：可執行的 ProjectCommand 清單 + 「缺少工具」的說明清單
```

- 結果放在現有 `Resolved` 旁邊（`Resolved.items` 是直譯器，另加 `project: ProjectResolved`），共用同一份背景執行緒與 30 秒／5 秒的過期重找。使用者裝好工具後按現有的「重新搜尋」就會更新。
- 設定檔大小設上限（例如 1 MB），超過就略過，避免卡在巨大的檔案。
- 從上往下找到的設定檔可能有好幾份（巢狀的 workspace）。**最近的那份優先**；Cargo 的 workspace 成員用自己的 `Cargo.toml`，在成員資料夾裡執行 `cargo run` 就是 cargo 的標準行為。

### 工具偵測（本設計的重點）

一個函式 `detect_tool(spec) -> ToolStatus`：

1. 在 `PATH` 找（沿用 `program_path_in` 與 `skip_paths`，Windows 同樣排除 `WindowsApps` 的假捷徑；也試加上 `.exe`、`.cmd`）。
2. `PATH` 沒有時試固定位置：`cargo` 試 `~/.cargo/bin`（rustup 預設位置，從 GUI 啟動時 `PATH` 常常不含它）；`uv` 試 `~/.local/bin`、`~/.cargo/bin`。
3. 找到之後在背景用 `--version` 確認真的能跑（逾時 4 秒，沿用 `probe_version`）。跑不起來就是 `Broken`：
   - rustup 的 `cargo` 沒有預設工具鏈 → `Broken("no default toolchain")`，提示 `rustup default stable`。
   - macOS 沒裝開發工具時的 `python3` 殼層 → 同理。
4. 同一個工具在同一輪搜尋只偵測一次（`cargo` 的五個命令共用一個結果）。

**規則：工具不是 `Available`，它的命令一律不進入可選清單。** 只產生一筆「缺少工具」的說明。

### 哪些命令需要哪個工具

重點：**需要的工具取決於「怎麼執行」，不是取決於「找到了什麼設定檔」**。

| 情況 | 執行方式 | 需要 |
|---|---|---|
| `Cargo.toml` | `cargo run`、`cargo test`… | `cargo` |
| `pyproject.toml` 有 `[project.scripts]`，專案裡已經有 `.venv` | 直接跑 `.venv/bin/名稱`（Windows `.venv\Scripts\名稱.exe`） | 不需要 `uv` |
| 同上，但沒有 `.venv`，專案用 uv（`uv.lock` 或 `[tool.uv]`） | `uv run 名稱` | `uv` |
| 專案用 poetry（`poetry.lock` 或 `[tool.poetry]`） | `poetry run 名稱` | `poetry` |

所以「`.venv` 已經建好、`uv` 沒裝」時仍然能列出腳本命令（走 `.venv`），不會因為缺 `uv` 就少掉可用的選項。

## 5. 缺少工具時怎麼呈現

- **不放可以按的項目。** 選單裡不出現灰掉的 `uv run …`，因為灰掉的項目看起來像壞了，也容易被誤會成功能不見。
- 改放**一行不能按的說明文字**（`label` 樣式，淡色），放在專案命令那一段的最後：
  - `找到使用 uv 的專案設定（pyproject.toml、uv.lock），但系統上沒有 uv`
  - `找到 Cargo.toml，但系統上沒有 cargo`
  - `找到 Cargo.toml，但 cargo 無法執行（沒有預設工具鏈）；試試 rustup default stable`
- 說明文字的 tooltip 寫安裝提示：Windows 優先 `scoop install uv`，macOS／Linux 優先 `brew install uv`，其他方式放官方文件網址。**只提示，不替使用者執行。**
- **只有證據夠強才顯示說明**，避免誤報：
  - uv：要有 `uv.lock` 或 `pyproject.toml` 的 `[tool.uv]`。光有 `pyproject.toml` 不代表使用者要用 uv，不顯示。
  - poetry：要有 `poetry.lock` 或 `[tool.poetry]`。
  - cargo：有 `Cargo.toml` 就算。
- 同一個缺少的工具只顯示一行，不管它影響幾個命令。
- 狀態列上的直譯器按鈕不受影響。這些說明只出現在下拉清單裡。

## 6. 提供者的範圍

### 6.1 Cargo（`.rs` 檔）

從檔案往上找最近的 `Cargo.toml`，依檔案位置給出命令：

| 檔案位置 | 命令 |
|---|---|
| 有 `src/main.rs` 或 `[[bin]]` 的 crate | `cargo run`（`[[bin]]` 多個時用 `--bin 名稱`） |
| `src/bin/名稱.rs`（或 `src/bin/名稱/main.rs`） | `cargo run --bin 名稱` |
| `examples/名稱.rs`（或 `examples/名稱/main.rs`） | `cargo run --example 名稱` |
| `tests/名稱.rs` | `cargo test --test 名稱` |
| 任一個 crate 檔案 | `cargo test`、`cargo build` |
| `src/lib.rs`、沒有任何 bin 的 crate | 只有 `cargo test`、`cargo build` |

- 目前檔案對應得到的命令排在前面（例如在 `examples/demo.rs` 就把 `--example demo` 排第一）。
- 命令的執行資料夾是 `Cargo.toml` 所在的資料夾；使用者輸入的「額外參數」接在 `--` 後面（給程式本身）。
- 需要讀 `Cargo.toml`：專案裡已經有 TOML 解析（預覽功能用的），第一版沿用，不新增相依套件（實作時再確認）。

### 6.2 Python（`.py` 檔）

從檔案往上找最近的 `pyproject.toml`：

| 發現 | 命令 |
|---|---|
| 專案用 uv | `uv run 檔案`、`uv run 腳本名稱`（每個 `[project.scripts]` 一項） |
| 專案用 poetry | `poetry run python 檔案`、`poetry run 腳本名稱` |
| 專案裡已有 `.venv` | `.venv` 裡的 `腳本名稱`（不需要 uv／poetry，見上一節） |
| 檔名是 `test_*.py` 或 `*_test.py`，且 `.venv` 或專案相依裡有 pytest | `pytest 檔案`（第二版再做） |

- `uv run 檔案` 的價值：專案還沒建 `.venv` 時，uv 會替你建好並裝相依；這是直譯器搜尋做不到的。
- 單檔腳本的 PEP 723 內嵌設定（`# /// script`）：有就提供 `uv run 檔案`（需要 uv），放第三階段。

### 6.3 之後可以加的

npm（`package.json` 的 `scripts`）、`Makefile`／`justfile`（目標清單）、`go.mod`（`go run`、`go test`）。這些命令清單可能很長，要先做好數量上限與搜尋（見第 9 節）。

## 7. 介面

下拉清單分成兩段，每段有小標題：

```
直譯器
  ● Python 3.12.1 · .venv
    Python 3.11.9 · PATH
專案命令（Cargo.toml）
    cargo run --example demo
    cargo test
    cargo build
    找到使用 uv 的專案設定，但系統上沒有 uv        ← 淡色、不能按
```

- **預設仍是直譯器**（現在的行為）。使用者選了專案命令之後才改用它，而且記在設定裡（鍵：`專案#cmd:id`，沿用現有 `choice_key` 的寫法，舊設定不受影響）。這樣升級後沒有人會發現原本按「執行」的行為變了。
- 選了專案命令之後，狀態列按鈕顯示 `cargo run --example demo`；命令不在了（設定檔改了、工具被移除）就退回第一個可用的直譯器。
- 執行時開（或沿用）終端機，`cwd` 用命令的 `cwd`，不是檔案所在的資料夾；`takes_file` 為真才接檔案路徑。
- 「額外參數」欄位的意思不變，接在命令最後（Cargo 接在 `--` 後）。

## 8. 為什麼用內建提供者、不做成外掛

- 現有的外掛（巨集、預覽）跑在沙盒裡，不能讀檔案、不能連網路。要讓外掛找專案設定檔，得先設計一組新的主機 API（往上找資料夾、讀檔、查工具），等於重做一次安全邊界。
- 要支援的設定檔只有幾種，格式固定，寫在程式裡好測，三個平台行為一致。
- 提供者的介面（輸入檔案路徑、輸出 `Finding`）刻意做得很窄：之後真的有人想自己加語言，這個介面可以直接開放成外掛 API，不用重新設計。到那時再決定要不要做。

## 9. 風險與要先想清楚的事

- **安全**：`package.json` 的 `scripts`、Makefile、`pyproject.toml` 的腳本都等於任意命令。探測階段不執行；選單上一律顯示完整命令列；不自動執行；使用者換了專案資料夾不沿用上一個專案的選擇（選擇記憶本來就以專案為單位）。
- **數量**：Makefile 之類可能有幾十個目標。需要上限（例如 12 個，其餘收進「更多…」），或做成可以輸入搜尋的清單。第一版只有 Cargo 與 Python，不會遇到，但資料結構要先留。
- **跨平台**：Windows 的 `cargo.exe`、`uv.exe`、`.cmd` 包裝；PATH 在 GUI 啟動時常常不完整（所以才有固定位置的備援）；`.venv` 裡可執行檔在 Windows 是 `Scripts\名稱.exe`。
- **終端機的環境**：現有 `terminal_env` 會在使用者選了 `.venv` 的直譯器時，替終端機加上 `VIRTUAL_ENV` 與 `PATH`。專案命令要不要一起套用，需要決定（建議：`uv`、`poetry` 自己管環境，不套用；`.venv` 腳本套用）。
- **設定檔解析失敗**：略過，不彈錯誤。但在說明行顯示「`Cargo.toml` 讀不了」比靜默消失好，第二版再決定。
- **版本探測成本**：每個工具 `--version` 要啟動一個程序（`uv`、`cargo` 都很快，`cargo` 在 rustup 下偶爾會慢）。結果在同一輪搜尋內共用，並且沿用 30 秒快取。

## 10. 實作階段

1. **骨架**：新增 `ui/src/project_cmd.rs`（資料結構、工具偵測、`Finding` 與提供者介面）、`Resolved` 加專案命令欄位、下拉清單分段顯示、`run_in_terminal` 支援「程式＋參數＋cwd」。先只做 Cargo。測試：用假檔案系統與假 `PATH`，涵蓋「有工具」、「沒工具」、「工具壞掉」三種結果，確認第二、三種只產生說明、不產生可以按的項目。
2. **Python**：uv／poetry／`.venv` 腳本，以及缺少工具的說明。
3. **擴充**：PEP 723、pytest、npm、Makefile、`go`；做數量上限與搜尋。
4. 之後視需求評估要不要把提供者介面開放成外掛。

## 11. 測試重點

- 提供者：給定檔案路徑與假的檔案內容，輸出的 `Finding` 與命令清單（含 workspace、`examples/`、`src/bin/` 的對應）。
- 工具偵測：`PATH` 上有、沒有、有但跑不起來、在固定位置、被 `skip_paths` 排除。
- 組合：`.venv` 已存在但沒有 uv → 仍列出 `.venv` 腳本；沒有 `.venv` 且沒有 uv → 只有說明；只有 `pyproject.toml` 沒有 uv 的證據 → 什麼都不顯示。
- 介面：缺少工具的說明不是按鈕（不能被點擊、不能被鍵盤選到）。
- 選擇記憶：舊的設定檔照樣讀得進來；選到已經消失的命令時退回直譯器。

## 12. 實作備註

- 資料結構與草案略有不同：提供者產生 `Finding`（含 `CommandSpec`，還沒綁定工具路徑），`resolve` 偵測工具後才變成 `ProjectCommand`；`.venv` 腳本的 `CommandSpec.program` 直接是腳本路徑，`Finding.tool` 是 `None`。
- 選擇記憶：鍵是 `專案#cmd:語言`（例如 `/code/app#cmd:Python`），值是命令的 `id`，存在原本的 `runtime_choices`；改選直譯器或「自動」時一併清掉同一專案的專案命令選擇。
- 說明文字統一成「找到專案設定（依據），但系統上沒有 工具」與「…但 工具 無法執行：原因」，依據是 `Cargo.toml`、`pyproject.toml, uv.lock` 這類。
- 專案命令在選單裡的標題是「專案命令（Cargo.toml）」；選單項目的 tooltip 是執行的資料夾與完整命令列。
- 沒有直譯器的語言（Rust）預設用第一個專案命令；Python 的直譯器都不能用時也一樣。
- 專案命令的搜尋結果依（語言, 檔案路徑）快取（和檔案位置有關），30 秒過期（什麼都沒有時 5 秒）；「重新搜尋」會一併重找。
