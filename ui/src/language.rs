//! 依副檔名判斷檔案類型，決定行註解符號等語言相關的行為。

use std::path::Path;

/// 行註解符號；沒有行註解（或不認得的類型）時回傳空字串。
pub fn comment_prefix(path: Option<&Path>) -> &'static str {
    let ext = path
        .and_then(|p| p.extension())
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "rs" | "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "cs" | "java" | "js" | "mjs" | "cjs"
        | "jsx" | "ts" | "tsx" | "go" | "swift" | "kt" | "kts" | "scala" | "php" | "dart"
        | "css" | "scss" | "less" | "jsonc" | "proto" | "zig" | "v" | "glsl" | "slint" => "//",
        "py" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "psm1" | "rb" | "pl" | "pm" | "r" | "yml"
        | "yaml" | "toml" | "conf" | "ini" | "cfg" | "properties" | "mk" | "cmake" | "gitignore"
        | "dockerfile" | "tf" | "nim" | "cr" | "ex" | "exs" | "jl" => "#",
        "sql" | "lua" | "hs" | "elm" | "ada" => "--",
        "vim" => "\"",
        "bat" | "cmd" => "REM",
        "tex" | "m" | "erl" => "%",
        "lisp" | "el" | "clj" | "scm" | "asm" => ";",
        // 純文字、Markdown、HTML、JSON 等沒有行註解
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_comment_prefix_by_extension_case_insensitively() {
        assert_eq!(comment_prefix(Some(Path::new("a/main.RS"))), "//");
        assert_eq!(comment_prefix(Some(Path::new("x.py"))), "#");
        assert_eq!(comment_prefix(Some(Path::new("q.sql"))), "--");
        assert_eq!(comment_prefix(Some(Path::new("notes.txt"))), "");
        assert_eq!(comment_prefix(Some(Path::new("noext"))), "");
        assert_eq!(comment_prefix(None), "");
    }
}
