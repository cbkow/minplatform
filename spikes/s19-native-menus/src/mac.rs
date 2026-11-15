//! macOS: an NSMenu with our own rows and the system's rows for a set of files, from public API only.
//! Nothing here launches an application or opens a Finder window unless S19_LIVE=1 (the owner's hand, Part B).
use crate::{counters, delta, fail, short, show, Files, Step, BUSY};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, sel, MainThreadMarker, AllocAnyThread, MainThreadOnly, Message};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType, NSImage, NSMenu, NSMenuItem, NSPasteboard, NSSharingServicePicker, NSView, NSWorkspace, NSWorkspaceOpenConfiguration};
use objc2_foundation::{ns_string, NSArray, NSFileManager, NSObject, NSObjectProtocol, NSPoint, NSProcessInfo, NSRect, NSRunLoop, NSSize, NSString, NSTimer, NSURL};
use objc2_uniform_type_identifiers::UTType;
use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Instant;

static TICK_DEFAULT: AtomicU64 = AtomicU64::new(0);
static TICK_COMMON: AtomicU64 = AtomicU64::new(0);
pub fn ticks() -> (u64, u64) { (TICK_DEFAULT.load(Relaxed), TICK_COMMON.load(Relaxed)) }

#[derive(Clone, Copy, PartialEq, Debug)]
enum How { Positioning, ContextEvent }
struct Spec { label: String, paths: Vec<PathBuf>, how: How, at: (f64, f64), cancel_after: Option<f64>, probe: bool, pick: bool, deferred: bool }

#[derive(Default)]
struct State {
    target: Option<Retained<Target>>, view: Option<Retained<NSView>>, requestor: Option<Retained<Requestor>>,
    /// the files the menu on screen is for, and the share picker that owns its Share row (it must outlive the menu)
    paths: Vec<PathBuf>, picker: Option<Retained<NSSharingServicePicker>>,
    chosen: Option<isize>, would: Option<String>, pending: Option<Spec>, probe: Option<String>, asked: Vec<String>, cancels: u32,
    log: PathBuf, dir: PathBuf,
}
thread_local! { static ST: RefCell<State> = RefCell::new(State::default()); }
fn st<R>(f: impl FnOnce(&mut State) -> R) -> R { ST.with(|s| f(&mut s.borrow_mut())) }
fn log(line: &str) { let p = st(|s| s.log.clone()); if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open(p) { let _ = writeln!(f, "{line}"); } println!("      [log] {line}"); }
fn shorten(p: &Path) -> String { let dir = st(|s| s.dir.clone()); p.strip_prefix(&dir).map(|r| format!("<files>/{}", r.display())).unwrap_or_else(|_| p.display().to_string()) }

define_class!(
    /// The target of every row, of the timers and of the delayed calls.
    #[unsafe(super(NSObject))] #[thread_kind = MainThreadOnly] #[name = "S19Target"]
    struct Target;
    unsafe impl NSObjectProtocol for Target {}
    impl Target {
        /// One of OUR rows: its tag is our command's id. This is what the application would act on.
        #[unsafe(method(ours:))]
        fn ours(&self, sender: &NSMenuItem) { let id = sender.tag(); st(|s| s.chosen = Some(id)); log(&format!("chosen: OURS id {id} \"{}\"", sender.title())); }
        /// "Open With <application>": the application's URL rides on the row.
        #[unsafe(method(openWith:))]
        fn open_with(&self, sender: &NSMenuItem) {
            let Some(app) = sender.representedObject().and_then(|o| o.downcast::<NSURL>().ok()) else { return };
            let paths = st(|s| s.paths.clone()); let app_path = app.path().map(|p| p.to_string()).unwrap_or_default();
            let line = format!("open {:?} with {}", paths.iter().map(|p| shorten(p)).collect::<Vec<_>>(), app_path);
            st(|s| s.would = Some(app_path.clone()));
            if std::env::var("S19_LIVE").is_ok() { log(&format!("LIVE: {line}")); let urls = urls_of(&paths);
                NSWorkspace::sharedWorkspace().openURLs_withApplicationAtURL_configuration_completionHandler(&urls, &app, &NSWorkspaceOpenConfiguration::configuration(), None); }
            else { log(&format!("would (nothing is launched): {line}")); }
        }
        /// The other system rows: logged; only "Show in Finder" is real, and only under S19_LIVE.
        #[unsafe(method(system:))]
        fn system(&self, sender: &NSMenuItem) {
            let title = sender.title().to_string(); let paths = st(|s| s.paths.clone()); st(|s| s.would = Some(title.clone()));
            if title == "Show in Finder" && std::env::var("S19_LIVE").is_ok() { log("LIVE: Show in Finder"); NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&urls_of(&paths)); return }
            log(&format!("would: \"{title}\" for {:?}", paths.iter().map(|p| shorten(p)).collect::<Vec<_>>()));
        }
        #[unsafe(method(cancel:))]
        fn cancel(&self, menu: &NSMenu) { st(|s| s.cancels += 1); menu.cancelTracking(); }
        /// While the menu is tracked: what does it hold now (did AppKit append Services)?
        #[unsafe(method(probe:))]
        fn probe(&self, menu: &NSMenu) { let titles: Vec<String> = menu.itemArray().iter().map(|i| if i.isSeparatorItem() { "--".into() } else if i.hasSubmenu() { format!("{} >", i.title()) } else { i.title().to_string() }).collect(); st(|s| s.probe = Some(titles.join(" | "))); }
        /// While the menu is tracked: choose its first row by program.
        #[unsafe(method(pick:))]
        fn pick(&self, menu: &NSMenu) { menu.performActionForItemAtIndex(0); }
        #[unsafe(method(tickDefault:))]
        fn tick_default(&self, _t: &NSTimer) { TICK_DEFAULT.fetch_add(1, Relaxed); }
        #[unsafe(method(tickCommon:))]
        fn tick_common(&self, _t: &NSTimer) { TICK_COMMON.fetch_add(1, Relaxed); }
        /// A popup started from the run loop itself, not from inside one of winit's callbacks.
        #[unsafe(method(deferred:))]
        fn deferred(&self, _o: Option<&AnyObject>) { if let Some(spec) = st(|s| s.pending.take()) { popup(spec); } BUSY.store(false, Relaxed); }
    }
);

define_class!(
    /// A view that says "the selection is these files" to the Services machinery. The context menu is shown FOR it.
    #[unsafe(super(NSView))] #[thread_kind = MainThreadOnly] #[name = "S19Requestor"]
    struct Requestor;
    unsafe impl NSObjectProtocol for Requestor {}
    impl Requestor {
        #[unsafe(method_id(validRequestorForSendType:returnType:))]
        fn valid(&self, send: Option<&NSString>, ret: Option<&NSString>) -> Option<Retained<AnyObject>> {
            let (send, ret) = (send.map(|s| s.to_string()).unwrap_or_default(), ret.map(|s| s.to_string()).unwrap_or_default());
            st(|s| { let k = format!("{send} -> {}", if ret.is_empty() { "nothing" } else { &ret }); if !s.asked.contains(&k) { s.asked.push(k); } });
            if ret.is_empty() && (send == "public.file-url" || send == "NSFilenamesPboardType") { let me: &AnyObject = self.as_ref(); Some(me.retain()) } else { None }
        }
        /// Called only when a service is CHOSEN: the files go on the pasteboard the system hands over (not the clipboard).
        #[unsafe(method(writeSelectionToPasteboard:types:))]
        fn write(&self, pboard: &NSPasteboard, _types: &NSArray<NSString>) -> bool {
            let paths = st(|s| s.paths.clone()); log(&format!("Services asked for the selection ({} file(s)) on the pasteboard named \"{}\"", paths.len(), pboard.name()));
            let urls = urls_of(&paths); pboard.clearContents(); let objs: Retained<NSArray<AnyObject>> = unsafe { Retained::cast_unchecked(urls) };
            unsafe { msg_send![pboard, writeObjects: &*objs] }
        }
    }
);

fn url_of(p: &Path) -> Retained<NSURL> { NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy())) }
fn urls_of(paths: &[PathBuf]) -> Retained<NSArray<NSURL>> { NSArray::from_retained_slice(&paths.iter().map(|p| url_of(p)).collect::<Vec<_>>()) }
fn app_name(app: &NSURL) -> String { app.path().map(|p| { let n = NSFileManager::defaultManager().displayNameAtPath(&p).to_string(); n.strip_suffix(".app").map(str::to_string).unwrap_or(n) }).unwrap_or_default() }
/// Finder tells two copies of one application apart by their version.
fn app_version(app: &NSURL) -> String { objc2_foundation::NSBundle::bundleWithURL(app).and_then(|b| b.objectForInfoDictionaryKey(ns_string!("CFBundleShortVersionString"))).and_then(|v| v.downcast::<NSString>().ok()).map(|v| v.to_string()).unwrap_or_default() }
fn app_names(apps: &[Retained<NSURL>]) -> Vec<String> { let names: Vec<String> = apps.iter().map(|a| app_name(a)).collect(); names.iter().enumerate().map(|(i, n)| if names.iter().filter(|m| *m == n).count() > 1 { format!("{n} ({})", app_version(&apps[i])) } else { n.clone() }).collect() }
fn app_path(app: &NSURL) -> String { app.path().map(|p| p.to_string()).unwrap_or_default() }
/// The applications the system offers for one file: the default, and all of them (the default first, the rest by name).
fn apps_for(path: &Path) -> (Option<Retained<NSURL>>, Vec<Retained<NSURL>>) {
    let ws = NSWorkspace::sharedWorkspace(); let url = url_of(path);
    let def = ws.URLForApplicationToOpenURL(&url); let mut rest: Vec<Retained<NSURL>> = ws.URLsForApplicationsToOpenURL(&url).iter().collect();
    rest.sort_by_key(|a| app_name(a).to_lowercase());
    let mut out: Vec<Retained<NSURL>> = def.iter().cloned().collect();
    for a in rest { if !out.iter().any(|o| app_path(o) == app_path(&a)) { out.push(a); } }
    (def, out)
}
/// For several files: the applications that open every one of them; a default only if they all share it.
fn apps_for_all(paths: &[PathBuf]) -> (Option<Retained<NSURL>>, Vec<Retained<NSURL>>) {
    let mut it = paths.iter(); let Some(first) = it.next() else { return (None, vec![]) }; let (mut def, mut apps) = apps_for(first);
    for p in it { let (d, a) = apps_for(p); apps.retain(|x| a.iter().any(|y| app_path(y) == app_path(x))); if d.as_ref().map(|d| app_path(d)) != def.as_ref().map(|d| app_path(d)) { def = None; } }
    if def.is_none() { apps.sort_by_key(|a| app_name(a).to_lowercase()); }
    (def, apps)
}

fn row(mtm: MainThreadMarker, title: &str, action: Option<Sel>, tag: isize) -> Retained<NSMenuItem> {
    let it = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str(title), action, ns_string!("")) };
    if action.is_some() { let t = st(|s| s.target.clone()); if let Some(t) = t { unsafe { it.setTarget(Some(&t)) }; } }
    it.setTag(tag); it
}
fn icon(path: &str) -> Retained<NSImage> { let i = NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(path)); i.setSize(NSSize::new(16.0, 16.0)); i }

/// Our ids (the application's commands). The system's rows carry tag 0.
pub const RENAME: isize = 1; pub const TRACKED: isize = 2; pub const OTHER_PANE: isize = 3; pub const BOOKMARK: isize = 4;

/// The menu: our rows, then the system's for these files.
fn build(mtm: MainThreadMarker, paths: &[PathBuf]) -> Retained<NSMenu> {
    st(|s| s.paths = paths.to_vec());
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("S19")); menu.setAutoenablesItems(false);
    // ours: one plain, one disabled, one with a submenu
    menu.addItem(&row(mtm, if paths.len() == 1 { "Rename" } else { "Rename Items…" }, Some(sel!(ours:)), RENAME));
    let tracked = row(mtm, "Mark as Tracked", Some(sel!(ours:)), TRACKED); tracked.setEnabled(false); menu.addItem(&tracked);
    let copy = row(mtm, "Copy to", None, 0); let sub = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("Copy to")); sub.setAutoenablesItems(false);
    sub.addItem(&row(mtm, "Other Pane", Some(sel!(ours:)), OTHER_PANE)); sub.addItem(&row(mtm, "Bookmarks…", Some(sel!(ours:)), BOOKMARK)); copy.setSubmenu(Some(&sub)); menu.addItem(&copy);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    // the system's: Open With
    let ow = row(mtm, "Open With", None, 0); let owm = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("Open With")); owm.setAutoenablesItems(false);
    let (def, apps) = apps_for_all(paths);
    let names = app_names(&apps);
    for (i, a) in apps.iter().enumerate() {
        let is_def = i == 0 && def.is_some(); let name = names[i].clone();
        let it = row(mtm, &if is_def { format!("{name} (default)") } else { name }, Some(sel!(openWith:)), 0);
        unsafe { it.setRepresentedObject(Some(a)) }; it.setImage(Some(&icon(&app_path(a)))); owm.addItem(&it);
        if is_def && apps.len() > 1 { owm.addItem(&NSMenuItem::separatorItem(mtm)); }
    }
    if !apps.is_empty() { owm.addItem(&NSMenuItem::separatorItem(mtm)); }
    owm.addItem(&row(mtm, "Other…", Some(sel!(system:)), 0)); ow.setSubmenu(Some(&owm)); menu.addItem(&ow);
    menu.addItem(&row(mtm, "Show in Finder", Some(sel!(system:)), 0));
    menu.addItem(&row(mtm, "Get Info", Some(sel!(system:)), 0));
    menu.addItem(&row(mtm, "Quick Look", Some(sel!(system:)), 0));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    // Share: AppKit's own row (macOS 13+); the picker owns it
    let objs: Retained<NSArray<AnyObject>> = unsafe { Retained::cast_unchecked(urls_of(paths)) };
    let picker = unsafe { NSSharingServicePicker::initWithItems(NSSharingServicePicker::alloc(), &objs) };
    let share = picker.standardShareMenuItem(mtm); menu.addItem(&share); st(|s| s.picker = Some(picker));
    // Tags: the label names are public, and so is the files' tag list (NSURLTagNamesKey)
    let tg = row(mtm, "Tags", None, 0); let tgm = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("Tags")); tgm.setAutoenablesItems(false);
    for (i, name) in NSWorkspace::sharedWorkspace().fileLabels().iter().enumerate() { if i == 0 { continue } tgm.addItem(&row(mtm, &name.to_string(), Some(sel!(system:)), 0)); }
    tg.setSubmenu(Some(&tgm)); menu.addItem(&tg);
    menu
}

fn dump(menu: &NSMenu, depth: usize, out: &mut String) {
    for it in menu.itemArray().iter() {
        let pad = "  ".repeat(depth + 2);
        if it.isSeparatorItem() { out.push_str(&format!("{pad}--------\n")); continue }
        let mut flags = vec![]; if !it.isEnabled() { flags.push("disabled".to_string()); } if it.image().is_some() { flags.push("icon".into()); }
        if it.tag() != 0 { flags.push(format!("OURS id {}", it.tag())); } if let Some(a) = it.action() { flags.push(format!("-> {}", a.name().to_string_lossy())); }
        let sub = it.submenu(); if let Some(s) = &sub { flags.push(format!("submenu of {}", s.numberOfItems())); }
        out.push_str(&format!("{pad}{}{}\n", it.title(), if flags.is_empty() { String::new() } else { format!("   [{}]", flags.join(", ")) }));
        if let Some(s) = sub { dump(&s, depth + 1, out); }
    }
}
fn find(menu: &NSMenu, title: &str) -> Option<(isize, Retained<NSMenuItem>)> { menu.itemArray().iter().enumerate().find(|(_, i)| i.title().to_string().starts_with(title)).map(|(n, i)| (n as isize, i)) }

pub fn init(view: *mut std::ffi::c_void, files: &Files) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let view: Retained<NSView> = unsafe { Retained::retain(view as *mut NSView) }.expect("the view");
    let target: Retained<Target> = unsafe { msg_send![super(Target::alloc(mtm).set_ivars(())), init] };
    let requestor: Retained<Requestor> = unsafe { msg_send![super(Requestor::alloc(mtm).set_ivars(())), initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0))] };
    view.addSubview(&requestor);
    // AppKit asks a view only about the types the APPLICATION has registered for Services (text types by default)
    if std::env::var("S19_NO_REGISTER").is_err() { let send = NSArray::from_retained_slice(&[NSString::from_str("public.file-url"), NSString::from_str("NSFilenamesPboardType")]); let none: Retained<NSArray<NSString>> = NSArray::new();
        objc2_app_kit::NSApplication::sharedApplication(mtm).registerServicesMenuSendTypes_returnTypes(&send, &none); }
    // two timers at 100 Hz: one in the default run-loop mode only, one in the common modes (which hold event tracking)
    unsafe {
        let t = NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(0.01, &target, sel!(tickDefault:), None, true); NSRunLoop::mainRunLoop().addTimer_forMode(&t, objc2_foundation::NSDefaultRunLoopMode);
        let t = NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(0.01, &target, sel!(tickCommon:), None, true); NSRunLoop::mainRunLoop().addTimer_forMode(&t, objc2_foundation::NSRunLoopCommonModes);
    }
    st(|s| { s.target = Some(target); s.view = Some(view); s.requestor = Some(requestor); s.log = files.log.clone(); s.dir = files.dir.clone(); });
}

/// Show the menu and block until it is gone. Called from inside a winit callback, or from the run loop (`deferred:`).
fn popup(spec: Spec) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let (Some(target), Some(view), Some(requestor)) = st(|s| (s.target.clone(), s.view.clone(), s.requestor.clone())) else { return };
    let menu = build(mtm, &spec.paths);
    let modes = NSArray::from_slice(&[unsafe { objc2_foundation::NSRunLoopCommonModes }]);
    let later = |sel: Sel, after: f64| unsafe { let _: () = msg_send![&*target, performSelector: sel, withObject: &*menu, afterDelay: after, inModes: &*modes]; };
    if spec.probe { later(sel!(probe:), 0.5); } if spec.pick { later(sel!(pick:), 0.5); } if let Some(s) = spec.cancel_after { later(sel!(cancel:), s); }
    st(|s| { s.chosen = None; s.would = None; s.probe = None; });
    let pt = NSPoint::new(spec.at.0, if view.isFlipped() { spec.at.1 } else { view.bounds().size.height - spec.at.1 });
    let (c, t) = (counters(), Instant::now());
    let returned = match spec.how {
        How::Positioning => Some(menu.popUpMenuPositioningItem_atLocation_inView(None, pt, Some(&view))),
        How::ContextEvent => {
            // an event OBJECT made for the call (nothing is posted to the system)
            let wn = view.window().map(|w| w.windowNumber()).unwrap_or(0);
            match NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(NSEventType::RightMouseDown, view.convertPoint_toView(pt, None), NSEventModifierFlags::empty(), NSProcessInfo::processInfo().systemUptime(), wn, None, 0, 1, 1.0) {
                Some(ev) => NSMenu::popUpContextMenu_withEvent_forView(&menu, &ev, &requestor), None => fail("no event object") }
            None
        }
    };
    let (dt, d) = (t.elapsed().as_secs_f64(), delta(c));
    unsafe { let _: () = msg_send![objc2::class!(NSObject), cancelPreviousPerformRequestsWithTarget: &*target]; }
    let (chosen, would, probe) = st(|s| (s.chosen, s.would.clone(), s.probe.take()));
    println!("   {:<26} {:>12} {:>11}  up {dt:4.2} s  returned {:<5}  ours chosen {:<8} | {}", spec.label, format!("{:?}", spec.how), if spec.deferred { "run loop" } else { "in handler" }, returned.map(|b| b.to_string()).unwrap_or("-".into()), chosen.map(|c| format!("id {c}")).unwrap_or("none".into()), show(d));
    if let Some(p) = probe { println!("      while tracked the menu held: {p}"); let asked = st(|s| std::mem::take(&mut s.asked)); let files: Vec<&String> = asked.iter().filter(|a| a.starts_with("public.file-url") || a.starts_with("NSFilenamesPboardType")).collect(); println!("      Services asked our view about {} send/return pair(s); for files: {}", asked.len(), if files.is_empty() { "none".into() } else { files.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("; ") }); }
    if let Some(w) = would { println!("      a system row was chosen: {w}"); }
    if let Some(s) = spec.cancel_after { if !spec.pick && (dt < s * 0.8 || dt > s + 1.5) { let front = NSWorkspace::sharedWorkspace().frontmostApplication().and_then(|a| a.localizedName()).map(|n| n.to_string()).unwrap_or_default();
            fail(&format!("the menu was up {dt:.2} s, asked for {s}: closed by something else (our application active: {}; frontmost: {front})", objc2_app_kit::NSApplication::sharedApplication(mtm).isActive())); } if d[4] == 0 { fail("no common-modes timer tick while tracking"); } }
    if spec.pick && chosen != Some(RENAME) { fail("the row chosen by program during tracking did not reach our callback"); }
}
fn defer(spec: Spec) { let t = st(|s| { s.pending = Some(spec); s.target.clone() }); BUSY.store(true, Relaxed); if let Some(t) = t { unsafe { let _: () = msg_send![&*t, performSelector: sel!(deferred:), withObject: None::<&AnyObject>, afterDelay: 0.0f64]; } } }

/// Part B: the owner's right click.
pub fn hand(_view: *mut std::ffi::c_void, at: (f64, f64), paths: Vec<PathBuf>) {
    if BUSY.load(Relaxed) { return }
    let how = if std::env::var("S19_HOW").as_deref() == Ok("positioning") { How::Positioning } else { How::ContextEvent };
    defer(Spec { label: format!("hand, {} file(s)", paths.len()), paths, how, at, cancel_after: None, probe: true, pick: false, deferred: true });
}

pub fn step(s: Step, _view: *mut std::ffi::c_void, files: &Files) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    match s {
        Step::Enumerate => {
            println!("\n== the applications the system offers (NSWorkspace URLForApplicationToOpenURL / URLsForApplicationsToOpenURL)");
            for (label, p) in files.all() {
                let (def, apps) = apps_for(&p);
                let ut = if p.is_dir() { UTType::typeWithIdentifier(ns_string!("public.folder")) } else { p.extension().and_then(|e| UTType::typeWithFilenameExtension(&NSString::from_str(&e.to_string_lossy()))) };
                let by_type = ut.as_ref().map(|t| NSWorkspace::sharedWorkspace().URLsForApplicationsToOpenContentType(t).len());
                println!("   {label}  ({}; type {}): default {}; {} by URL, {} by content type", short(files, &p), ut.as_ref().map(|t| t.identifier().to_string()).unwrap_or("?".into()), def.as_ref().map(|d| app_name(d)).unwrap_or("none".into()), apps.len(), by_type.map(|n| n.to_string()).unwrap_or("?".into()));
                println!("      {}", app_names(&apps).join(", "));
                if !p.is_dir() && apps.is_empty() { fail("no application offered"); }
            }
        }
        Step::Menus => {
            for (label, paths) in [("ONE file (.txt)", files.one()), ("TWO files (.txt, .txt)", files.many()), ("THREE files of three types (.txt, .png, .pdf)", files.mixed())] {
                println!("\n== the menu for {label}"); let menu = build(mtm, &paths); let mut out = String::new(); dump(&menu, 0, &mut out); print!("{out}");
                let ours: Vec<_> = menu.itemArray().iter().take(3).collect();
                if !(ours.len() == 3 && ours[0].isEnabled() && !ours[1].isEnabled() && ours[2].hasSubmenu()) { fail("our three rows are not as asked"); }
                let Some((_, ow)) = find(&menu, "Open With") else { fail("no Open With"); continue }; let owm = ow.submenu().expect("submenu");
                let first = owm.itemAtIndex(0);
                if paths.len() == 1 && !first.as_ref().is_some_and(|f| f.title().to_string().ends_with("(default)") && f.image().is_some()) { fail("Open With: no default with an icon first"); }
                for t in ["Get Info", "Show in Finder", "Share", "Tags"] { if find(&menu, t).is_none() { fail(&format!("no {t}")); } }
            }
        }
        Step::Plumbing => {
            println!("\n== plumbing: each row's action is invoked by program (performActionForItemAtIndex:), no menu on screen");
            let paths = files.one(); let menu = build(mtm, &paths); let take = || st(|s| (s.chosen.take(), s.would.take()));
            menu.performActionForItemAtIndex(0); let r = take(); println!("   row 0 (Rename): our callback got {:?}", r.0); if r.0 != Some(RENAME) { fail("Rename did not reach our callback"); }
            menu.performActionForItemAtIndex(1); let r = take(); println!("   row 1 (Mark as Tracked, DISABLED): our callback got {:?}", r.0); if r.0.is_some() { fail("a disabled row fired"); }
            let sub = menu.itemAtIndex(2).and_then(|i| i.submenu()).expect("submenu"); sub.performActionForItemAtIndex(0); let r = take(); println!("   row 2 > 0 (Copy to > Other Pane): our callback got {:?}", r.0); if r.0 != Some(OTHER_PANE) { fail("the submenu row did not reach our callback"); }
            let (n, ow) = find(&menu, "Open With").expect("Open With"); let _ = n; let owm = ow.submenu().expect("submenu"); owm.performActionForItemAtIndex(0); let r = take();
            let want = NSWorkspace::sharedWorkspace().URLForApplicationToOpenURL(&url_of(&paths[0])).map(|u| app_path(&u));
            println!("   Open With > row 0: would use {:?}; the system's default for the file is {:?}", r.1, want); if r.1.is_none() || r.1 != want { fail("Open With would not use the default application"); }
            let last = owm.numberOfItems() - 1; if last >= 2 { let it = owm.itemAtIndex(last - 2).expect("row"); if !it.isSeparatorItem() { owm.performActionForItemAtIndex(last - 2); let r = take(); println!("   Open With > \"{}\": would use {:?}", it.title(), r.1); if r.1.is_none() { fail("an Open With row carries no application"); } } }
            for t in ["Get Info", "Show in Finder"] { let (n, _) = find(&menu, t).expect("row"); menu.performActionForItemAtIndex(n); let r = take(); println!("   {t}: {:?}", r.1); if r.1.as_deref() != Some(t) { fail("a system row did not fire"); } }
            let text = std::fs::read_to_string(&files.log).unwrap_or_default(); println!("   the log file holds {} line(s)", text.lines().count()); if text.lines().count() < 5 { fail("the log file is short"); }
        }
        Step::PopInHandler(n) => { println!("\n== ONE popup shown from INSIDE a winit callback (proxy_wake_up)");
            popup(Spec { label: format!("#{n} {}", if n % 2 == 1 { "one file" } else { "two files" }), paths: if n % 2 == 1 { files.one() } else { files.many() }, how: How::Positioning, at: (60.0 + 8.0 * n as f64, 60.0), cancel_after: Some(1.0), probe: false, pick: false, deferred: false }); }
        Step::PopDeferred(n) => { if n == 1 { println!("\n== popups, shown from the run loop and cancelled by program after 1 s (the counts: what ran WHILE the menu was tracked)"); } defer(Spec { label: format!("#{} {}", n, if n % 2 == 1 { "one file" } else { "three files" }), paths: if n % 2 == 1 { files.one() } else { files.mixed() }, how: How::Positioning, at: (60.0 + 8.0 * n as f64, 90.0), cancel_after: Some(1.0), probe: false, pick: false, deferred: true }); }
        Step::PopContext(n) => defer(Spec { label: format!("#{} context, {}", n + 20, ["one file", "two files", "a folder"][n as usize - 1]), paths: match n { 1 => files.one(), 2 => files.many(), _ => vec![files.folder.clone()] }, how: How::ContextEvent, at: (80.0, 120.0), cancel_after: Some(1.0), probe: true, pick: false, deferred: true }),
        Step::PopPick => { defer(Spec { label: "#24 row 0 picked at 0.5 s".into(), paths: files.one(), how: How::Positioning, at: (80.0, 140.0), cancel_after: Some(1.5), probe: false, pick: true, deferred: true }); }
        _ => {}
    }
}
