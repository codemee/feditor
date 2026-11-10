//! 修正輸入法造成的「突然變大寫」：注音輸入法（英文模式）快速打字時，有些字母不走輸入法的「確認」事件，
//! 而是變成按鍵事件加上大寫的文字事件——Shift 沒按、Caps Lock 也沒開，文字卻是大寫（按鍵紀錄：
//! `Key { key: U, modifiers: NONE }` 後面跟著 `Text("U")`）。
//!
//! 在按鍵事件交給編輯區與終端機之前先處理：同一個字母鍵、沒有任何修飾鍵、Caps Lock 沒開，卻得到大寫字母，就改回小寫。

use eframe::egui::{Event, Key};

/// 字母鍵對應的小寫字元。
fn letter_of(key: Key) -> Option<char> {
    let name = key.name();
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_uppercase() => Some(c.to_ascii_lowercase()),
        _ => None,
    }
}

/// 輸入法最近一次用「確認」事件送來單一英文字母的時間。
/// 有人用 Caps Lock 切換輸入法的中英文：英文模式下 Caps Lock 是開著的，但那不是要打大寫，
/// 輸入法確認的字母都是小寫。看到這種字母，就知道這時候 Caps Lock 是輸入法的開關。
static LAST_IME_LETTER: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// 這段時間內有過輸入法的英文字母，就當作 Caps Lock 是輸入法的開關。
const IME_LETTER_MEMORY: std::time::Duration = std::time::Duration::from_secs(10);

/// 把 `events` 裡不該是大寫的單一字母文字改成小寫；`caps_lock` 是 Caps Lock 現在有沒有開。回傳改了幾個。
/// Caps Lock 開著時，只有「剛剛輸入法送過英文字母」才改（Caps Lock 是輸入法開關）；真的用 Caps Lock 打大寫的人不受影響。
pub fn fix(events: &mut [Event], caps_lock: bool) -> usize {
    fix_at(events, caps_lock, std::time::Instant::now())
}

fn fix_at(events: &mut [Event], caps_lock: bool, now: std::time::Instant) -> usize {
    let mut last = LAST_IME_LETTER.lock().unwrap_or_else(|e| e.into_inner());
    // 最近一個按下的字母鍵（沒有修飾鍵時才算）；文字事件緊跟在它後面
    let mut plain_letter: Option<char> = None;
    let mut fixed = 0;
    for event in events.iter_mut() {
        match event {
            Event::Ime(eframe::egui::ImeEvent::Commit(text)) => {
                let mut chars = text.chars();
                if matches!((chars.next(), chars.next()), (Some(c), None) if c.is_ascii_alphabetic()) {
                    *last = Some(now);
                }
            }
            Event::Text(_) if caps_lock && !last.is_some_and(|t| now.duration_since(t) <= IME_LETTER_MEMORY) => {
                plain_letter = None;
            }
            Event::Key { key, pressed: true, modifiers, .. } => {
                plain_letter = if modifiers.is_none() { letter_of(*key) } else { None };
            }
            Event::Text(text) => {
                let mut chars = text.chars();
                if let (Some(c), None, Some(expected)) = (chars.next(), chars.next(), plain_letter.take()) {
                    if c.is_ascii_uppercase() && c.to_ascii_lowercase() == expected {
                        *text = expected.to_string();
                        fixed += 1;
                    }
                }
            }
            _ => {}
        }
    }
    fixed
}

/// Caps Lock 現在有沒有開（只有 Windows 查得到；其他系統當作沒開，那裡沒有這個問題）。
pub fn caps_lock_on() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CAPITAL};
        // SAFETY: 單純查詢按鍵狀態，沒有任何前置條件。低位元是「切換」狀態。
        unsafe { GetKeyState(VK_CAPITAL as i32) & 1 != 0 }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Modifiers;

    fn key(k: Key, modifiers: Modifiers) -> Event {
        Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers }
    }

    /// 測試共用一個全域的記憶：每個測試開頭清掉，並且不要同時跑。
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn reset() {
        *LAST_IME_LETTER.lock().unwrap() = None;
    }

    /// 用 Caps Lock 切換輸入法的人：Caps Lock 開著，輸入法確認了幾個小寫字母，後面漏網的大寫字母要改回小寫。
    #[test]
    fn when_caps_lock_is_the_ime_switch_stray_capitals_still_go_back_to_lowercase() {
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        let mut events = vec![
            Event::Ime(eframe::egui::ImeEvent::Commit("c".into())),
            Event::Ime(eframe::egui::ImeEvent::Commit("l".into())),
            key(Key::U, Modifiers::NONE),
            Event::Text("U".into()),
            key(Key::D, Modifiers::SHIFT),
            Event::Text("D".into()),
        ];
        assert_eq!(fix(&mut events, true), 1);
        assert!(matches!(&events[3], Event::Text(t) if t == "u"));
        assert!(matches!(&events[5], Event::Text(t) if t == "D"), "按著 Shift 的還是大寫");
        // 過了一陣子輸入法都沒再送字母：Caps Lock 又當作真的大寫鎖定
        let later = std::time::Instant::now() + IME_LETTER_MEMORY + std::time::Duration::from_secs(1);
        let mut events = vec![key(Key::U, Modifiers::NONE), Event::Text("U".into())];
        assert_eq!(fix_at(&mut events, true, later), 0);
    }

    #[test]
    fn an_uppercase_letter_without_shift_or_caps_lock_goes_back_to_lowercase() {
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        let mut events = vec![key(Key::U, Modifiers::NONE), Event::Text("U".into()), key(Key::D, Modifiers::NONE), Event::Text("D".into())];
        assert_eq!(fix(&mut events, false), 2);
        assert!(matches!(&events[1], Event::Text(t) if t == "u"));
        assert!(matches!(&events[3], Event::Text(t) if t == "d"));
    }

    #[test]
    fn real_capitals_are_left_alone() {
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        // 按著 Shift
        let mut shifted = vec![key(Key::U, Modifiers::SHIFT), Event::Text("U".into())];
        assert_eq!(fix(&mut shifted, false), 0);
        // Caps Lock 開著、而且輸入法沒送過英文字母：是真的在打大寫
        reset();
        let mut caps = vec![key(Key::U, Modifiers::NONE), Event::Text("U".into())];
        assert_eq!(fix(&mut caps, true), 0);
        assert!(matches!(&caps[1], Event::Text(t) if t == "U"));
        // 文字和按的鍵對不上（例如輸入法送來的別的字）、多個字、本來就小寫、沒有按鍵事件
        for events in [
            vec![key(Key::U, Modifiers::NONE), Event::Text("X".into())],
            vec![key(Key::U, Modifiers::NONE), Event::Text("UU".into())],
            vec![key(Key::U, Modifiers::NONE), Event::Text("u".into())],
            vec![Event::Text("U".into())],
            vec![key(Key::Enter, Modifiers::NONE), Event::Text("U".into())],
        ] {
            let mut events = events;
            assert_eq!(fix(&mut events, false), 0, "{events:?}");
        }
    }
}
