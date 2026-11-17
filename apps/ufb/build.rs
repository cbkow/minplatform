//! Windows: the exe's resources (as QCView's, minNotes' and minRender's, canon rows 248, 372) - the icon
//! (`assets/icons/ufb.ico`, 1.4.2's own, id 1: Explorer, the taskbar, Alt+Tab) and the version details from the
//! package version. Nothing on the other platforms.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=UFB_IDENTITY");
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/icons/ufb.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon_with_id("assets/icons/ufb.ico", "1");
        res.set("ProductName", "UFB");
        res.set("FileDescription", "UFB");
        res.set("CompanyName", "UFB");
        res.set("LegalCopyright", "GPL-3.0-or-later");
        res.set("OriginalFilename", "UFB.exe");
        res.set("InternalName", "ufb");
        res.compile().expect("the exe's resources (rc.exe from the Windows SDK)");
    }
}
