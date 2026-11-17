//! Start at sign-in (canon rows 457, 519 point 6, 535). The INSTALLED application's alone: it is the system that
//! starts it, by the application's identity - a build run from the source tree has none.
//!
//! - macOS 13 and later: the application registers ITSELF as a login item (`SMAppService`); the user sees it under
//!   System Settings > General > Login Items and can take it out there. Off until asked for.
//! - Windows: the Store package's startup task (its id is 1.4.x's, `ufbStartup`); the system may hold it "disabled by
//!   the user" or "by policy" - then only the user can turn it on, in Settings > Apps > Startup, and it is said so.
//!
//! [`available`]: the switch can be used at all. [`enabled`]: how it stands now. [`set`]: make it so, or say why not.

#[cfg(target_os = "macos")]
mod imp {
    use objc2::runtime::{AnyClass, AnyObject}; use objc2::msg_send;
    #[link(name = "ServiceManagement", kind = "framework")] unsafe extern "C" {}
    fn service() -> Option<*mut AnyObject> {
        // (only a bundle with an identity can be a login item)
        let bundled = objc2_foundation::NSBundle::mainBundle().bundleIdentifier().is_some(); if !bundled { return None }
        let cls = AnyClass::get(c"SMAppService")?; let s: *mut AnyObject = unsafe { msg_send![cls, mainAppService] }; (!s.is_null()).then_some(s)
    }
    pub fn available() -> bool { service().is_some() }
    /// (the service's status: 0 not registered, 1 enabled, 2 waits for the user's approval, 3 not found)
    pub fn enabled() -> bool { service().is_some_and(|s| { let st: isize = unsafe { msg_send![s, status] }; st == 1 }) }
    pub fn set(on: bool) -> Result<(), String> {
        let s = service().ok_or("Only the installed application starts at sign-in.")?; let mut err: *mut AnyObject = std::ptr::null_mut();
        let ok: bool = unsafe { if on { msg_send![s, registerAndReturnError: &mut err] } else { msg_send![s, unregisterAndReturnError: &mut err] } };
        if ok { return Ok(()) }
        let why = unsafe { if err.is_null() { None } else { let d: *mut objc2_foundation::NSString = msg_send![err, localizedDescription]; d.as_ref().map(|d| d.to_string()) } };
        Err(format!("Start at sign-in could not be {}: {}", if on { "turned on" } else { "turned off" }, why.unwrap_or_else(|| "the system gave no reason".into())))
    }
}

#[cfg(windows)]
mod imp {
    use windows::ApplicationModel::{StartupTask, StartupTaskState}; use windows::core::HSTRING;
    /// The package's startup task; none when this is no package (a build run from a folder).
    fn task() -> Option<StartupTask> { StartupTask::GetAsync(&HSTRING::from("ufbStartup")).ok()?.join().ok() }
    pub fn available() -> bool { task().is_some() }
    pub fn enabled() -> bool { task().and_then(|t| t.State().ok()).is_some_and(|s| s == StartupTaskState::Enabled || s == StartupTaskState::EnabledByPolicy) }
    pub fn set(on: bool) -> Result<(), String> {
        let t = task().ok_or("Only the installed application starts at sign-in.")?;
        if !on { return t.Disable().map_err(|e| format!("Start at sign-in could not be turned off: {e}")) }
        match t.RequestEnableAsync().and_then(|a| a.join()) {
            Ok(StartupTaskState::Enabled) | Ok(StartupTaskState::EnabledByPolicy) => Ok(()),
            Ok(StartupTaskState::DisabledByUser) => Err("Windows keeps it off because it was turned off there: turn it on in Settings > Apps > Startup.".into()),
            Ok(StartupTaskState::DisabledByPolicy) => Err("This machine's policy does not let applications start at sign-in.".into()),
            Ok(_) => Err("Windows did not turn it on.".into()),
            Err(e) => Err(format!("Start at sign-in could not be turned on: {e}")),
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod imp { pub fn available() -> bool { false } pub fn enabled() -> bool { false } pub fn set(_: bool) -> Result<(), String> { Err("Not on this system.".into()) } }

pub use imp::{available, enabled, set};
