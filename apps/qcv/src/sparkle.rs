//! macOS: the Sparkle updater (canon row 248) — QCView's `sparkle_updater_macos.mm`, call for call: one
//! `SPUStandardUpdaterController` started with the app (it reads SUFeedURL / SUPublicEDKey / the check interval from
//! Info.plist and schedules the daily check), About ▸ Check for Updates…, and the Settings switch bound to Sparkle's own
//! `automaticallyChecksForUpdates` (kept by Sparkle in the user defaults, not in our settings store).
//! Sparkle.framework is LOADED at run time from the bundle's Frameworks, not linked: a development run outside the bundle
//! simply has no updater ([`available`] is false; the menu item and the switch are inert).
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::{msg_send, MainThreadMarker};
use objc2_foundation::{NSBundle, NSString};
use std::cell::RefCell;

thread_local! { static CONTROLLER: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) }; }

/// Start the updater once (main thread). True when it runs.
pub fn start() -> bool {
    if CONTROLLER.with_borrow(|c| c.is_some()) { return true }
    let Some(_mtm) = MainThreadMarker::new() else { return false };
    let Some(dir) = NSBundle::mainBundle().privateFrameworksPath() else { return false };
    let path = NSString::from_str(&format!("{dir}/Sparkle.framework"));
    let Some(fw) = NSBundle::bundleWithPath(&path) else { log::info!("Sparkle: no framework in this bundle — no updater"); return false };
    if !unsafe { fw.load() } { log::warn!("Sparkle: the framework did not load"); return false }
    let Some(cls) = AnyClass::get(c"SPUStandardUpdaterController") else { log::warn!("Sparkle: no SPUStandardUpdaterController"); return false };
    let none: *const AnyObject = std::ptr::null();
    let ctl: Option<Retained<AnyObject>> = unsafe {
        let alloc: *mut AnyObject = msg_send![cls, alloc];
        let obj: *mut AnyObject = msg_send![alloc, initWithStartingUpdater: Bool::YES, updaterDelegate: none, userDriverDelegate: none];
        Retained::from_raw(obj)   // +1 from init
    };
    let Some(ctl) = ctl else { return false };
    CONTROLLER.with_borrow_mut(|c| *c = Some(ctl));
    log::info!("Sparkle: updater started (automatic checks {})", automatic());
    true
}
/// The bundle carries Sparkle (known before the updater starts — the Settings switch is live only then).
pub fn bundled() -> bool { NSBundle::mainBundle().privateFrameworksPath().is_some_and(|d| std::path::Path::new(&format!("{d}/Sparkle.framework")).exists()) }
fn updater() -> Option<Retained<AnyObject>> { CONTROLLER.with_borrow(|c| c.as_ref().and_then(|ctl| unsafe { let u: *mut AnyObject = msg_send![&**ctl, updater]; Retained::retain(u) })) }
/// About ▸ Check for Updates… (Sparkle shows its own window).
pub fn check_now() {
    if !start() { log::info!("Sparkle: Check for Updates — no updater in this build"); return }
    let none: *const AnyObject = std::ptr::null();
    CONTROLLER.with_borrow(|c| if let Some(ctl) = c { let _: () = unsafe { msg_send![&**ctl, checkForUpdates: none] }; });
}
pub fn automatic() -> bool { updater().map(|u| { let b: Bool = unsafe { msg_send![&*u, automaticallyChecksForUpdates] }; b.as_bool() }).unwrap_or(false) }
pub fn set_automatic(on: bool) { if let Some(u) = updater() { let _: () = unsafe { msg_send![&*u, setAutomaticallyChecksForUpdates: Bool::new(on)] }; } }
