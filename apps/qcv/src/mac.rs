//! A native menu bar (objc2): qcv › Toggle Full Screen · Quit. The responder chain does the work.
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
use objc2_foundation::NSString;
pub fn menu_bar() {
    let mtm = MainThreadMarker::new().unwrap();
    let app = NSApplication::sharedApplication(mtm);
    let bar = NSMenu::new(mtm);
    let app_item = NSMenuItem::new(mtm);
    let app_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("qcv"));
    unsafe {
        let fs = NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str("Toggle Full Screen"), Some(objc2::sel!(toggleFullScreen:)), &NSString::from_str("f"));
        let quit = NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str("Quit qcv"), Some(objc2::sel!(terminate:)), &NSString::from_str("q"));
        app_menu.addItem(&fs); app_menu.addItem(&NSMenuItem::separatorItem(mtm)); app_menu.addItem(&quit);
    }
    app_item.setSubmenu(Some(&app_menu));
    bar.addItem(&app_item);
    app.setMainMenu(Some(&bar));
}
/// Bring the process to the front: a plain executable (no bundle) is not activated by launch, and wgpu-hal skips
/// drawable acquisition while the NSWindow's occlusion state says "not visible" (its workaround for a vsync hang).
pub fn activate() {
    let mtm = MainThreadMarker::new().unwrap();
    let app = NSApplication::sharedApplication(mtm);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
}
