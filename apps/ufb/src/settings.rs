//! What the application remembers between launches (the family's settings service, `min-settings`): one JSON file in
//! the system's place for it, under this development build's own name - nothing of an installed 1.4.x is read or
//! written (canon row 463, point 5). `UFB_SETTINGS_DIR=<folder>` moves the whole store (scripted looks, tests).
//!
//! So far: the player's loop, mute and volume; the rate image sequences play at - the one chosen last, and the one
//! chosen for each sequence (row 478).
use min_settings::{Key, Schema, Settings};
use std::sync::{Mutex, OnceLock};

static SCHEMA: Schema = Schema { keys: &[
    Key::bool("player/loop", true), Key::bool("player/muted", false), Key::float("player/volume", 1.0, 0.0, 1.0),
    // the rate a sequence plays at when nothing is remembered for it: the one chosen last (`num/den`)
    Key::str("player/sequence_rate", "24/1"),
    // the rate chosen for each sequence: lines of `num/den <tab> the sequence's pattern`, the newest first
    Key::str("player/sequence_rates", ""),
    // the columns of a list, a kind of panel (row 488): the browser's panes, a job's first pane, a job's second - `shown|widths`
    // the item list of a job shows the date its items were changed
    Key::bool("columns/items_modified", true),
    // the saved shares (row 498): a list, as JSON - names and addresses, never a password (the system keeps those)
    Key::str("shares/list", ""),
    // sync (row 499): on, the farm's folder, this machine's name in it (empty: the system's), whether it may lead
    // ("" | "leader" | "noleader"), and - a development build's belt - the one folder whose rows take part
    Key::bool("sync/on", false), Key::str("sync/farm", ""), Key::str("sync/node", ""), Key::str("sync/leader", ""), Key::str("sync/only", ""),
    // what a pane shows from the start (row 516): hidden files, folders before files, sequences as one entry, a grid
    Key::bool("view/hidden", false), Key::bool("view/folders_first", true), Key::bool("view/sequences", true), Key::bool("view/grid", false), Key::bool("view/second_pane", true),
    // (the installed application's: wired with packaging)
    Key::bool("general/start_at_sign_in", false),
    // the window as it was left (row 523): its size in points (`1280x800`), whether it filled the screen, and the
    // dividers - the sidebar's width, the two panes' share side by side, the same in a job (one above the other),
    // the width of a job's item list
    Key::str("window/size", ""), Key::bool("window/maximized", false),
    Key::float("window/sidebar", 0.0, 0.0, 4000.0), Key::float("window/split", 0.5, 0.15, 0.85), Key::float("window/job_split", 0.5, 0.15, 0.85), Key::float("window/job_list", 0.0, 0.0, 4000.0),
    // the move from 1.4.x was made (row 532): when, in seconds since 1970; empty: not yet
    Key::str("migrate/from_1_4", ""),
    // the favourites the user made (row 531): a list, as JSON - each a label and the folder as the others would read it
    Key::str("favorites/list", ""),
    // path mappings that are no saved share's (row 517): a list, as JSON - a name, the Windows side, the macOS side, on or off
    Key::str("mappings/list", ""),
    Key::str("columns/browser", "1,1,1|150,86,150"), Key::str("columns/job_top", "1,0,0|150,86,150"), Key::str("columns/job_low", "1,0,0|150,86,150"),
] };
static STORE: OnceLock<Mutex<Settings>> = OnceLock::new();
/// This application's own settings file (row 534). NOT `settings.json`: in the release this build's data folder is UFB
/// 1.4.x's, and that name is 1.4.x's own file there - never written by us (2.0.1 to 2.0.3 read it once, to take 1.4.x over).
pub const FILE: &str = "ufb2-settings.json";
/// Builds before row 534 kept their settings as `settings.json`: in the development build and the test package, whose
/// folders are theirs alone, that file is taken over once. Never in the release - there it is 1.4.x's.
fn carry_over() {
    if crate::identity::kind() == crate::identity::Kind::Release { return }
    let Some(dir) = crate::identity::data_dir() else { return }; let (old, new) = (dir.join("settings.json"), dir.join(FILE));
    if !new.exists() && old.is_file() { let _ = std::fs::copy(&old, &new); }
}

/// Read or change the store. A change is written at once (the file is small).
pub fn with<R>(f: impl FnOnce(&mut Settings) -> R) -> R {
    let mut s = STORE.get_or_init(|| { carry_over(); Mutex::new(Settings::open_named(crate::identity::data_name(), "UFB", &SCHEMA, FILE)) }).lock().unwrap();
    let r = f(&mut s); let _ = s.save_if_dirty(); r
}

/// How many sequences' rates are kept.
const KEPT: usize = 300;
fn parse_rate(s: &str) -> Option<(u32, u32)> { let (n, d) = s.split_once('/')?; let (n, d) = (n.trim().parse().ok()?, d.trim().parse().ok()?); (n > 0 && d > 0).then_some((n, d)) }
/// The rate a sequence plays at: the one chosen for it before, else the one chosen last for any, else 24.
pub fn sequence_rate(pattern: &str) -> (u32, u32) {
    with(|s| { let all = s.string("player/sequence_rates");
        all.lines().find_map(|l| l.split_once('\t').filter(|(_, p)| *p == pattern).and_then(|(r, _)| parse_rate(r))).or_else(|| parse_rate(&s.string("player/sequence_rate"))).unwrap_or((24, 1)) })
}
/// A rate was chosen for a sequence: remembered for it, and as the one chosen last.
pub fn set_sequence_rate(pattern: &str, rate: (u32, u32)) {
    with(|s| { let all = s.string("player/sequence_rates"); let me = format!("{}/{}\t{pattern}", rate.0, rate.1);
        let rest: Vec<&str> = all.lines().filter(|l| l.split_once('\t').is_some_and(|(_, p)| p != pattern)).take(KEPT - 1).collect();
        s.set_str("player/sequence_rates", &std::iter::once(me.as_str()).chain(rest).collect::<Vec<_>>().join("\n")); s.set_str("player/sequence_rate", &format!("{}/{}", rate.0, rate.1)); });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_sequences_rate_is_remembered_for_it_and_as_the_last_chosen() {
        // (the store of the tests is a folder of their own)
        let dir = std::env::temp_dir().join(format!("ufb-settings-test-{}", std::process::id())); unsafe { std::env::set_var("UFB_SETTINGS_DIR", &dir); }
        assert_eq!(sequence_rate("/a/shot.%04d.exr"), (24, 1), "nothing remembered: 24");
        set_sequence_rate("/a/shot.%04d.exr", (25, 1)); assert_eq!(sequence_rate("/a/shot.%04d.exr"), (25, 1)); assert_eq!(sequence_rate("/b/other.%04d.exr"), (25, 1), "another sequence takes the one chosen last");
        set_sequence_rate("/b/other.%04d.exr", (30000, 1001)); assert_eq!(sequence_rate("/a/shot.%04d.exr"), (25, 1), "each keeps its own"); assert_eq!(sequence_rate("/b/other.%04d.exr"), (30000, 1001)); assert_eq!(sequence_rate("/c/new.%04d.exr"), (30000, 1001));
        set_sequence_rate("/a/shot.%04d.exr", (60, 1)); assert_eq!(with(|s| s.string("player/sequence_rates")).lines().count(), 2, "one line a sequence");
        assert_eq!(parse_rate("24000/1001"), Some((24000, 1001))); assert_eq!(parse_rate("0/1"), None); assert_eq!(parse_rate("x"), None);
        assert!(dir.join(FILE).is_file(), "written to the store's file"); let _ = std::fs::remove_dir_all(&dir);
    }
}
