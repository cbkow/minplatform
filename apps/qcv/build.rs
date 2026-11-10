//! Windows: the exe's resources (canon row 248) — the icon (`assets/icons/qcview.ico`, id 1: Explorer, the taskbar, the
//! window's big icon), the version details from the package version (QCView 2.x's exe carried none) and QCView's
//! application manifest (`packaging/windows/qcview.manifest`: long paths). Nothing on the other platforms.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/icons/qcview.ico");
        println!("cargo:rerun-if-changed=../../packaging/windows/qcview.manifest");
        let mut res = winresource::WindowsResource::new();
        res.set_icon_with_id("assets/icons/qcview.ico", "1");
        res.set_manifest_file("../../packaging/windows/qcview.manifest");
        res.set("ProductName", "QCView");
        res.set("FileDescription", "QCView");
        res.set("CompanyName", "QCView");
        res.set("LegalCopyright", "GPL-3.0-or-later");
        res.set("OriginalFilename", "qcview.exe");
        res.set("InternalName", "qcview");
        res.compile().expect("the exe's resources (rc.exe from the Windows SDK)");
    }
}
