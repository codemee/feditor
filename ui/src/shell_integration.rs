//! 和檔案總管（Windows）、Finder（macOS）整合：在檔案與資料夾的右鍵選單加入 FEditor。
//!
//! - **Windows**：寫在 `HKEY_CURRENT_USER\Software\Classes`（不需要系統管理員權限）。
//!   檔案多一個「用 FEditor 開啟」，資料夾（以及資料夾的空白處）多一個「用 FEditor 選取檔案開啟…」。
//!   Windows 11 的新版右鍵選單會把它收在「顯示其他選項」裡。
//! - **macOS**：Finder 沒有直接加項目的方式，所以在 `~/Library/Services` 產生兩個「快速動作」
//!   （檔案：開啟；資料夾：選取檔案開啟），出現在右鍵的「快速動作」子選單。
//! - 其他系統不支援（[`supported`] 回傳 `false`，設定面板就不顯示開關）。
//!
//! 兩種項目執行的都是同一件事：用 `feditor <路徑…>` 啟動。檔案直接開；資料夾會顯示「開啟檔案」交談窗，
//! 從那個資料夾開始讓使用者選（見 `App::open_dropped`）。

// 兩個平台的內容產生邏輯在每個平台都編譯（為了測試），另一個平台用不到的部分不算多餘
#![allow(dead_code)]

use std::path::Path;

/// 右鍵選單裡兩個項目的文字（跟著介面語言，所以由呼叫端翻譯好再傳進來）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Labels {
    /// 檔案上的項目。
    pub open: String,
    /// 資料夾上的項目。
    pub pick: String,
}

/// 這個系統支不支援右鍵選單整合。
pub fn supported() -> bool {
    cfg!(any(windows, target_os = "macos"))
}

/// 加入（或更新成這個執行檔與這些文字的）右鍵選單。
pub fn enable(exe: &Path, labels: &Labels) -> Result<(), String> {
    imp::enable(exe, labels)
}

/// 移除右鍵選單。本來就沒有也算成功。
pub fn disable() -> Result<(), String> {
    imp::disable()
}

// ───────── Windows 登錄檔的內容（純文字邏輯，每個平台都能測） ─────────

/// 登錄檔裡的一個值。`key` 是 `Software\Classes` 底下的子機碼；`name` 空字串是預設值。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegValue {
    pub key: String,
    pub name: &'static str,
    pub data: String,
}

/// 我們在 `Software\Classes` 底下建立的三個機碼（移除時整棵刪掉）。
pub const REGISTRY_KEYS: [&str; 3] = [
    r"*\shell\FEditor",
    r"Directory\shell\FEditor",
    r"Directory\Background\shell\FEditor",
];

/// Windows 要寫進登錄檔的所有值。
pub fn registry_values(exe: &Path, labels: &Labels) -> Vec<RegValue> {
    let exe = exe.display().to_string();
    let icon = format!("\"{exe}\",0");
    // %1：右鍵的那個檔案或資料夾；%V：在資料夾空白處按右鍵時，那個資料夾
    let items = [
        (REGISTRY_KEYS[0], &labels.open, "%1"),
        (REGISTRY_KEYS[1], &labels.pick, "%1"),
        (REGISTRY_KEYS[2], &labels.pick, "%V"),
    ];
    let mut values = Vec::new();
    for (key, label, arg) in items {
        values.push(RegValue { key: key.to_owned(), name: "", data: label.clone() });
        values.push(RegValue { key: key.to_owned(), name: "Icon", data: icon.clone() });
        values.push(RegValue { key: format!(r"{key}\command"), name: "", data: format!("\"{exe}\" \"{arg}\"") });
    }
    values
}

// ───────── macOS 快速動作的內容（同樣是純文字邏輯） ─────────

/// 快速動作的資料夾名稱與要處理的檔案類型（`public.data` 是檔案，`public.folder` 是資料夾）。
pub const SERVICE_NAMES: [(&str, &str); 2] = [("FEditor-Open.workflow", "public.data"), ("FEditor-Pick.workflow", "public.folder")];

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// 包成 shell 的單引號字串（字串裡的單引號寫成 `'\''`）。
pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// 快速動作的 `Contents/Info.plist`：宣告它是 Finder 的服務，選單文字是 `label`。
pub fn service_info_plist(label: &str, file_type: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>NSServices</key>
	<array>
		<dict>
			<key>NSMenuItem</key>
			<dict>
				<key>default</key>
				<string>{label}</string>
			</dict>
			<key>NSMessage</key>
			<string>runWorkflowAsService</string>
			<key>NSRequiredContext</key>
			<dict>
				<key>NSApplicationIdentifier</key>
				<string>com.apple.finder</string>
			</dict>
			<key>NSSendFileTypes</key>
			<array>
				<string>{file_type}</string>
			</array>
		</dict>
	</array>
</dict>
</plist>
"#,
        label = xml_escape(label),
        file_type = xml_escape(file_type),
    )
}

/// 快速動作的 `Contents/document.wflow`：只有一個「執行 Shell 描述檔」，把選到的項目當參數交給 FEditor。
pub fn service_workflow(exe: &Path) -> String {
    // 放到背景、斷開輸出：快速動作結束時不會把 FEditor 一起帶走
    let script = format!("nohup {} \"$@\" >/dev/null 2>&1 &", shell_quote(&exe.display().to_string()));
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>AMApplicationBuild</key>
	<string>523</string>
	<key>AMApplicationVersion</key>
	<string>2.10</string>
	<key>AMDocumentVersion</key>
	<string>2</string>
	<key>actions</key>
	<array>
		<dict>
			<key>action</key>
			<dict>
				<key>AMAccepts</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Optional</key>
					<true/>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.string</string>
					</array>
				</dict>
				<key>AMActionVersion</key>
				<string>2.0.3</string>
				<key>AMApplication</key>
				<array>
					<string>Automator</string>
				</array>
				<key>AMParameterProperties</key>
				<dict>
					<key>COMMAND_STRING</key>
					<dict/>
					<key>CheckedForUserDefaultShell</key>
					<dict/>
					<key>inputMethod</key>
					<dict/>
					<key>shell</key>
					<dict/>
					<key>source</key>
					<dict/>
				</dict>
				<key>AMProvides</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.string</string>
					</array>
				</dict>
				<key>ActionBundlePath</key>
				<string>/System/Library/Automator/Run Shell Script.action</string>
				<key>ActionName</key>
				<string>Run Shell Script</string>
				<key>ActionParameters</key>
				<dict>
					<key>COMMAND_STRING</key>
					<string>{script}</string>
					<key>CheckedForUserDefaultShell</key>
					<true/>
					<key>inputMethod</key>
					<integer>1</integer>
					<key>shell</key>
					<string>/bin/zsh</string>
					<key>source</key>
					<string></string>
				</dict>
				<key>BundleIdentifier</key>
				<string>com.apple.RunShellScript</string>
				<key>CFBundleVersion</key>
				<string>2.0.3</string>
				<key>CanShowSelectedItemsWhenRun</key>
				<false/>
				<key>CanShowWhenRun</key>
				<true/>
				<key>Category</key>
				<array>
					<string>AMCategoryUtilities</string>
				</array>
				<key>Class Name</key>
				<string>RunShellScriptAction</string>
				<key>InputUUID</key>
				<string>6F1F0D3A-5B1E-4C0B-9B53-0E5A1F6C2A11</string>
				<key>Keywords</key>
				<array>
					<string>Shell</string>
					<string>Script</string>
				</array>
				<key>OutputUUID</key>
				<string>0C2B8A47-3D55-4E5C-8A2A-7A8F6B1D3E22</string>
				<key>UUID</key>
				<string>A4D7C1E9-2B63-4F0A-B8D1-5C9E3F7A4B33</string>
				<key>UnlocalizedApplications</key>
				<array>
					<string>Automator</string>
				</array>
				<key>arguments</key>
				<dict/>
				<key>isViewVisible</key>
				<integer>1</integer>
				<key>location</key>
				<string>309.000000:253.000000</string>
				<key>nibPath</key>
				<string>/System/Library/Automator/Run Shell Script.action/Contents/Resources/Base.lproj/main.nib</string>
			</dict>
			<key>isViewVisible</key>
			<integer>1</integer>
		</dict>
	</array>
	<key>connectors</key>
	<dict/>
	<key>workflowMetaData</key>
	<dict>
		<key>serviceInputTypeIdentifier</key>
		<string>com.apple.Automator.fileSystemObject</string>
		<key>serviceOutputTypeIdentifier</key>
		<string>com.apple.Automator.nothing</string>
		<key>serviceProcessesInput</key>
		<integer>0</integer>
		<key>workflowTypeIdentifier</key>
		<string>com.apple.Automator.servicesMenu</string>
	</dict>
</dict>
</plist>
"#,
        script = xml_escape(&script),
    )
}

/// 在 `dir`（通常是 `~/Library/Services`）寫入兩個快速動作。
pub fn write_services(dir: &Path, exe: &Path, labels: &Labels) -> std::io::Result<()> {
    for (name, file_type) in SERVICE_NAMES {
        let label = if file_type == "public.folder" { &labels.pick } else { &labels.open };
        let contents = dir.join(name).join("Contents");
        std::fs::create_dir_all(&contents)?;
        std::fs::write(contents.join("Info.plist"), service_info_plist(label, file_type))?;
        std::fs::write(contents.join("document.wflow"), service_workflow(exe))?;
    }
    Ok(())
}

/// 從 `dir` 移除我們的快速動作（沒有就略過）。
pub fn remove_services(dir: &Path) -> std::io::Result<()> {
    for (name, _) in SERVICE_NAMES {
        match std::fs::remove_dir_all(dir.join(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

// ───────── 各平台的實作 ─────────

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW,
        RegDeleteTreeW, RegGetValueW, RegSetValueExW,
    };

    pub const CLASSES: &str = r"Software\Classes";

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn check(code: u32, what: &str) -> Result<(), String> {
        if code == ERROR_SUCCESS { Ok(()) } else { Err(format!("{what}（錯誤碼 {code}）")) }
    }

    /// 把 `values` 寫到 `HKCU\<classes>` 底下。
    pub fn write(classes: &str, values: &[RegValue]) -> Result<(), String> {
        for v in values {
            let path = wide(&format!(r"{classes}\{}", v.key));
            let mut key: HKEY = null_mut();
            // SAFETY: 參數都是以 0 結尾的 UTF-16 字串或有效的輸出指標
            let created = unsafe {
                RegCreateKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, null(), REG_OPTION_NON_VOLATILE, KEY_WRITE, null(), &mut key, null_mut())
            };
            check(created, "無法建立登錄機碼")?;
            let name = wide(v.name);
            let data = wide(&v.data);
            // SAFETY: key 剛建立成功；資料長度以位元組計，含結尾的 0
            let set = unsafe { RegSetValueExW(key, name.as_ptr(), 0, REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32) };
            // SAFETY: 關閉剛開啟的 key
            unsafe { RegCloseKey(key) };
            check(set, "無法寫入登錄值")?;
        }
        Ok(())
    }

    /// 刪掉 `HKCU\<classes>\<key>` 整棵；本來就沒有算成功。
    pub fn delete(classes: &str, keys: &[&str]) -> Result<(), String> {
        for key in keys {
            let path = wide(&format!(r"{classes}\{key}"));
            // SAFETY: 以 0 結尾的 UTF-16 字串
            let code = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, path.as_ptr()) };
            if code != ERROR_FILE_NOT_FOUND {
                check(code, "無法刪除登錄機碼")?;
            }
        }
        Ok(())
    }

    /// 讀 `HKCU\<classes>\<key>` 的字串值；沒有就 `None`。
    pub fn read(classes: &str, key: &str, name: &str) -> Option<String> {
        let path = wide(&format!(r"{classes}\{key}"));
        let name = wide(name);
        let mut buf = vec![0u16; 2048];
        let mut bytes = (buf.len() * 2) as u32;
        // SAFETY: buf 夠大，bytes 是它的位元組數；函式會回報實際長度
        let code = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, path.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, null_mut(), buf.as_mut_ptr().cast(), &mut bytes)
        };
        if code != ERROR_SUCCESS {
            return None;
        }
        let len = (bytes as usize / 2).saturating_sub(1).min(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }

    pub fn enable(exe: &Path, labels: &Labels) -> Result<(), String> {
        write(CLASSES, &registry_values(exe, labels))
    }

    pub fn disable() -> Result<(), String> {
        delete(CLASSES, &REGISTRY_KEYS)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use std::path::PathBuf;

    fn services_dir() -> Result<PathBuf, String> {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library/Services"))
            .ok_or_else(|| "找不到使用者的家目錄".to_owned())
    }

    /// 請系統重新讀取服務清單（不然要登出再登入才看得到）；失敗也沒關係。
    fn refresh() {
        let _ = std::process::Command::new("/System/Library/CoreServices/pbs").arg("-update").status();
    }

    pub fn enable(exe: &Path, labels: &Labels) -> Result<(), String> {
        write_services(&services_dir()?, exe, labels).map_err(|e| e.to_string())?;
        refresh();
        Ok(())
    }

    pub fn disable() -> Result<(), String> {
        remove_services(&services_dir()?).map_err(|e| e.to_string())?;
        refresh();
        Ok(())
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use super::*;

    pub fn enable(_: &Path, _: &Labels) -> Result<(), String> {
        Err("這個系統不支援".to_owned())
    }

    pub fn disable() -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels() -> Labels {
        Labels { open: "用 FEditor 開啟".to_owned(), pick: "用 FEditor 選取檔案開啟…".to_owned() }
    }

    #[test]
    fn registry_values_cover_files_folders_and_folder_backgrounds() {
        let values = registry_values(Path::new(r"C:\Apps\FEditor\feditor.exe"), &labels());
        let get = |key: &str, name: &str| values.iter().find(|v| v.key == key && v.name == name).map(|v| v.data.clone());
        // 檔案：開啟
        assert_eq!(get(r"*\shell\FEditor", "").as_deref(), Some("用 FEditor 開啟"));
        assert_eq!(get(r"*\shell\FEditor\command", "").as_deref(), Some(r#""C:\Apps\FEditor\feditor.exe" "%1""#));
        // 資料夾：選取檔案開啟；資料夾空白處用 %V
        assert_eq!(get(r"Directory\shell\FEditor", "").as_deref(), Some("用 FEditor 選取檔案開啟…"));
        assert_eq!(get(r"Directory\shell\FEditor\command", "").as_deref(), Some(r#""C:\Apps\FEditor\feditor.exe" "%1""#));
        assert_eq!(get(r"Directory\Background\shell\FEditor\command", "").as_deref(), Some(r#""C:\Apps\FEditor\feditor.exe" "%V""#));
        // 每個項目都有圖示；全部都在我們自己的三個機碼底下（移除時才刪得乾淨）
        assert_eq!(get(r"*\shell\FEditor", "Icon").as_deref(), Some(r#""C:\Apps\FEditor\feditor.exe",0"#));
        assert_eq!(values.len(), 9);
        assert!(values.iter().all(|v| REGISTRY_KEYS.iter().any(|k| v.key == *k || v.key.starts_with(&format!(r"{k}\")))));
    }

    #[test]
    fn shell_quote_and_xml_escape_keep_special_characters_safe() {
        assert_eq!(shell_quote("/Applications/FEditor.app/Contents/MacOS/feditor"), "'/Applications/FEditor.app/Contents/MacOS/feditor'");
        assert_eq!(shell_quote("/a b/it's"), r"'/a b/it'\''s'");
        assert_eq!(xml_escape(r#"a&b<c>"d""#), "a&amp;b&lt;c&gt;&quot;d&quot;");
        let wflow = service_workflow(Path::new("/Users/me/My Apps/it's & <ok>/feditor"));
        assert!(wflow.contains("nohup '/Users/me/My Apps/it'\\''s &amp; &lt;ok&gt;/feditor' &quot;$@&quot; &gt;/dev/null 2&gt;&amp;1 &amp;"), "{wflow}");
        let info = service_info_plist("用 FEditor 選取檔案開啟…", "public.folder");
        assert!(info.contains("<string>用 FEditor 選取檔案開啟…</string>") && info.contains("<string>public.folder</string>"));
        assert!(info.contains("com.apple.finder") && info.contains("runWorkflowAsService"));
    }

    #[test]
    fn services_are_written_updated_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let services = dir.path().join("Library/Services");
        write_services(&services, Path::new("/x/feditor"), &labels()).unwrap();
        let info = |name: &str| std::fs::read_to_string(services.join(name).join("Contents/Info.plist")).unwrap();
        assert!(info("FEditor-Open.workflow").contains("用 FEditor 開啟") && info("FEditor-Open.workflow").contains("public.data"));
        assert!(info("FEditor-Pick.workflow").contains("用 FEditor 選取檔案開啟…") && info("FEditor-Pick.workflow").contains("public.folder"));
        assert!(services.join("FEditor-Pick.workflow/Contents/document.wflow").is_file());
        // 換語言、換位置：重寫一次就更新
        let english = Labels { open: "Open with FEditor".to_owned(), pick: "Pick files with FEditor...".to_owned() };
        write_services(&services, Path::new("/y/feditor"), &english).unwrap();
        assert!(info("FEditor-Open.workflow").contains("Open with FEditor"));
        assert!(std::fs::read_to_string(services.join("FEditor-Open.workflow/Contents/document.wflow")).unwrap().contains("/y/feditor"));
        // 別人的快速動作不會被動到
        std::fs::create_dir_all(services.join("Other.workflow")).unwrap();
        remove_services(&services).unwrap();
        assert!(!services.join("FEditor-Open.workflow").exists() && !services.join("FEditor-Pick.workflow").exists());
        assert!(services.join("Other.workflow").exists());
        remove_services(&services).unwrap(); // 沒有也算成功
    }

    #[cfg(windows)]
    #[test]
    fn windows_registry_round_trip_in_a_private_key() {
        // 寫在自己的測試機碼底下，不碰真正的 Software\Classes
        let classes = format!(r"Software\feditor-test-{}\Classes", std::process::id());
        let values = registry_values(Path::new(r"C:\Apps\feditor.exe"), &labels());
        imp::write(&classes, &values).unwrap();
        assert_eq!(imp::read(&classes, r"*\shell\FEditor", "").as_deref(), Some("用 FEditor 開啟"));
        assert_eq!(imp::read(&classes, r"Directory\Background\shell\FEditor\command", "").as_deref(), Some(r#""C:\Apps\feditor.exe" "%V""#));
        imp::write(&classes, &registry_values(Path::new(r"D:\feditor.exe"), &labels())).unwrap(); // 更新
        assert_eq!(imp::read(&classes, r"*\shell\FEditor\command", "").as_deref(), Some(r#""D:\feditor.exe" "%1""#));
        imp::delete(&classes, &REGISTRY_KEYS).unwrap();
        assert_eq!(imp::read(&classes, r"*\shell\FEditor", ""), None);
        imp::delete(&classes, &REGISTRY_KEYS).unwrap(); // 沒有也算成功
        imp::delete("Software", &[&format!("feditor-test-{}", std::process::id())]).unwrap(); // 連測試用的機碼一起清掉
    }
}
