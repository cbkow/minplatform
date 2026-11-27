//! A native menu bar (objc2): qcv › Toggle Full Screen · Quit. The responder chain does the work.
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
use objc2_foundation::NSString;
pub fn menu_bar() {
    let mtm = MainThreadMarker::new().unwrap();
    let app = NSApplication::sharedApplication(mtm);
    let bar = NSMenu::new(mtm);
    let app_item = NSMenuItem::new(mtm);
    let app_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("QCView"));
    unsafe {
        let fs = NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str("Toggle Full Screen"), Some(objc2::sel!(toggleFullScreen:)), &NSString::from_str("f"));
        let quit = NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str("Quit QCView"), Some(objc2::sel!(terminate:)), &NSString::from_str("q"));
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
/// Compact mode's window (canon row 617): the system's own titled window with its title bar made part of the picture - the
/// content under a transparent bar, no title, no buttons - so the window keeps the system's round corners, edge line,
/// shadow and resizing, which a window without decorations loses. `on` false gives the title bar back. Changes only
/// what differs, so it can be asked again after the system has had the window (leaving full screen).
pub fn compact_chrome(window: &dyn winit31::window::Window, on: bool) {
    use objc2::{msg_send, runtime::AnyObject};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(h) = window.window_handle() else { return };
    let RawWindowHandle::AppKit(h) = h.as_raw() else { return };
    const FULL_SIZE_CONTENT_VIEW: usize = 1 << 15;
    unsafe {
        let view = h.ns_view.as_ptr() as *mut AnyObject;
        let win: *mut AnyObject = msg_send![view, window]; if win.is_null() { return }
        let mask: usize = msg_send![win, styleMask];
        let want = if on { mask | FULL_SIZE_CONTENT_VIEW } else { mask & !FULL_SIZE_CONTENT_VIEW };
        if want != mask { let _: () = msg_send![win, setStyleMask: want]; }
        let clear: bool = msg_send![win, titlebarAppearsTransparent];
        if clear != on { let _: () = msg_send![win, setTitlebarAppearsTransparent: on]; let _: () = msg_send![win, setTitleVisibility: if on { 1isize } else { 0isize }]; }
        for b in 0..3usize { let btn: *mut AnyObject = msg_send![win, standardWindowButton: b]; if !btn.is_null() { let hidden: bool = msg_send![btn, isHidden]; if hidden != on { let _: () = msg_send![btn, setHidden: on]; } } }
    }
}
