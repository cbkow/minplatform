//! Windows: the exe's resources (as QCView's, canon row 248; minNotes row 372) - the icon (`assets/icons/minnotes.ico`,
//! id 1: Explorer, the taskbar, the window's big icon), the version details from the package version and the
//! application manifest (`packaging/windows/minnotes.manifest`: long paths). Nothing on the other platforms.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/icons/minnotes.ico");
        println!("cargo:rerun-if-changed=../../packaging/windows/minnotes.manifest");
        let mut res = winresource::WindowsResource::new();
        res.set_icon_with_id("assets/icons/minnotes.ico", "1");
        res.set_manifest_file("../../packaging/windows/minnotes.manifest");
        res.set("ProductName", "minNotes");
        res.set("FileDescription", "minNotes");
        res.set("CompanyName", "minNotes");
        res.set("LegalCopyright", "GPL-3.0-or-later");
        res.set("OriginalFilename", "minNotes.exe");
        res.set("InternalName", "minnotes");
        res.compile().expect("the exe's resources (rc.exe from the Windows SDK)");
    }
}
