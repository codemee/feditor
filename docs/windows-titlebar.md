[English](windows-titlebar.en.md) | **繁體中文**

[← 回到 README](../README.md)

# 自繪標題列與 Windows 11 貼齊版面

Windows 版把系統標題列拿掉，改由**主工具列兼任標題列**。這份文件記錄它的版面、拖曳與縮放的做法，以及最麻煩的一個問題：怎麼讓 Windows 11 的「貼齊版面」（Snap Layouts）選單在自繪的最大化鈕上出現。使用者操作請看 [README](../README.md)。

| 平台 | 標題列 | 視窗邊緣縮放 |
|---|---|---|
| Windows | 自繪（本文件） | 自己攔 `WM_NCHITTEST`（`ui/src/win_frame.rs`） |
| macOS、Linux | 系統原生 | 系統處理 |

macOS、Linux 目前**沒有**啟用：自繪標題列要自己處理視窗邊緣縮放，這部分只有 Windows 做了。開關是 `ui/src/window_chrome.rs` 的 `CUSTOM`。

## 版面

![寬視窗的工具列](images/titlebar-wide.png)

- 最左邊是應用程式圖示，最右邊是設定「⋯」與最小化、最大化（還原）、關閉三顆鈕。
- 工具列的空白處可以拖曳視窗，雙按切換最大化與還原。
- 視窗標題（工作列、Alt+Tab 看到的）仍然設成「檔名 - FEditor」，只是不畫在視窗裡；版本號改放設定面板的「關於」頁。
- 標籤列一律顯示（單一窗格、單一分頁也一樣），所以工具列固定在最上面，版面不會因為窗格數量跳動。

### 放不下時：收進「更多」

![窄視窗的工具列](images/titlebar-narrow.png)

工具列分成 6 組，**整組**一起顯示或一起收進 `»` 下拉清單：

| 序 | 群組 | 內容 |
|---|---|---|
| 0 | 檔案 | 新增、開啟（含最近開啟）、儲存、另存新檔 |
| 1 | 格式 | 轉換縮排、排列格式、用文字包住 |
| 2 | 尋找 | 尋找與取代 |
| 3 | 終端機 | 新增終端機（含 shell 清單）、執行檔案（含命令列參數） |
| 4 | 預覽 | 預覽開關 |
| 5 | 巨集 | 巨集面板、重跑、錄製 |

設定鈕與三顆視窗按鈕不在這六組裡，**固定靠右**，不會被收起來。

做法（`App::toolbar`）：

1. 每一組畫完就量出實際寬度，記在 egui 的暫存資料（`toolbar_group_widths`）；第一個畫面全部照畫，下一個畫面才有寬度可用。
2. 可用寬度 = 工具列右緣 − 右側固定區（視窗按鈕、設定鈕）− 圖示佔掉的寬度。
3. 全部放得下就都畫；放不下就從最後一組開始往前收，直到「已顯示的組 + 分隔線 + 『更多』鈕」放得下。
4. 被收起來的組，改畫在 `»` 的下拉清單裡，每組一列，用的是**同一個繪製函式**（`App::toolbar_group`），所以按鈕行為完全一致。
5. 巨集面板展開時需要錨點：巨集鈕被收起來時，改以 `»` 鈕的位置當錨點。
6. 最小視窗寬度是 480 點（`MIN_WINDOW_WIDTH`），放得下圖示、`»`、設定鈕和三顆視窗按鈕。

## 拖曳與最大化

- 工具列先登記一個整條的拖曳區（`window_chrome::drag_area`），**要在其他按鈕之前登記**，按鈕疊在它上面，點到按鈕才不會變成拖曳視窗。
- 按下開始拖曳時送 `ViewportCommand::StartDrag`，由系統移動視窗；雙按送 `Maximized(!目前狀態)`。
- 關閉鈕送 `ViewportCommand::Close`，走一般的關閉流程，有未儲存的修改會先詢問。

## Windows 的視窗程序

拿掉系統標題列之後，winit 的無框視窗有兩個問題：視窗邊緣大部分抓不到（只有最上緣能縮放），以及最大化鈕上沒有貼齊版面選單。`win_frame::install` 在視窗建好之後，用 `SetWindowLongPtrW(GWLP_WNDPROC)` 掛上自己的視窗程序，攔三種訊息。

不用 `SetWindowSubclass`：它在 comctl32 裡，沒有 manifest 要求 v6 時，載到的舊版不一定匯出這個函式。

### `WM_NCCALCSIZE`：保留原生邊框，只收回標題列

這是讓貼齊版面出現的關鍵。

1. `wParam` 為 0 時直接回傳 0。
2. 否則先記下系統原本給的客戶區**上緣**，再呼叫 `DefWindowProcW`，讓 Windows 照常算出原生邊框。
3. 保留系統算出的**左、右、下**邊框，只把客戶區上緣改回原本的上緣：標題列的位置收回客戶區，交給 egui 畫。
4. 最大化時，原生縮放邊框會落在螢幕外，上緣要再加上 `GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)`，不然內容頂端會被裁掉。
5. 裝好視窗程序後立刻 `SetWindowPos(SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER)`，強迫系統重算一次邊框。

「持續保留原生邊框」是重點：不能只在啟動時初始化一次，之後又把整個非客戶區清空。

### `WM_NCHITTEST`：縮放區與最大化鈕

依序判斷，第一個符合的就回傳：

| 順序 | 條件 | 回傳 |
|---|---|---|
| 1 | 視窗沒有最大化，且在邊緣 6 個點內（四個角各放大到 12 點） | `HTLEFT`、`HTTOP`、`HTTOPLEFT`… |
| 2 | 在自繪最大化（還原）鈕的範圍內 | `HTMAXBUTTON` |
| 3 | 其他 | `HTCLIENT` |

- 邊緣寬度依 DPI 放大（`GetDpiForWindow`）。
- `lParam` 的座標是**有號 16 位元**，多螢幕時可能是負的，要用 `as i16` 轉。
- 最大化鈕的範圍每個畫面由 UI 回報（`set_max_button`，換成實體像素、客戶區座標），命中時用 `ScreenToClient` 換算。
- 第 3 項明確回 `HTCLIENT`，避免系統把已經不存在的原生標題列或隱藏的按鈕當成命中。

### 最大化鈕上的滑鼠訊息

回傳 `HTMAXBUTTON` 之後，滑鼠在那顆鈕上的訊息會變成非客戶區訊息，**不會到 egui**，所以要自己處理：

| 訊息 | 處理 |
|---|---|
| `WM_NCMOUSEMOVE`（在最大化鈕上） | 設懸停旗標，UI 畫灰底；訊息**照常交還系統**，貼齊版面靠它觸發 |
| `WM_NCMOUSELEAVE`、`WM_MOUSEMOVE` | 清掉懸停旗標 |
| 懸停旗標亮著時 | 每個畫面用 `GetCursorPos` 重新確認游標還在鈕上，並約每 60 毫秒重畫，滑鼠離開才會熄 |
| `WM_NCLBUTTONDOWN` | 回傳 0，不交給系統（它會去畫不存在的標題列按鈕） |
| `WM_NCLBUTTONUP` | 送 `WM_SYSCOMMAND`，最大化時是 `SC_RESTORE`，否則 `SC_MAXIMIZE` |

代價：滑鼠在最大化鈕上時 egui 看不到游標，所以這顆鈕沒有 tooltip。

## 問題：貼齊版面選單不出現

### 現象

自繪的最大化鈕已經能點、能最大化、有灰底，診斷記錄也證明 `WM_NCHITTEST` 對那個位置回傳了 `HTMAXBUTTON`、`WM_NCMOUSEMOVE` 有送進來，但滑鼠停上去，Windows 11 的貼齊版面選單始終不出現。同一個程式改用原生標題列時，選單正常。

### 根因

winit 的無框模式把整個非客戶區都併進客戶區。這種視窗即使 `WM_NCHITTEST` 回報 `HTMAXBUTTON`，Windows 11 也不會顯示貼齊版面；視窗要**保留原生邊框**（只讓出標題列）才會顯示。微軟文件講自繪標題列時只提到要回報 `HTMAXBUTTON`，沒有看到這個條件；這是對照原生標題列與 Windows Terminal 的做法、實測得到的結論。

### 解法

照 Windows Terminal 的 `NonClientIslandWindow::_OnNcCalcSize` 做：攔 `WM_NCCALCSIZE`，保留邊框、只收回標題列（見上面「`WM_NCCALCSIZE`」一節）。

### 試過但沒有效的做法

| 做法 | 結果 |
|---|---|
| 只回報 `HTMAXBUTTON` | 命中、灰底、點擊都正常，沒有選單 |
| 自己呼叫 `TrackMouseEvent` 追蹤離開 | 沒有選單 |
| `DwmExtendFrameIntoClientArea` 擴展框架 | 沒有選單 |
| `DwmDefWindowProc` 額外轉送訊息 | 沒有選單 |
| 只在啟動時初始化一次原生標題列 | 沒有選單 |
| 加 Windows 相容性 manifest | 沒有選單 |

另外，網路上 Tauri 的「疊一個透明子視窗回報 `HTMAXBUTTON`」的做法不適用：那是因為 WebView2 的子視窗擋住了滑鼠，頂層視窗的 `WM_NCHITTEST` 根本沒被問到。FEditor 沒有子視窗，頂層視窗收得到滑鼠訊息。

### 驗證與除錯

- **對照**：同一個程式切回原生標題列，看選單是否出現，先確認系統設定（設定 → 系統 → 多工 → 貼齊視窗）沒關掉。
- **直接問視窗程序**：用 `SendMessage(hwnd, WM_NCHITTEST, 0, MAKELPARAM(x, y))` 問幾個位置（左緣、四角、最大化鈕、視窗中央），確認回傳 `HTLEFT`、`HTBOTTOMLEFT`、`HTMAXBUTTON`、`HTCLIENT`，不需要動滑鼠。
- **擷圖看不到**：`PrintWindow` 拍不到 DWM 疊上來的貼齊版面選單，要用 Windows.Graphics.Capture 或直接用眼睛看。

## 參考

- [Windows Terminal：`NonClientIslandWindow.cpp`](https://github.com/microsoft/terminal/blob/main/src/cascadia/WindowsTerminal/NonClientIslandWindow.cpp)（`_OnNcCalcSize`）
- [Chromium：`hwnd_message_handler.cc`](https://github.com/chromium/chromium/blob/main/ui/views/win/hwnd_message_handler.cc)
- [Microsoft Learn：Support snap layouts for desktop apps on Windows 11](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-snap-layout-menu)
