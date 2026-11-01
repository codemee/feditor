// 在 Windows 上把應用程式圖示內嵌進 exe，檔案總管與工作列才會顯示它。
// 圖示由 assets/make_icon.py 產生。
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "feditor");
        res.set("FileDescription", "feditor");
        res.compile().expect("內嵌 Windows 資源（圖示）失敗");
    }
}
