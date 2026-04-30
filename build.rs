fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("packaging/windows/AppIcon.ico");
        if let Err(e) = res.compile() {
            eprintln!("warning: failed to embed Windows icon: {e}");
        }
    }
}
