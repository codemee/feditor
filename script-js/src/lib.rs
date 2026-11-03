//! feditor 的 JavaScript 巨集引擎（QuickJS，經由 rquickjs）。
//!
//! 巨集可使用的全域物件：
//!
//! | API | 說明 |
//! |---|---|
//! | `editor.text()` | 完整文字 |
//! | `editor.length()` | 總字元數 |
//! | `editor.lineCount()` | 行數 |
//! | `editor.line(i)` | 第 `i` 行（含換行），超出範圍為 `undefined` |
//! | `editor.insert(at, text)` | 在位置 `at` 插入 |
//! | `editor.remove(start, end)` | 刪除 `[start, end)` |
//! | `editor.replace(start, end, text)` | 取代 `[start, end)` |
//! | `editor.slice(start, end)` | `[start, end)` 的文字 |
//! | `editor.lineStart(i)` / `editor.lineOf(pos)` | 第 `i` 行的起點／位置 `pos` 在第幾行 |
//! | `editor.selection()` | `[錨點, 游標]` |
//! | `editor.setSelection(anchor, head)` | 設定選取 |
//! | `editor.selectedText()` | 選取的文字 |
//! | `editor.path()` | 目前檔案的完整路徑，沒存過為 `undefined` |
//! | `editor.run(name, count = 1)` | 執行編輯器指令（`selectAll`、`moveLineDown`…），與按鍵相同 |
//! | `editor.type(text)` / `editor.paste(text)` | 像使用者打字／貼上（取代選取、游標跟著移動） |
//! | `editor.find(query, from = 0, opts)` | 找到回傳 `[start, end]`，否則 `undefined`；`opts`：`regex`、`caseSensitive`、`wholeWord`、`backward` |
//! | `editor.alert(msg)` / `editor.confirm(msg)` | 對話框；`confirm` 回傳是否按了「是」 |
//! | `console.log(...)` | 輸出，由 [`JsEngine::take_output`] 取回 |
//!
//! **位置單位是 Unicode 碼點（code point），不是 JavaScript 字串的 UTF-16 單位。**
//! 含有 emoji 等補充平面字元時，`"😀".length` 是 2，但 `editor.length()` 只算 1。
//!
//! 每次 [`run`](ScriptEngine::run) 都使用全新的 context，巨集之間不會共用全域變數。

use feditor_core::i18n::t;
use feditor_core::tf;
use feditor_script::{EditorApi, FindOptions, ScriptEngine, ScriptError};
use rquickjs::convert::Coerced;
use rquickjs::function::{Opt, Rest};
use rquickjs::{CatchResultExt, Context, Ctx, Exception, Function, Object, Runtime};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// 預設的執行時間上限。
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
/// 預設的記憶體上限（64 MiB）。
pub const DEFAULT_MEMORY_LIMIT: usize = 64 * 1024 * 1024;

pub struct JsEngine {
    runtime: Runtime,
    timeout: Duration,
    output: Rc<RefCell<Vec<String>>>,
}

impl JsEngine {
    pub fn new() -> Result<Self, ScriptError> {
        let runtime = Runtime::new().map_err(|e| ScriptError(e.to_string()))?;
        runtime.set_memory_limit(DEFAULT_MEMORY_LIMIT);
        Ok(Self {
            runtime,
            timeout: DEFAULT_TIMEOUT,
            output: Rc::default(),
        })
    }

    /// 設定單次巨集的執行時間上限。
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    /// 取回並清空 `console.log` 累積的輸出。
    pub fn take_output(&mut self) -> Vec<String> {
        std::mem::take(&mut *self.output.borrow_mut())
    }
}

/// 執行期間暫時借出的編輯器指標。只在 `run` 期間有值，結束（含 panic）時一律清除。
type HostCell = Rc<Cell<Option<*mut dyn EditorApi>>>;

struct ClearOnDrop(HostCell);

impl Drop for ClearOnDrop {
    fn drop(&mut self) {
        self.0.set(None);
    }
}

/// 以借出的編輯器執行 `f`。巨集在 `run` 之外不可能呼叫到這裡，但仍檢查以防萬一。
fn with_editor<R>(
    host: &HostCell,
    ctx: &Ctx<'_>,
    f: impl FnOnce(&mut dyn EditorApi) -> R,
) -> rquickjs::Result<R> {
    let Some(ptr) = host.get() else {
        return Err(Exception::throw_type(ctx, t("editor 目前無法使用")));
    };
    // SAFETY: 指標來自 `run` 持有的 `&mut dyn EditorApi`，`run` 在 eval 期間不會再使用該參考，
    // 且 `ClearOnDrop` 保證 `run` 返回前指標已清除；這裡的 `&mut` 只在 `f` 內存活，不會重疊。
    Ok(f(unsafe { &mut *ptr }))
}

/// 把 JS 數字轉成位置索引；負數、NaN、無限大、非整數都丟出 `RangeError`。
fn to_index(ctx: &Ctx<'_>, name: &str, v: f64) -> rquickjs::Result<usize> {
    if v.is_finite() && v >= 0.0 && v.fract() == 0.0 && v <= u32::MAX as f64 * 4096.0 {
        Ok(v as usize)
    } else {
        Err(Exception::throw_range(
            ctx,
            &tf!("{name} 必須是非負整數，收到 {v}", name = name, v = v),
        ))
    }
}

fn install_api<'js>(
    ctx: &Ctx<'js>,
    host: &HostCell,
    output: &Rc<RefCell<Vec<String>>>,
) -> rquickjs::Result<()> {
    // 注意：閉包不可捕獲 `Ctx`，否則會形成 context → 函式 → 閉包 → context 的循環參考，
    // 導致 runtime 釋放時 QuickJS 斷言失敗。需要 ctx 時一律用注入的第一個參數。
    let editor = Object::new(ctx.clone())?;

    let h = host.clone();
    editor.set(
        "text",
        Function::new(ctx.clone(), move |c: Ctx<'_>| {
            with_editor(&h, &c, |e| e.text())
        }),
    )?;

    let h = host.clone();
    editor.set(
        "length",
        Function::new(ctx.clone(), move |c: Ctx<'_>| {
            with_editor(&h, &c, |e| e.len_chars() as f64)
        }),
    )?;

    let h = host.clone();
    editor.set(
        "lineCount",
        Function::new(ctx.clone(), move |c: Ctx<'_>| {
            with_editor(&h, &c, |e| e.line_count() as f64)
        }),
    )?;

    let h = host.clone();
    editor.set(
        "line",
        Function::new(ctx.clone(), move |c: Ctx<'_>, i: f64| {
            let i = to_index(&c, "line", i)?;
            with_editor(&h, &c, |e| e.line(i))
        }),
    )?;

    let h = host.clone();
    editor.set(
        "insert",
        Function::new(ctx.clone(), move |c: Ctx<'_>, at: f64, text: String| {
            let at = to_index(&c, "at", at)?;
            with_editor(&h, &c, |e| e.insert(at, &text))
        }),
    )?;

    let h = host.clone();
    editor.set(
        "remove",
        Function::new(ctx.clone(), move |c: Ctx<'_>, start: f64, end: f64| {
            let (start, end) = (to_index(&c, "start", start)?, to_index(&c, "end", end)?);
            with_editor(&h, &c, |e| e.remove(start..end))
        }),
    )?;

    let h = host.clone();
    editor.set(
        "replace",
        Function::new(
            ctx.clone(),
            move |c: Ctx<'_>, start: f64, end: f64, text: String| {
                let (start, end) = (to_index(&c, "start", start)?, to_index(&c, "end", end)?);
                with_editor(&h, &c, |e| e.replace(start..end, &text))
            },
        ),
    )?;

    let h = host.clone();
    editor.set(
        "slice",
        Function::new(ctx.clone(), move |c: Ctx<'_>, start: f64, end: f64| {
            let (start, end) = (to_index(&c, "start", start)?, to_index(&c, "end", end)?);
            with_editor(&h, &c, |e| e.slice(start..end))
        }),
    )?;

    let h = host.clone();
    editor.set(
        "lineStart",
        Function::new(ctx.clone(), move |c: Ctx<'_>, i: f64| {
            let i = to_index(&c, "line", i)?;
            with_editor(&h, &c, |e| e.line_start(i) as f64)
        }),
    )?;

    let h = host.clone();
    editor.set(
        "lineOf",
        Function::new(ctx.clone(), move |c: Ctx<'_>, pos: f64| {
            let pos = to_index(&c, "pos", pos)?;
            with_editor(&h, &c, |e| e.line_of(pos) as f64)
        }),
    )?;

    let h = host.clone();
    editor.set(
        "selection",
        Function::new(ctx.clone(), move |c: Ctx<'_>| {
            with_editor(&h, &c, |e| {
                let (a, b) = e.selection();
                vec![a as f64, b as f64]
            })
        }),
    )?;

    let h = host.clone();
    editor.set(
        "setSelection",
        Function::new(ctx.clone(), move |c: Ctx<'_>, anchor: f64, head: f64| {
            let (a, b) = (to_index(&c, "anchor", anchor)?, to_index(&c, "head", head)?);
            with_editor(&h, &c, |e| e.set_selection(a, b))
        }),
    )?;

    let h = host.clone();
    editor.set(
        "selectedText",
        Function::new(ctx.clone(), move |c: Ctx<'_>| {
            with_editor(&h, &c, |e| {
                let (a, b) = e.selection();
                e.slice(a.min(b)..a.max(b))
            })
        }),
    )?;

    let h = host.clone();
    editor.set(
        "path",
        Function::new(ctx.clone(), move |c: Ctx<'_>| with_editor(&h, &c, |e| e.path())),
    )?;

    let h = host.clone();
    editor.set(
        "run",
        Function::new(ctx.clone(), move |c: Ctx<'_>, name: String, count: Opt<f64>| {
            let count = match count.0 {
                Some(n) => to_index(&c, "count", n)?,
                None => 1,
            };
            match with_editor(&h, &c, |e| e.run_command(&name, count))? {
                Ok(()) => Ok(()),
                Err(msg) => Err(Exception::throw_message(&c, &msg)),
            }
        }),
    )?;

    let h = host.clone();
    editor.set(
        "type",
        Function::new(ctx.clone(), move |c: Ctx<'_>, text: String| {
            with_editor(&h, &c, |e| e.type_text(&text))
        }),
    )?;

    let h = host.clone();
    editor.set(
        "paste",
        Function::new(ctx.clone(), move |c: Ctx<'_>, text: String| {
            with_editor(&h, &c, |e| e.paste_text(&text))
        }),
    )?;

    let h = host.clone();
    editor.set(
        "alert",
        Function::new(ctx.clone(), move |c: Ctx<'_>, msg: Coerced<String>| {
            with_editor(&h, &c, |e| e.alert(&msg.0))
        }),
    )?;

    let h = host.clone();
    editor.set(
        "confirm",
        Function::new(ctx.clone(), move |c: Ctx<'_>, msg: Coerced<String>| {
            with_editor(&h, &c, |e| e.confirm(&msg.0))
        }),
    )?;

    let h = host.clone();
    editor.set(
        "find",
        Function::new(
            ctx.clone(),
            move |c: Ctx<'_>, query: String, from: Opt<f64>, opts: Opt<Object<'_>>| {
                let from = match from.0 {
                    Some(n) => to_index(&c, "from", n)?,
                    None => 0,
                };
                let flag = |name: &str| -> bool {
                    opts.0
                        .as_ref()
                        .and_then(|o| o.get::<_, Option<bool>>(name).ok().flatten())
                        .unwrap_or(false)
                };
                let options = FindOptions {
                    regex: flag("regex"),
                    case_sensitive: flag("caseSensitive"),
                    whole_word: flag("wholeWord"),
                    backward: flag("backward"),
                };
                match with_editor(&h, &c, |e| e.find(&query, from, options))? {
                    Ok(found) => Ok(found.map(|r| vec![r.start as f64, r.end as f64])),
                    Err(msg) => Err(Exception::throw_message(&c, &msg)),
                }
            },
        ),
    )?;

    let console = Object::new(ctx.clone())?;
    let out = output.clone();
    console.set(
        "log",
        Function::new(ctx.clone(), move |args: Rest<Coerced<String>>| {
            let line = args.0.into_iter().map(|a| a.0).collect::<Vec<_>>().join(" ");
            out.borrow_mut().push(line);
        }),
    )?;

    let globals = ctx.globals();
    globals.set("editor", editor)?;
    globals.set("console", console)?;
    Ok(())
}

impl JsEngine {
    /// 執行預覽外掛：載入 `source`，呼叫全域函式 `preview(text, info)`，把回傳值轉成 JSON 字串。
    ///
    /// `info` 是 `{ path, name, ext }`（沒存過檔時 `path` 與 `name` 是 `undefined`）。
    /// 外掛跑在和巨集同樣的沙盒裡（有時間與記憶體上限、不能碰檔案與網路），而且拿不到 `editor`：
    /// 呼叫 `editor.*` 會丟出錯誤。`console.log` 的輸出可用 [`JsEngine::take_output`] 取回。
    pub fn convert(&mut self, source: &str, text: &str, path: Option<&str>) -> Result<String, ScriptError> {
        let host: HostCell = Rc::default(); // 沒有編輯器
        let deadline = Instant::now() + self.timeout;
        self.runtime
            .set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));

        let context = Context::full(&self.runtime).map_err(|e| ScriptError(e.to_string()))?;
        let output = self.output.clone();
        let result = context.with(|ctx| {
            (|| -> rquickjs::Result<String> {
                install_api(&ctx, &host, &output)?;
                ctx.eval::<(), _>(source)?;
                let globals = ctx.globals();
                let Ok(func) = globals.get::<_, Function>("preview") else {
                    return Err(Exception::throw_type(&ctx, t("外掛沒有定義 preview 函式")));
                };
                let info = Object::new(ctx.clone())?;
                if let Some(path) = path {
                    let p = std::path::Path::new(path);
                    info.set("path", path)?;
                    if let Some(name) = p.file_name() {
                        info.set("name", name.to_string_lossy().into_owned())?;
                    }
                    if let Some(ext) = p.extension() {
                        info.set("ext", ext.to_string_lossy().to_ascii_lowercase())?;
                    }
                }
                let value: rquickjs::Value = func.call((text, info))?;
                match ctx.json_stringify(value)? {
                    Some(json) => json.to_string(),
                    None => Err(Exception::throw_type(&ctx, t("preview 沒有回傳內容"))),
                }
            })()
            .catch(&ctx)
            .map_err(|e| ScriptError(e.to_string()))
        });

        self.runtime.set_interrupt_handler(None);
        result
    }
}

impl ScriptEngine for JsEngine {
    fn name(&self) -> &'static str {
        "javascript"
    }

    fn run(&mut self, source: &str, editor: &mut dyn EditorApi) -> Result<(), ScriptError> {
        let host: HostCell = Rc::default();
        let ptr: *mut (dyn EditorApi + '_) = editor;
        // SAFETY: 只是抹除生命週期以便存進 `'static` 的閉包；實際有效期間由 `ClearOnDrop` 限制在本函式內。
        let ptr: *mut (dyn EditorApi + 'static) = unsafe { std::mem::transmute(ptr) };
        host.set(Some(ptr));
        let _guard = ClearOnDrop(host.clone());

        let deadline = Instant::now() + self.timeout;
        self.runtime
            .set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));

        let context = Context::full(&self.runtime).map_err(|e| ScriptError(e.to_string()))?;
        let output = self.output.clone();
        let result = context.with(|ctx| {
            install_api(&ctx, &host, &output)
                .and_then(|()| ctx.eval::<(), _>(source))
                .catch(&ctx)
                .map_err(|e| ScriptError(e.to_string()))
        });

        self.runtime.set_interrupt_handler(None);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use feditor_core::Buffer;

    fn run(src: &str, buf: &mut Buffer) -> Result<Vec<String>, ScriptError> {
        let mut engine = JsEngine::new()?;
        engine.run(src, buf)?;
        Ok(engine.take_output())
    }

    fn convert(src: &str, text: &str, path: Option<&str>) -> Result<String, ScriptError> {
        JsEngine::new()?.convert(src, text, path)
    }

    #[test]
    fn preview_plugins_turn_text_into_json_and_see_the_file_info() {
        let src = "function preview(text, info) { return { header: ['行'], rows: text.split('\\n').map(l => [l]), name: info.name, ext: info.ext }; }";
        let json = convert(src, "a\nb", Some("C:\\x\\Report.LOG")).unwrap();
        assert!(json.contains("\"header\":[\"行\"]"), "{json}");
        assert!(json.contains("\"name\":\"Report.LOG\"") && json.contains("\"ext\":\"log\""), "{json}");
        // 沒有檔名時 name、ext 是 undefined，序列化時被略過
        let json = convert("function preview(t, i) { return { n: i.name, t }; }", "x", None).unwrap();
        assert_eq!(json, "{\"t\":\"x\"}");
        // 回傳字串也行
        assert_eq!(convert("function preview(t) { return '# ' + t; }", "hi", None).unwrap(), "\"# hi\"");
    }

    #[test]
    fn preview_plugins_report_missing_functions_empty_results_and_errors() {
        let e = convert("var x = 1;", "t", None).unwrap_err();
        assert!(e.to_string().contains("preview"), "{e}");
        let e = convert("function preview() {}", "t", None).unwrap_err();
        assert!(e.to_string().contains("preview"), "{e}");
        let e = convert("function preview() { throw new Error('壞掉了'); }", "t", None).unwrap_err();
        assert!(e.to_string().contains("壞掉了"), "{e}");
        assert!(convert("function preview( {", "t", None).is_err(), "語法錯誤");
    }

    #[test]
    fn preview_plugins_have_no_editor_and_are_stopped_when_they_loop_forever() {
        let e = convert("function preview() { editor.insert(0, 'x'); return 1; }", "t", None).unwrap_err();
        assert!(e.to_string().to_lowercase().contains("editor") || !e.to_string().is_empty(), "{e}");
        let mut engine = JsEngine::new().unwrap();
        engine.set_timeout(Duration::from_millis(200));
        let started = Instant::now();
        assert!(engine.convert("function preview() { for(;;){} }", "t", None).is_err());
        assert!(started.elapsed() < Duration::from_secs(3), "無窮迴圈要被中止");
        // console.log 的輸出可以取回
        let mut engine = JsEngine::new().unwrap();
        engine.convert("function preview() { console.log('偵錯'); return 1; }", "t", None).unwrap();
        assert_eq!(engine.take_output(), ["偵錯"]);
    }

    #[test]
    fn reads_and_writes_buffer() {
        let mut buf = Buffer::from_text("你好世界");
        run("editor.insert(2, '，');", &mut buf).unwrap();
        assert_eq!(buf.text(), "你好，世界");
        run("editor.remove(2, 3);", &mut buf).unwrap();
        assert_eq!(buf.text(), "你好世界");
        run("editor.replace(0, 2, 'Hi');", &mut buf).unwrap();
        assert_eq!(buf.text(), "Hi世界");
    }

    #[test]
    fn query_functions() {
        let mut buf = Buffer::from_text("a\nbb\n");
        let out = run(
            "console.log(editor.length(), editor.lineCount(), JSON.stringify(editor.line(1)), editor.line(99), editor.text().length);",
            &mut buf,
        )
        .unwrap();
        assert_eq!(out, vec![r#"5 3 "bb\n" undefined 5"#]);
    }

    #[test]
    fn typical_macro_uppercase_all() {
        let mut buf = Buffer::from_text("hello\nworld\n");
        run(
            "const t = editor.text(); editor.replace(0, editor.length(), t.toUpperCase());",
            &mut buf,
        )
        .unwrap();
        assert_eq!(buf.text(), "HELLO\nWORLD\n");
    }

    #[test]
    fn indices_are_code_points_not_utf16() {
        let mut buf = Buffer::from_text("😀a");
        let out = run("console.log(editor.length(), editor.text().length);", &mut buf).unwrap();
        assert_eq!(out, vec!["2 3"]);
        run("editor.insert(1, 'X');", &mut buf).unwrap();
        assert_eq!(buf.text(), "😀Xa");
    }

    #[test]
    fn syntax_and_runtime_errors_are_reported() {
        let mut buf = Buffer::from_text("x");
        let e = run("let = ;", &mut buf).unwrap_err();
        assert!(!e.0.is_empty());
        let e = run("throw new Error('boom');", &mut buf).unwrap_err();
        assert!(e.0.contains("boom"), "{e}");
        assert_eq!(buf.text(), "x");
    }

    #[test]
    fn invalid_index_throws_range_error() {
        let mut buf = Buffer::from_text("x");
        let e = run("editor.insert(-1, 'a');", &mut buf).unwrap_err();
        assert!(e.0.contains("RangeError") || e.0.contains("非負整數"), "{e}");
        let e = run("editor.insert(1.5, 'a');", &mut buf).unwrap_err();
        assert!(e.0.contains("非負整數"), "{e}");
        assert_eq!(buf.text(), "x");
    }

    #[test]
    fn infinite_loop_is_interrupted() {
        let mut buf = Buffer::new();
        let mut engine = JsEngine::new().unwrap();
        engine.set_timeout(Duration::from_millis(200));
        let start = Instant::now();
        let r = engine.run("while (true) {}", &mut buf);
        assert!(r.is_err());
        assert!(start.elapsed() < Duration::from_secs(3));
        // 逾時後引擎仍可再用
        engine.set_timeout(Duration::from_secs(5));
        engine.run("editor.insert(0, 'ok');", &mut buf).unwrap();
        assert_eq!(buf.text(), "ok");
    }

    #[test]
    fn memory_bomb_is_stopped() {
        let mut buf = Buffer::new();
        let r = run(
            "const a = []; while (true) { a.push('x'.repeat(1 << 20)); }",
            &mut buf,
        );
        assert!(r.is_err());
    }

    #[test]
    fn globals_do_not_leak_between_runs() {
        let mut buf = Buffer::new();
        let mut engine = JsEngine::new().unwrap();
        engine.run("globalThis.secret = 42;", &mut buf).unwrap();
        engine
            .run("console.log(typeof globalThis.secret);", &mut buf)
            .unwrap();
        assert_eq!(engine.take_output(), vec!["undefined"]);
    }

    #[test]
    fn promise_jobs_never_touch_editor_after_run() {
        // 非同步任務若在 run 結束後才執行，editor 指標已清除，只能丟錯，不能碰到已釋放的緩衝區。
        // 這個測試主要確保不會崩潰；任務是否在 run 內執行都算合法。
        let mut buf = Buffer::from_text("x");
        let mut engine = JsEngine::new().unwrap();
        engine
            .run(
                "Promise.resolve().then(() => editor.insert(0, 'p'));",
                &mut buf,
            )
            .unwrap();
        assert!(buf.text() == "x" || buf.text() == "px", "{}", buf.text());
        drop(buf);
        // 再跑一次，確認引擎在 buffer 已釋放後仍然健康
        let mut buf2 = Buffer::new();
        engine.run("editor.insert(0, 'ok');", &mut buf2).unwrap();
        assert_eq!(buf2.text(), "ok");
    }

    #[test]
    fn new_query_functions_work_with_the_default_host() {
        let mut buf = Buffer::from_text("ab\ncd\n");
        let out = run(
            "console.log(editor.slice(1, 4).length, editor.lineStart(1), editor.lineOf(4), JSON.stringify(editor.selection()), editor.selectedText().length, editor.path());",
            &mut buf,
        )
        .unwrap();
        assert_eq!(out, vec!["3 3 1 [0,0] 0 undefined"]);
    }

    #[test]
    fn find_returns_positions_and_honours_options() {
        let mut buf = Buffer::from_text("你好 Foo foo");
        let out = run(
            "console.log(JSON.stringify(editor.find('foo')), JSON.stringify(editor.find('foo', 4)), JSON.stringify(editor.find('foo', 0, {caseSensitive: true})), editor.find('zzz'), JSON.stringify(editor.find('o+', 99, {regex: true, backward: true})));",
            &mut buf,
        )
        .unwrap();
        assert_eq!(out, vec!["[3,6] [7,10] [7,10] undefined [8,10]"]);
        let e = run("editor.find('(', 0, {regex: true});", &mut buf).unwrap_err();
        assert!(!e.0.is_empty());
    }

    #[test]
    fn unsupported_run_and_dialogs_have_safe_defaults() {
        let mut buf = Buffer::new();
        let e = run("editor.run('selectAll');", &mut buf).unwrap_err();
        assert!(e.0.contains("selectAll"), "{e}");
        let out = run(
            "editor.alert('hi'); console.log(editor.confirm('?'));",
            &mut buf,
        )
        .unwrap();
        assert_eq!(out, vec!["false"]);
    }

    #[test]
    fn type_without_a_ui_host_appends() {
        let mut buf = Buffer::from_text("ab");
        run("editor.type('c'); editor.paste('d');", &mut buf).unwrap();
        assert_eq!(buf.text(), "abcd");
    }
}
