//! The transport key map — ported from `WindowManager::eventFilter`'s transport table (`window_manager.cpp`, the
//! application-level filter that runs before any QML routing so plain-letter shortcuts work regardless of focus; the
//! stale-text-focus guard stays the shell's). Hold keys (A / J rewind, D / L fast-forward) act on press AND release and
//! ignore auto-repeat (the shuttle's timer drives the scrub); Q / ← and E / → step and ALLOW auto-repeat (hold-to-step);
//! everything else is press-only, no repeat. V (not L) toggles the loop: L is JKL fast-forward and that alias wins. This
//! is row 143's transport slice as a table until the shortcut registry (row 74) lands with the shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key { Char(char), Space, Left, Right, Up, Down, Home, End }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods { pub shift: bool, pub ctrl: bool, pub alt: bool, /// Qt's KeypadModifier counts as plain
    pub keypad: bool }
impl Mods { pub fn plain(&self) -> bool { !self.shift && !self.ctrl && !self.alt } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// hold-to-shuttle: −1 rewind / +1 fast-forward; `ShuttleEnd` on release
    ShuttleBegin(i32), ShuttleEnd, Step(i32), TogglePlayback, SeekToStart, SeekToEnd, SetInPoint, SetOutPoint, ClearInOut,
    ToggleLoop, CycleReviewSpeed, ResetReviewSpeed, ToggleMute, VolumeUp, VolumeDown,
    /// the shell's: cycle the background, screenshot to file / clipboard, fullscreen, annotation undo / redo
    CycleBackground, Screenshot, ScreenshotToClipboard, ToggleFullscreen, AnnotationUndo, AnnotationRedo,
}

/// `pressed` = key down (false = release); `auto_repeat` = the OS's held-key repeat.
pub fn map_key(key: Key, mods: Mods, pressed: bool, auto_repeat: bool) -> Option<Action> {
    let letter = |c: char| if let Key::Char(k) = key { k.eq_ignore_ascii_case(&c) } else { false };
    if !auto_repeat {
        let dir = if letter('a') || letter('j') { -1 } else if letter('d') || letter('l') { 1 } else { 0 };
        if dir != 0 { return Some(if pressed { Action::ShuttleBegin(dir) } else { Action::ShuttleEnd }) }
    }
    if !pressed { return None }
    if mods.plain() {
        if letter('q') || key == Key::Left { return Some(Action::Step(-1)) }
        if letter('e') || key == Key::Right { return Some(Action::Step(1)) }
        if auto_repeat { return None }
        if key == Key::Space || letter('k') || letter('w') || letter('s') { return Some(Action::TogglePlayback) }
        return match key {
            Key::Home => Some(Action::SeekToStart), Key::End => Some(Action::SeekToEnd),
            Key::Up => Some(Action::VolumeUp), Key::Down => Some(Action::VolumeDown),
            Key::Char(c) => match c.to_ascii_lowercase() {
                'm' => Some(Action::ToggleMute), 'b' => Some(Action::CycleBackground), 't' => Some(Action::Screenshot),
                'i' => Some(Action::SetInPoint), 'o' => Some(Action::SetOutPoint), 'v' => Some(Action::ToggleLoop),
                'r' => Some(Action::CycleReviewSpeed), 'f' => Some(Action::ToggleFullscreen), _ => None },
            _ => None,
        }
    }
    if auto_repeat { return None }
    if mods.shift && !mods.ctrl && !mods.alt { if letter('i') { return Some(Action::ClearInOut) } if letter('r') { return Some(Action::ResetReviewSpeed) } }
    if mods.ctrl && !mods.shift && !mods.alt { if letter('t') { return Some(Action::ScreenshotToClipboard) } if letter('z') { return Some(Action::AnnotationUndo) } }
    if mods.ctrl && mods.shift && !mods.alt && letter('z') { return Some(Action::AnnotationRedo) }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn the_table() {
        let p = Mods::default();
        assert_eq!(map_key(Key::Char('j'), p, true, false), Some(Action::ShuttleBegin(-1)));
        assert_eq!(map_key(Key::Char('L'), p, false, false), Some(Action::ShuttleEnd));
        assert_eq!(map_key(Key::Char('l'), p, true, true), None);                       // repeat ignored for hold keys
        assert_eq!(map_key(Key::Right, p, true, true), Some(Action::Step(1)));           // repeat allowed for steps
        assert_eq!(map_key(Key::Space, p, true, true), None);
        assert_eq!(map_key(Key::Char('k'), p, true, false), Some(Action::TogglePlayback));
        assert_eq!(map_key(Key::Char('v'), p, true, false), Some(Action::ToggleLoop));
        assert_eq!(map_key(Key::Char('i'), Mods { shift: true, ..p }, true, false), Some(Action::ClearInOut));
        assert_eq!(map_key(Key::Char('r'), Mods { shift: true, ..p }, true, false), Some(Action::ResetReviewSpeed));
        assert_eq!(map_key(Key::Char('z'), Mods { ctrl: true, shift: true, ..p }, true, false), Some(Action::AnnotationRedo));
        assert_eq!(map_key(Key::Char('x'), p, true, false), None);
    }
}
