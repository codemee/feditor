//! 字型：向作業系統查詢等寬字型、列出可選的等寬字型家族、建立 egui 的字型設定，
//! 並讓拉丁字母與中文字落在同一條基準線上。

use eframe::egui;
use fontique::{
    Collection, CollectionOptions, FamilyInfo, FontInfo, FontStyle, GenericFamily, SourceCache,
    SourceCacheOptions,
};
use std::sync::Arc;

/// 載入好的字型檔。
pub struct LoadedFont {
    /// 字型檔內容。
    pub bytes: Vec<u8>,
    /// 在字型集合檔（.ttc）裡的索引；一般字型檔為 0。
    pub index: u32,
}

fn new_collection() -> (Collection, SourceCache) {
    (
        Collection::new(CollectionOptions {
            shared: false,
            system_fonts: true,
        }),
        SourceCache::new(SourceCacheOptions::default()),
    )
}

/// 同一個家族有很多字重與斜體，挑「非斜體、字重最接近一般（400）」的那一款。
fn regular_face(family: &FamilyInfo) -> Option<&FontInfo> {
    family
        .fonts()
        .iter()
        .filter(|f| f.style() == FontStyle::Normal)
        .min_by(|a, b| {
            (a.weight().value() - 400.0)
                .abs()
                .total_cmp(&(b.weight().value() - 400.0).abs())
        })
}

fn load_face(family: &FamilyInfo, cache: &mut SourceCache) -> Option<LoadedFont> {
    let face = regular_face(family)?;
    let blob = face.load(Some(cache))?;
    Some(LoadedFont {
        bytes: blob.as_ref().to_vec(),
        index: face.index(),
    })
}

/// 載入指定家族的「一般粗細」字型。`None` 表示作業系統預設的等寬字型。找不到時回傳 `None`。
///
/// 預設等寬字型是透過 `fontique` 使用平台自己的對應（Windows 的 DirectWrite、macOS 的
/// CoreText、Linux 的 fontconfig）查出來的，不寫死任何字型名稱。
pub fn load_family(name: Option<&str>) -> Option<LoadedFont> {
    let (mut collection, mut cache) = new_collection();
    match name {
        Some(name) => {
            let family = collection.family_by_name(name)?;
            load_face(&family, &mut cache)
        }
        None => {
            let ids: Vec<_> = collection.generic_families(GenericFamily::Monospace).collect();
            ids.into_iter().find_map(|id| {
                let family = collection.family(id)?;
                load_face(&family, &mut cache)
            })
        }
    }
}

/// 作業系統預設的等寬字型家族名稱。
pub fn default_monospace_name() -> Option<String> {
    let (mut collection, _) = new_collection();
    let id = collection
        .generic_families(GenericFamily::Monospace)
        .next()?;
    collection.family_name(id).map(str::to_owned)
}

/// 這個字型是不是等寬：比對幾個寬度差很多的字元（`i`、`l`、`m`、`W`、`.`、`0`）的字寬，
/// 全部一樣才算。不看字型檔裡的「等寬」旗標，因為有不少字型旗標標錯。
/// 缺少這些拉丁字元的字型（例如純中文字型）不算等寬，因為不適合當程式碼字型。
pub fn is_monospace(bytes: &[u8], index: u32) -> bool {
    use skrifa::{
        FontRef, MetadataProvider,
        instance::{LocationRef, Size},
    };
    let Ok(font) = FontRef::from_index(bytes, index) else {
        return false;
    };
    let charmap = font.charmap();
    let metrics = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    let mut widths = Vec::with_capacity(6);
    for ch in ['i', 'l', 'm', 'W', '.', '0'] {
        let Some(width) = charmap
            .map(ch)
            .and_then(|glyph| metrics.advance_width(glyph))
        else {
            return false;
        };
        widths.push(width);
    }
    widths.windows(2).all(|w| (w[0] - w[1]).abs() < 1.0)
}

/// 這個字型的拉丁字母能不能「正常地」畫出來？
///
/// 只比對字寬還不夠。有些老字型（細明體、MS Gothic 等）的拉丁字母是**內嵌的彩色點陣**，
/// egui 對這種字形不會依文字顏色上色，畫在淺色或深色底上都變成看不見，編輯器就像壞掉一樣。
/// 這種字型就算字寬一致也不能給使用者選。
///
/// 判斷方式不去猜字型檔的內部結構，而是用 egui 真正的排版與點陣化流程去試：
/// 排一個 `Fox`，每個字都要產生非空、而且不是「自帶顏色」的字形才算數。
/// 字級不影響結果（我實測過 9 到 32 點），所以只用 16 點檢查一次。
pub fn renders_latin(bytes: Vec<u8>, index: u32) -> bool {
    let mut fonts = egui::FontDefinitions::default();
    let mut data = egui::FontData::from_owned(bytes);
    data.index = index;
    fonts.font_data.insert("probe".into(), Arc::new(data));
    // 只放這一個字型、不放後備字型：缺字或畫不出來時不會被別的字型蓋過去
    fonts
        .families
        .insert(egui::FontFamily::Monospace, vec!["probe".into()]);

    let ctx = egui::Context::default();
    ctx.set_fonts(fonts);
    run_headless_pass(&ctx, |_| {}); // 字型設定下一個 pass 才生效
    let mut ok = false;
    run_headless_pass(&ctx, |ui| {
        let galley = ui.painter().layout_no_wrap(
            "Fox".to_owned(),
            egui::FontId::monospace(16.0),
            egui::Color32::WHITE,
        );
        ok = galley
            .rows
            .first()
            .is_some_and(|row| is_visible_text(&row.row.glyphs, 3));
    });
    ok
}

/// 這些字形是不是「看得見的一般文字」：數量對、每個都有非空的貼圖、而且不是自帶顏色的點陣。
fn is_visible_text(glyphs: &[egui::epaint::text::Glyph], expected: usize) -> bool {
    glyphs.len() == expected
        && glyphs
            .iter()
            .all(|g| !g.is_color && g.uv_rect.size.x > 0.0 && g.uv_rect.size.y > 0.0)
}

/// 列出系統上所有「等寬」字型家族的名稱（已排序、去重）。
///
/// 要逐一開啟字型檔檢查，家族很多時要花數百毫秒，所以應該在背景執行緒呼叫。
pub fn list_monospace_families() -> Vec<String> {
    let (mut collection, mut cache) = new_collection();
    let names: Vec<String> = collection.family_names().map(str::to_owned).collect();
    let mut result: Vec<String> = names
        .into_iter()
        // 以 @ 開頭的是直書用的字型變體，不是獨立的字型
        .filter(|name| !name.is_empty() && !name.starts_with('@'))
        .filter(|name| {
            collection
                .family_by_name(name)
                .and_then(|family| load_face(&family, &mut cache))
                .is_some_and(|font| {
                    is_monospace(&font.bytes, font.index) && renders_latin(font.bytes, font.index)
                })
        })
        .collect();
    result.sort_by_key(|name| name.to_lowercase());
    result.dedup();
    result
}

/// 在沒有視窗的 egui context 上跑一個 pass，丟棄輸出。
/// 只為了讓字型設定生效、取得排版結果。輸出裡有待上傳的貼圖，必須先清掉再丟棄，
/// 否則 debug 版本會在丟棄時 panic。
pub fn run_headless_pass(ctx: &egui::Context, run_ui: impl FnMut(&mut egui::Ui)) {
    let mut output = ctx.run_ui(egui::RawInput::default(), run_ui);
    output.textures_delta.clear();
}

/// 量出「拉丁字母」與「中文字」排在同一行時的垂直位置差，以字級的比例表示。
///
/// 為什麼要量：egui 排版時，同一行裡不同字型的字是「以行高置中」對齊的，不是對齊基準線。
/// 兩個字型的上緣與下緣比例差越多（例如 Consolas 與微軟正黑體），拉丁字母就會越偏離中文字，
/// 看起來上下參差。這裡用 egui 自己的排版結果去量，不去假設字型的內部度量。
fn latin_cjk_offset(fonts: egui::FontDefinitions, family: egui::FontFamily) -> f32 {
    // 排版座標會取整到像素，所以量測用很大的字級：取整誤差相對於字級就小到可以忽略，
    // 套用到各種字級時殘差才會在 1 點以內。
    const PROBE_SIZE: f32 = 96.0;
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts);
    // 字型設定要到下一個 pass 才會生效，所以先空跑一個
    run_headless_pass(&ctx, |_| {});
    let mut gap = 0.0;
    run_headless_pass(&ctx, |ui| {
        let galley = ui.painter().layout_no_wrap(
            "A第".to_owned(),
            egui::FontId::new(PROBE_SIZE, family.clone()),
            egui::Color32::WHITE,
        );
        if let Some(row) = galley.rows.first() {
            if let [latin, cjk, ..] = row.row.glyphs.as_slice() {
                gap = cjk.pos.y - latin.pos.y; // 中文字比拉丁字母低多少（點）
            }
        }
    });
    gap / PROBE_SIZE
}

/// 建立字型設定：編輯區用指定的等寬字型（`None` 或找不到時用系統預設），
/// 中文字形用微軟正黑體補，並讓兩者的基準線對齊。
pub fn build_fonts(family: Option<&str>) -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();

    // 中文字形：優先用微軟正黑體；找不到時 egui 會改用系統的字型後備機制
    let has_cjk = match cjk_font_bytes() {
        Some(bytes) => {
            fonts
                .font_data
                .insert("cjk".into(), Arc::new(egui::FontData::from_static(bytes)));
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(family).or_default().push("cjk".into());
            }
            true
        }
        None => false,
    };

    // 指定的字型放在等寬家族的最前面；指定的找不到就用系統預設的。
    // egui 內建的字型留在後面當後備。
    // 指定的字型若畫不出拉丁字母（例如設定檔裡是手動改的，或是舊版存下來的），也退回預設，
    // 不能讓編輯區變成一片空白。
    let chosen = family
        .and_then(|name| load_family(Some(name)))
        .filter(|font| renders_latin(font.bytes.clone(), font.index))
        .or_else(|| load_family(None));
    if let Some(font) = chosen {
        let mut data = egui::FontData::from_owned(font.bytes);
        data.index = font.index;
        fonts.font_data.insert("system-mono".into(), Arc::new(data.clone()));
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "system-mono".into());

        // 先量出拉丁字母與中文字的垂直落差，再把等寬字型往下補償，讓兩者基準線一致。
        // 只有真的載入了中文字型才量（否則量到的是缺字的替代符號，沒有意義）。
        if has_cjk {
            let gap = latin_cjk_offset(fonts.clone(), egui::FontFamily::Monospace);
            // 落差大得離譜表示量測有問題，寧可不補償
            if gap.abs() < 0.5 {
                data.tweak.y_offset_factor = gap;
                fonts.font_data.insert("system-mono".into(), Arc::new(data));
            }
        }
    }

    // 介面文字（標籤、下拉選單、按鈕）用的是比例字型，同樣有拉丁字母與中文字的基準線落差：
    // 不補償的話，中文字會比旁邊的英文數字、也比下拉選單的框低一截。
    // 這邊把中文字往上抬（拉丁字型不動），和等寬字型那邊（把拉丁字母往下補）方向相反：
    // 介面的拉丁字型本來就置中，偏的是中文。
    if has_cjk {
        lift_ui_cjk(&mut fonts);
    }
    fonts
}

/// 介面用的中文字型項目名稱（資料與 `"cjk"` 共用同一份，只有垂直補償不同）。
const UI_CJK: &str = "cjk-ui";

/// 量出介面（比例）字型的拉丁字母與中文字的基準線落差，把介面專用的中文字型往上抬同樣的量。
fn lift_ui_cjk(fonts: &mut egui::FontDefinitions) {
    let gap = latin_cjk_offset(fonts.clone(), egui::FontFamily::Proportional);
    // 落差大得離譜表示量測有問題，寧可不補償
    if gap.abs() >= 0.5 {
        return;
    }
    let Some(base) = fonts.font_data.get("cjk").cloned() else { return };
    let mut data = (*base).clone(); // 資料是靜態借用的，複製的只有設定
    data.tweak.y_offset_factor = -gap;
    fonts.font_data.insert(UI_CJK.into(), Arc::new(data));
    if let Some(names) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        for name in names.iter_mut().filter(|n| n.as_str() == "cjk") {
            *name = UI_CJK.into();
        }
    }
}

/// 微軟正黑體的檔案內容。整個程式只讀一次、不釋放，讓字型設定重建時不必重複複製這份大檔案。
fn cjk_font_bytes() -> Option<&'static [u8]> {
    static BYTES: std::sync::OnceLock<Option<&'static [u8]>> = std::sync::OnceLock::new();
    *BYTES.get_or_init(|| {
        std::fs::read(r"C:\Windows\Fonts\msjh.ttc")
            .ok()
            .map(|bytes| &*Box::leak(bytes.into_boxed_slice()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 家族名稱是否在清單裡（不分大小寫）。
    fn contains(list: &[String], name: &str) -> bool {
        list.iter().any(|n| n.eq_ignore_ascii_case(name))
    }

    #[test]
    fn default_monospace_is_found_and_valid() {
        let font = load_family(None).expect("這台機器應該有預設的等寬字型");
        let name = default_monospace_name().expect("預設等寬字型應該有家族名稱");
        eprintln!(
            "系統預設等寬字型：{name}（{} KB，索引 {}）",
            font.bytes.len() / 1024,
            font.index
        );
        assert!(!name.is_empty());
        assert!(font.bytes.len() > 10_000, "字型檔太小，不像有效的字型");
        // TrueType: 00 01 00 00、OpenType(CFF): "OTTO"、TrueType 集合: "ttcf"
        let magic = &font.bytes[..4];
        assert!(
            magic == [0, 1, 0, 0] || magic == b"OTTO" || magic == b"ttcf" || magic == b"true",
            "不認得的字型檔標頭：{magic:?}"
        );
        // 預設的等寬字型當然要通過等寬檢查
        assert!(is_monospace(&font.bytes, font.index));
    }

    #[cfg(windows)]
    #[test]
    fn monospace_check_accepts_mono_and_rejects_proportional() {
        let read = |file: &str| std::fs::read(format!(r"C:\Windows\Fonts\{file}")).ok();
        for (file, expect_mono) in [
            ("consola.ttf", true), // Consolas
            ("cour.ttf", true),    // Courier New
            ("arial.ttf", false),
            ("times.ttf", false),
            ("segoeui.ttf", false),
            ("calibri.ttf", false),
        ] {
            if let Some(bytes) = read(file) {
                assert_eq!(is_monospace(&bytes, 0), expect_mono, "{file}");
            }
        }
        // 垃圾資料不能讓檢查 panic
        assert!(!is_monospace(&[], 0));
        assert!(!is_monospace(b"not a font at all", 0));
    }

    /// 字型清單只能出現等寬字型：已知的比例字型一個都不能在裡面，已知的等寬字型要在裡面，
    /// 而且清單裡的每一個都要能重新載入並通過等寬檢查。
    #[cfg(windows)]
    #[test]
    fn family_list_contains_only_monospace_fonts() {
        let start = std::time::Instant::now();
        let list = list_monospace_families();
        eprintln!("等寬字型 {} 個，耗時 {:?}：{list:?}", list.len(), start.elapsed());

        assert!(!list.is_empty());
        assert!(contains(&list, "Consolas"));
        for proportional in [
            "Arial",
            "Times New Roman",
            "Segoe UI",
            "Calibri",
            "Microsoft JhengHei",
            "Verdana",
        ] {
            assert!(!contains(&list, proportional), "{proportional} 不是等寬字型");
        }
        // 排序、去重、沒有直書變體
        let mut sorted = list.clone();
        sorted.sort_by_key(|n| n.to_lowercase());
        assert_eq!(list, sorted);
        assert!(list.iter().all(|n| !n.is_empty() && !n.starts_with('@')));
        let unique: std::collections::BTreeSet<_> = list.iter().collect();
        assert_eq!(unique.len(), list.len());

        // 清單裡的每一個都要真的是等寬
        for name in &list {
            let font = load_family(Some(name)).unwrap_or_else(|| panic!("{name} 載不進來"));
            assert!(is_monospace(&font.bytes, font.index), "{name} 不是等寬字型");
        }
    }




    /// 能正常畫出拉丁字母的字型要通過；拉丁字母是內嵌彩色點陣的老字型要被擋下。
    #[cfg(windows)]
    #[test]
    fn renders_latin_accepts_normal_fonts_and_rejects_color_bitmap_fonts() {
        let check = |name: &str| load_family(Some(name)).map(|f| renders_latin(f.bytes, f.index));
        assert_eq!(check("Consolas"), Some(true));
        // 這兩個在這台機器上實測拉丁字形是自帶顏色的點陣（畫出來看不見）；沒安裝就略過
        for name in ["MingLiU", "MS Gothic"] {
            if let Some(result) = check(name) {
                assert!(!result, "{name} 的拉丁字母是彩色點陣，不該被當成可用的字型");
            }
        }
    }

    /// 端到端：清單裡的每一個字型，用真正的 `build_fonts`（含後備字型與補償）排出來，
    /// 拉丁字母都要是看得見的一般文字。這才是使用者實際會遇到的情況。
    /// 同時確認畫不出來的老字型沒有出現在清單裡。
    #[cfg(windows)]
    #[test]
    fn every_listed_family_draws_visible_text() {
        let list = list_monospace_families();
        for bad in ["MingLiU", "細明體", "MS Gothic", "ＭＳ ゴシック"] {
            assert!(!contains(&list, bad), "{bad} 畫不出拉丁字母，不該出現在清單裡");
        }
        for name in &list {
            let ctx = egui::Context::default();
            ctx.set_fonts(build_fonts(Some(name)));
            run_headless_pass(&ctx, |_| {});
            let mut visible = false;
            run_headless_pass(&ctx, |ui| {
                let galley = ui.painter().layout_no_wrap(
                    "Fox".to_owned(),
                    egui::FontId::monospace(20.0),
                    egui::Color32::WHITE,
                );
                visible = galley
                    .rows
                    .first()
                    .is_some_and(|row| is_visible_text(&row.row.glyphs, 3));
            });
            assert!(visible, "{name} 的拉丁字母畫不出來");
        }
        eprintln!("清單共 {} 個字型，拉丁字母都看得見", list.len());
    }

    /// 選了畫不出拉丁字母的字型（例如設定檔是手動改的）：`build_fonts` 要退回系統預設，
    /// 不能讓編輯區變成一片空白。
    #[cfg(windows)]
    #[test]
    fn unusable_font_choice_falls_back_to_default() {
        if load_family(Some("MingLiU")).is_none() {
            return;
        }
        let size_of =
            |f: &egui::FontDefinitions| f.font_data.get("system-mono").map(|d| d.bytes().len());
        assert_eq!(
            size_of(&build_fonts(Some("MingLiU"))),
            size_of(&build_fonts(None))
        );
    }
    #[test]
    fn loading_an_unknown_family_gives_none() {
        assert!(load_family(Some("這個字型不存在 NoSuchFont 12345")).is_none());
    }

    /// 指定的家族要真的被用在編輯區：換了家族，字型資料就跟著換。
    #[cfg(windows)]
    #[test]
    fn chosen_family_is_used_and_unknown_one_falls_back_to_default() {
        let size_of = |fonts: &egui::FontDefinitions| {
            fonts.font_data.get("system-mono").map(|d| d.bytes().len())
        };
        let default = build_fonts(None);
        let consolas = load_family(Some("Consolas")).map(|f| f.bytes.len());
        let courier = load_family(Some("Courier New")).map(|f| f.bytes.len());
        if let (Some(c), Some(k)) = (consolas, courier) {
            assert_eq!(size_of(&build_fonts(Some("Consolas"))), Some(c));
            assert_eq!(size_of(&build_fonts(Some("Courier New"))), Some(k));
            assert_ne!(c, k, "兩個字型的檔案不該一樣大，否則測不出有沒有換");
        }
        // 找不到的家族退回系統預設，不能讓編輯區沒有字型
        assert_eq!(size_of(&build_fonts(Some("NoSuchFont 12345"))), size_of(&default));
        assert!(size_of(&default).is_some());
    }

    /// 排一行「F第行」，回傳（大寫 F 的網格底邊, 拉丁字母的排版基準線, 中文字的排版基準線）。
    ///
    /// 補償（`y_offset_factor`）只作用在繪製出來的字形位置，不會反映在排版座標，
    /// 所以要從實際產生的網格頂點去讀（一個字形 4 個頂點，取最下緣）。
    #[cfg(windows)]
    fn layout_f_cjk(fonts: egui::FontDefinitions, size: f32) -> (f32, f32, f32) {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts);
        run_headless_pass(&ctx, |_| {}); // 字型下一個 pass 才生效
        let mut result = (f32::NAN, f32::NAN, f32::NAN);
        run_headless_pass(&ctx, |ui| {
            let galley = ui.painter().layout_no_wrap(
                "F第行".to_owned(),
                egui::FontId::monospace(size),
                egui::Color32::WHITE,
            );
            let row = &galley.rows[0].row;
            let first = row.glyphs[0].first_vertex as usize;
            let bottom = row.visuals.mesh.vertices[first..first + 4]
                .iter()
                .map(|v| v.pos.y)
                .fold(f32::MIN, f32::max);
            result = (bottom, row.glyphs[0].pos.y, row.glyphs[1].pos.y);
        });
        result
    }

    /// 把補償拿掉的字型設定（其他都一樣）。
    #[cfg(windows)]
    fn without_compensation(fonts: &egui::FontDefinitions) -> egui::FontDefinitions {
        let mut fonts = fonts.clone();
        if let Some(data) = fonts.font_data.get("system-mono").cloned() {
            let mut data = (*data).clone();
            data.tweak.y_offset_factor = 0.0;
            fonts.font_data.insert("system-mono".into(), Arc::new(data));
        }
        fonts
    }

    /// 拉丁字母與中文字的基準線殘差（點）：`拉丁基準線 + 補償實際移動量 − 中文基準線`。
    ///
    /// 補償的實際移動量，是同一個字型「有補償」與「沒補償」各排一次時，`F` 網格底邊的差。
    /// 這個量只看變化，不依賴 `F` 的底邊是否剛好落在基準線上，
    /// 所以對「用中文字型設計的拉丁字母」（細明體等）也成立。
    #[cfg(windows)]
    fn baseline_residual(fonts: &egui::FontDefinitions, size: f32) -> f32 {
        let (bottom_with, latin, cjk) = layout_f_cjk(fonts.clone(), size);
        let (bottom_without, _, _) = layout_f_cjk(without_compensation(fonts), size);
        let applied_shift = bottom_with - bottom_without;
        latin + applied_shift - cjk
    }

    #[cfg(windows)]
    fn have_cjk_font() -> bool {
        std::path::Path::new(r"C:\Windows\Fonts\msjh.ttc").exists()
    }

    /// 拉丁字母與中文字排在同一行時，基準線必須對齊（容許 1 點內的取整誤差）。
    /// 不只預設字型，每一個使用者可能選的等寬字型都要對齊，因為各字型的度量差很多。
    #[cfg(windows)]
    #[test]
    fn latin_and_cjk_share_a_baseline_for_every_monospace_font() {
        if !have_cjk_font() {
            return; // 沒有微軟正黑體的機器沒有這個問題可測
        }
        let list = list_monospace_families();
        let mut checked = 0;
        for name in std::iter::once(None).chain(list.iter().map(|n| Some(n.as_str()))) {
            let fonts = build_fonts(name);
            for size in [12.0_f32, 14.0, 20.0] {
                let residual = baseline_residual(&fonts, size);
                assert!(
                    residual.abs() <= 1.0,
                    "{}：字級 {size} 的基準線殘差有 {residual} 點",
                    name.unwrap_or("系統預設")
                );
            }
            checked += 1;
        }
        eprintln!("已檢查 {checked} 個等寬字型的基準線對齊");
        assert!(checked >= 2);
    }

    /// 介面（比例）字型排「F第」：回傳（拉丁字母的排版基準線, 中文字的排版基準線, 中文字網格的最下緣）。
    #[cfg(windows)]
    fn probe_proportional(fonts: egui::FontDefinitions, size: f32) -> (f32, f32, f32) {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts);
        run_headless_pass(&ctx, |_| {});
        let mut result = (f32::NAN, f32::NAN, f32::NAN);
        run_headless_pass(&ctx, |ui| {
            let galley = ui.painter().layout_no_wrap(
                "F第".to_owned(),
                egui::FontId::proportional(size),
                egui::Color32::WHITE,
            );
            let row = &galley.rows[0].row;
            let first = row.glyphs[1].first_vertex as usize;
            let bottom = row.visuals.mesh.vertices[first..first + 4]
                .iter()
                .map(|v| v.pos.y)
                .fold(f32::MIN, f32::max);
            result = (row.glyphs[0].pos.y, row.glyphs[1].pos.y, bottom);
        });
        result
    }

    /// 介面字型：拉丁字母基準線 − 中文字實際畫出來的基準線（中文字往上抬了多少要算進去）。
    #[cfg(windows)]
    fn ui_residual(fonts: &egui::FontDefinitions, size: f32, lifted: bool) -> f32 {
        let mut fonts = fonts.clone();
        if !lifted {
            let mut data = (*fonts.font_data[UI_CJK]).clone();
            data.tweak.y_offset_factor = 0.0;
            fonts.font_data.insert(UI_CJK.into(), Arc::new(data));
        }
        let (latin, cjk, bottom) = probe_proportional(fonts.clone(), size);
        // 沒抬的版本拿來當中文字原本的位置
        let mut plain = fonts;
        let mut data = (*plain.font_data[UI_CJK]).clone();
        data.tweak.y_offset_factor = 0.0;
        plain.font_data.insert(UI_CJK.into(), Arc::new(data));
        let (_, _, bottom_plain) = probe_proportional(plain, size);
        latin - (cjk + (bottom - bottom_plain))
    }

    /// 介面的中文字要和拉丁字母對齊基準線（下拉選單裡的中文才不會偏下），
    /// 而且是用「抬中文字」的方式補，等寬字型那邊（編輯區）不受影響。
    #[cfg(windows)]
    #[test]
    fn ui_chinese_text_is_lifted_to_the_latin_baseline_without_touching_the_editor_font() {
        if !have_cjk_font() {
            return;
        }
        let fonts = build_fonts(None);
        let lift = fonts.font_data[UI_CJK].tweak.y_offset_factor;
        assert!(lift < -0.02, "中文字應該被往上抬（量到 {lift}）");
        let proportional = &fonts.families[&egui::FontFamily::Proportional];
        assert!(proportional.iter().any(|n| n == UI_CJK) && !proportional.iter().any(|n| n == "cjk"));
        let mono = &fonts.families[&egui::FontFamily::Monospace];
        assert!(mono.iter().any(|n| n == "cjk"), "編輯區仍用原本的中文字型項目");
        assert_eq!(fonts.font_data["cjk"].tweak.y_offset_factor, 0.0);
        for size in [12.0_f32, 14.0, 20.0] {
            let with = ui_residual(&fonts, size, true);
            let without = ui_residual(&fonts, size, false);
            eprintln!("字級 {size}：抬之後殘差 {with}，沒抬 {without}");
            assert!(with.abs() <= 1.0, "字級 {size}：抬了之後基準線還差 {with} 點");
            assert!(without.abs() > with.abs() + 0.5, "字級 {size}：沒抬時落差應該更大（{without}）");
        }
    }

    /// 對照組：沒有補償時，基準線落差必須真的存在。
    /// 這證明上面的測試量得到問題，不是永遠會過。
    #[cfg(windows)]
    #[test]
    fn without_compensation_the_baselines_really_differ() {
        if !have_cjk_font() {
            return;
        }
        let fonts = build_fonts(Some("Consolas"));
        let compensation = fonts.font_data["system-mono"].tweak.y_offset_factor;
        assert!(compensation.abs() > 0.05, "build_fonts 應該已經量出並套用補償");
        let uncompensated = without_compensation(&fonts);
        let residual = baseline_residual(&uncompensated, 14.0);
        eprintln!("沒有補償時的基準線殘差：{residual}（補償比例 {compensation}）");
        assert!(residual.abs() >= 2.0, "沒有補償時落差應該明顯（量到 {residual}）");
    }
}
