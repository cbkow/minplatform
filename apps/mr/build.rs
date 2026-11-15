//! Windows: the exe's resources (as QCView's and minNotes', canon rows 248, 372) - the icon (`assets/icons/minrender.ico`,
//! id 1: Explorer, the taskbar, the window's big icon) and the version details from the package version. Nothing on
//! the other platforms.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/icons/minrender.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon_with_id("assets/icons/minrender.ico", "1");
        res.set("ProductName", "minRender");
        res.set("FileDescription", "minRender");
        res.set("CompanyName", "minRender");
        res.set("LegalCopyright", "GPL-3.0-or-later");
        res.set("OriginalFilename", "minRender.exe");
        res.set("InternalName", "minrender");
        res.compile().expect("the exe's resources (rc.exe from the Windows SDK)");
    }
}
