//! MenuBar — QCView's in-window menu bar (the Qt Quick `MenuBar` Main.qml draws on Windows, with `ThemedMenu` popups;
//! canon row 194). One strip of titles across the top of the window in the recessed tone with a hairline below; a title
//! opens its menu below it; rows are the context menu's rows (`menu::paint_row`) plus a check gutter and submenu carets;
//! a submenu cascades to the right (to the left when there is no room). Always dark: nothing reads the OS colour scheme.
//!
//! Pointer (Windows' and Qt's rules): a press on a title opens its menu (a press on the open title closes it); while a
//! menu is open, hovering another title switches to it; hovering a submenu row opens the submenu; a release over an
//! enabled row chooses it (so press-on-title, drag, release works too); any press outside the bar and the popups closes
//! everything and is CONSUMED (QCView's click-eater while a menu is open).
//!
//! Keyboard (Windows' rules; the shell translates): Alt tapped alone or F10 focuses the first title with the mnemonics
//! shown; Alt+letter opens the title with that mnemonic; Left / Right walk the titles (Right opens a submenu row, Left
//! closes a submenu); Up / Down walk the rows, skipping separators and disabled rows; Enter / Space choose; a letter
//! chooses the row with that mnemonic; Escape steps back one level (submenu → menu → focused title → nothing).
//! Mnemonics come from `&` in the labels ("E&xit"; "&&" is a literal ampersand) and are underlined only while the
//! keyboard drives the bar, as Windows shows its keyboard cues.
use crate::{input::{Button, Event, Named}, menu::{paint_row, paint_separator, row_width, underline, Row}, paint, Family, Fonts, Popover};
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use vello::Scene;

#[derive(Clone, Debug, PartialEq)]
pub enum BarItem {
    /// `id` is the owner's: it comes back from `event` / `key` when the row is chosen.
    Action { id: u32, label: String, enabled: bool, shortcut: String, checked: Option<bool> },
    Sub { label: String, enabled: bool, items: Vec<BarItem> },
    Separator,
}
impl BarItem {
    pub fn action(id: u32, label: &str, enabled: bool, shortcut: &str) -> BarItem { BarItem::Action { id, label: label.into(), enabled, shortcut: shortcut.into(), checked: None } }
    fn raw_label(&self) -> &str { match self { BarItem::Action { label, .. } | BarItem::Sub { label, .. } => label, BarItem::Separator => "" } }
    fn enabled(&self) -> bool { match self { BarItem::Action { enabled, .. } | BarItem::Sub { enabled, .. } => *enabled, BarItem::Separator => false } }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BarMenu { pub title: String, pub items: Vec<BarItem> }

/// A key as the bar sees it (the shell maps its toolkit's events; Alt-with-Ctrl combinations are the shell's, not ours).
#[derive(Clone, Debug, PartialEq)]
pub enum BarKey { AltTap, F10, Alt(char), Named(Named), Space, Char(char), /// any other key: swallowed while the bar holds the keyboard
    Other }

/// "E&xit" → ("Exit", the mnemonic's char index 1, 'x'); "&&" is a literal '&'.
pub fn mnemonic(label: &str) -> (String, Option<(usize, char)>) {
    let (mut out, mut m, mut it, mut n) = (String::new(), None, label.chars().peekable(), 0usize);
    while let Some(c) = it.next() {
        if c == '&' { match it.peek().copied() { Some('&') => { it.next(); out.push('&'); n += 1; } Some(k) if m.is_none() => { m = Some((n, k.to_ascii_lowercase())); } _ => {} } continue }
        out.push(c); n += 1;
    }
    (out, m)
}

struct Level { items: Vec<BarItem>, pop: Popover, hover: Option<usize>, gutter: bool, scale: f64 }
impl Level {
    fn row_h(&self, i: usize) -> f64 { if matches!(self.items[i], BarItem::Separator) { t::SPACING_100 as f64 } else { t::MENU_ITEM_HEIGHT as f64 } }
    fn item_rect(&self, i: usize) -> Rect {
        let c = self.pop.content; let s = self.scale;
        let y0 = c.y0 + (0..i).map(|k| self.row_h(k)).sum::<f64>() * s;
        Rect::new(c.x0, y0, c.x1, y0 + self.row_h(i) * s)
    }
    fn item_at(&self, x: f64, y: f64) -> Option<usize> {
        if !self.pop.content.contains(kurbo::Point::new(x, y)) { return None }
        (0..self.items.len()).find(|&i| self.item_rect(i).contains(kurbo::Point::new(x, y)))
    }
    fn step(&self, from: Option<usize>, down: bool) -> Option<usize> {
        let n = self.items.len(); if n == 0 { return None }
        let mut i = from.unwrap_or(if down { n - 1 } else { 0 });
        for _ in 0..n { i = if down { (i + 1) % n } else { (i + n - 1) % n }; if self.items[i].enabled() { return Some(i) } }
        None
    }
}

pub struct MenuBar {
    pub menus: Vec<BarMenu>,
    /// The strip (set by `layout`); zero-height when the shell hides it.
    pub rect: Rect,
    titles: Vec<Rect>,
    open: Option<usize>,
    levels: Vec<Level>,
    hover_title: Option<usize>,
    /// Keyboard focus on a title with no menu open (Alt / F10).
    focused: Option<usize>,
    /// The keyboard drives the bar: mnemonics underlined.
    cues: bool,
    scale: f64,
    bounds: Rect,
}

impl MenuBar {
    pub fn new(menus: Vec<BarMenu>) -> MenuBar { MenuBar { menus, rect: Rect::ZERO, titles: vec![], open: None, levels: vec![], hover_title: None, focused: None, cues: false, scale: 1.0, bounds: Rect::ZERO } }
    /// Replace the menus (a Recent list or a checkmark changed). An open menu is rebuilt in place, its row kept.
    pub fn set_menus(&mut self, menus: Vec<BarMenu>, fonts: &mut Fonts) {
        if menus == self.menus { return }
        self.menus = menus;
        if let Some(i) = self.open { let hover = self.levels.first().and_then(|l| l.hover); self.open_menu(i, fonts); if let Some(l) = self.levels.first_mut() { l.hover = hover.filter(|&h| h < l.items.len()); } }
    }
    /// The bar is taking input: a menu is open or a title holds the keyboard focus.
    pub fn is_active(&self) -> bool { self.open.is_some() || self.focused.is_some() }
    pub fn is_open(&self) -> bool { self.open.is_some() }
    /// Close everything (focus lost, a resize, fullscreen).
    pub fn cancel(&mut self) -> bool { let was = self.is_active(); self.open = None; self.levels.clear(); self.focused = None; self.cues = false; was }

    /// Lay the titles out along the strip `[x0, x1] × [y0, y0 + menu-bar-height]`; `bounds` = the window (popups clamp to it).
    pub fn layout(&mut self, fonts: &mut Fonts, x0: f64, y0: f64, x1: f64, bounds: Rect, scale: f64) {
        self.scale = scale; self.bounds = bounds;
        self.rect = Rect::new(x0, y0, x1, y0 + t::MENU_BAR_HEIGHT as f64 * scale);
        let pad = t::MENU_BAR_TITLE_EDGE_TO_TEXT as f64 * scale;
        let mut x = x0 + t::SPACING_75 as f64 * scale;
        self.titles = self.menus.iter().map(|m| {
            let w = fonts.layout(&mnemonic(&m.title).0, Family::Sans, t::BODY_SIZE_M * scale as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE).width() as f64 + 2.0 * pad;
            let r = Rect::new(x, self.rect.y0, x + w, self.rect.y1); x += w; r
        }).collect();
    }

    fn title_at(&self, x: f64, y: f64) -> Option<usize> { self.titles.iter().position(|r| r.contains(kurbo::Point::new(x, y))) }

    fn make_level(&self, items: Vec<BarItem>, fonts: &mut Fonts) -> (Level, f64, f64) {
        let gutter = items.iter().any(|i| matches!(i, BarItem::Action { checked: Some(_), .. }));
        let sub_col = items.iter().any(|i| matches!(i, BarItem::Sub { .. }));   // every row keeps the caret column, so shortcuts align
        let w = items.iter().map(|it| match it {
            BarItem::Action { label, shortcut, .. } => row_width(fonts, &mnemonic(label).0, shortcut, gutter, sub_col),
            BarItem::Sub { label, .. } => row_width(fonts, &mnemonic(label).0, "", gutter, true),
            BarItem::Separator => 0.0 }).fold(t::MENU_MIN_WIDTH as f64, f64::max);
        let h: f64 = items.iter().map(|i| if matches!(i, BarItem::Separator) { t::SPACING_100 as f64 } else { t::MENU_ITEM_HEIGHT as f64 }).sum();
        (Level { items, pop: Popover::new(""), hover: None, gutter, scale: self.scale }, w, h)
    }

    /// Place a popup of content `w × h` (logical) with its frame's top-left at (x, y), clamped into the window.
    fn place(&self, l: &mut Level, x: f64, y: f64, w: f64, h: f64) {
        let s = self.scale; let pad = t::POPOVER_PADDING as f64 * s;
        let (fw, fh) = (w * s + 2.0 * pad, h * s + 2.0 * pad);
        let b = self.bounds;
        let x = x.min(b.x1 - fw).max(b.x0); let y = y.min(b.y1 - fh).max(b.y0);
        l.pop.rect = Rect::new(x, y, x + fw, y + fh);
        l.pop.content = Rect::new(x + pad, y + pad, x + fw - pad, y + fh - pad);
        l.pop.open = true;
    }

    fn open_menu(&mut self, i: usize, fonts: &mut Fonts) {
        self.open = Some(i); self.focused = None; self.levels.clear();
        let (mut l, w, h) = self.make_level(self.menus[i].items.clone(), fonts);
        let tr = self.titles.get(i).copied().unwrap_or(self.rect);
        self.place(&mut l, tr.x0, self.rect.y1, w, h);
        self.levels.push(l);
    }

    /// Open the submenu of row `row` at level `depth` (closing anything deeper).
    fn open_sub(&mut self, depth: usize, row: usize, fonts: &mut Fonts) {
        self.levels.truncate(depth + 1);
        let BarItem::Sub { items, enabled: true, .. } = self.levels[depth].items[row].clone() else { return };
        let (mut l, w, h) = self.make_level(items, fonts);
        let parent = self.levels[depth].pop.rect; let r = self.levels[depth].item_rect(row);
        let s = self.scale; let pad = t::POPOVER_PADDING as f64 * s; let ov = t::MENU_SUBMENU_OVERLAP as f64 * s;
        let fw = w * s + 2.0 * pad;
        let x = if parent.x1 - ov + fw <= self.bounds.x1 { parent.x1 - ov } else { parent.x0 + ov - fw };
        self.place(&mut l, x, r.y0 - pad, w, h);
        self.levels.push(l);
    }

    fn sub_open_for(&self, depth: usize) -> bool { self.levels.len() > depth + 1 }

    /// Returns (the chosen row's id, redraw needed, consumed).
    pub fn event(&mut self, e: &Event, fonts: &mut Fonts) -> (Option<u32>, bool, bool) {
        let before = self.state();
        let mut chosen = None; let mut consumed = self.is_active();
        match *e {
            Event::Move { x, y } => {
                self.hover_title = self.title_at(x, y);
                if let (Some(i), Some(o)) = (self.hover_title, self.open) { if i != o { self.open_menu(i, fonts); self.cues = false; } }
                else if let Some(d) = (0..self.levels.len()).rev().find(|&d| self.levels[d].pop.hit(x, y)) {
                    let at = self.levels[d].item_at(x, y);
                    match at {
                        Some(r) if self.levels[d].items[r].enabled() => {
                            // a new row closes the deeper popups; a submenu row opens its own (the same row keeps its child)
                            let same = self.levels[d].hover == Some(r) && self.sub_open_for(d);
                            if !same {
                                self.levels[d].hover = Some(r); self.levels.truncate(d + 1);
                                if matches!(self.levels[d].items[r], BarItem::Sub { .. }) { self.open_sub(d, r, fonts); }
                            }
                        }
                        _ => { if !self.sub_open_for(d) { self.levels[d].hover = None; } }
                    }
                } else if let Some(l) = self.levels.last_mut() { l.hover = None; }
                if self.open.is_some() { consumed = true; }
            }
            Event::Down { x, y, .. } => {
                if let Some(i) = self.title_at(x, y) {
                    if self.open == Some(i) { self.cancel(); } else { self.open_menu(i, fonts); self.cues = false; }
                    consumed = true;
                } else if self.levels.iter().any(|l| l.pop.hit(x, y)) { consumed = true; }
                else if self.is_active() { self.cancel(); consumed = true; }
                else if self.rect.contains(kurbo::Point::new(x, y)) { consumed = true; }
            }
            Event::Up { x, y, button } => {
                if let Some(d) = (0..self.levels.len()).rev().find(|&d| self.levels[d].pop.hit(x, y)) {
                    if let Some(r) = self.levels[d].item_at(x, y) {
                        match &self.levels[d].items[r] {
                            BarItem::Action { id, enabled: true, .. } if matches!(button, Button::Primary | Button::Secondary) => { chosen = Some(*id); self.cancel(); }
                            BarItem::Sub { enabled: true, .. } if !self.sub_open_for(d) => self.open_sub(d, r, fonts),
                            _ => {}
                        }
                    }
                    consumed = true;
                } else if self.rect.contains(kurbo::Point::new(x, y)) { consumed = true; }
            }
            Event::Leave => { self.hover_title = None; }
        }
        (chosen, chosen.is_some() || before != self.state(), consumed)
    }

    /// Returns (the chosen row's id, redraw needed, consumed). Inactive, only AltTap / F10 / Alt+letter act.
    pub fn key(&mut self, k: &BarKey, fonts: &mut Fonts) -> (Option<u32>, bool, bool) {
        let before = self.state(); let mut chosen = None;
        let n = self.menus.len();
        let consumed = match k {
            BarKey::AltTap | BarKey::F10 => { if self.is_active() { self.cancel(); } else if n > 0 { self.focused = Some(0); self.cues = true; } true }
            BarKey::Alt(c) => match self.title_by_mnemonic(*c) { Some(i) => { self.open_menu(i, fonts); self.cues = true; self.first_row(); true } None => self.is_active() },
            _ if !self.is_active() => false,
            BarKey::Named(Named::Escape) => {
                if self.levels.len() > 1 { self.levels.pop(); } else if let Some(o) = self.open { self.levels.clear(); self.open = None; self.focused = Some(o); } else { self.focused = None; self.cues = false; }
                true
            }
            BarKey::Named(Named::Left) => { if self.levels.len() > 1 { self.levels.pop(); } else { self.walk_titles(false, fonts); } true }
            BarKey::Named(Named::Right) => {
                let d = self.levels.len().wrapping_sub(1);
                let sub = self.levels.last().and_then(|l| l.hover.map(|h| (h, matches!(l.items[h], BarItem::Sub { enabled: true, .. }))));
                if let Some((h, true)) = sub { self.open_sub(d, h, fonts); self.first_row(); } else { self.walk_titles(true, fonts); }
                true
            }
            BarKey::Named(dir @ (Named::Up | Named::Down)) => {
                let down = *dir == Named::Down;
                if let Some(f) = self.focused { self.open_menu(f, fonts); if let Some(l) = self.levels.last_mut() { l.hover = l.step(None, down); } }
                else if let Some(l) = self.levels.last_mut() { l.hover = l.step(l.hover, down); }
                true
            }
            BarKey::Named(Named::Enter) | BarKey::Space => {
                if let Some(f) = self.focused { self.open_menu(f, fonts); self.first_row(); } else { chosen = self.activate_hover(fonts); }
                true
            }
            BarKey::Char(c) => {
                if self.focused.is_some() && self.open.is_none() { if let Some(i) = self.title_by_mnemonic(*c) { self.open_menu(i, fonts); self.first_row(); } }
                else if let Some(l) = self.levels.last_mut() {
                    if let Some(r) = (0..l.items.len()).find(|&r| l.items[r].enabled() && mnemonic(l.items[r].raw_label()).1.map(|m| m.1) == Some(c.to_ascii_lowercase())) { l.hover = Some(r); chosen = self.activate_hover(fonts); }
                }
                true
            }
            BarKey::Named(_) | BarKey::Other => true,
        };
        if self.is_active() { self.cues = self.cues || matches!(k, BarKey::AltTap | BarKey::F10 | BarKey::Alt(_)); }
        (chosen, chosen.is_some() || before != self.state(), consumed)
    }

    fn first_row(&mut self) { if let Some(l) = self.levels.last_mut() { l.hover = l.step(None, true); } }
    fn title_by_mnemonic(&self, c: char) -> Option<usize> { let c = c.to_ascii_lowercase(); self.menus.iter().position(|m| mnemonic(&m.title).1.map(|m| m.1) == Some(c)) }
    fn walk_titles(&mut self, right: bool, fonts: &mut Fonts) {
        let n = self.menus.len(); if n == 0 { return }
        let step = |i: usize| if right { (i + 1) % n } else { (i + n - 1) % n };
        if let Some(o) = self.open { self.open_menu(step(o), fonts); self.first_row(); } else if let Some(f) = self.focused { self.focused = Some(step(f)); }
    }
    fn activate_hover(&mut self, fonts: &mut Fonts) -> Option<u32> {
        let d = self.levels.len().checked_sub(1)?; let h = self.levels[d].hover?;
        match self.levels[d].items[h].clone() {
            BarItem::Action { id, enabled: true, .. } => { self.cancel(); Some(id) }
            BarItem::Sub { enabled: true, .. } => { self.open_sub(d, h, fonts); self.first_row(); None }
            _ => None,
        }
    }

    fn state(&self) -> (Option<usize>, Option<usize>, Option<usize>, bool, Vec<Option<usize>>) { (self.open, self.hover_title, self.focused, self.cues, self.levels.iter().map(|l| l.hover).collect()) }

    /// The strip: the recessed tone, the hairline, the titles (the open / hovered / focused one on a grey fill).
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64) {
        if self.rect.height() <= 0.0 { return }
        paint::rect(scene, self.rect, t::MENU_BAR_BACKGROUND_COLOR);
        let hair = scale.max(1.0).floor(); paint::rect(scene, Rect::new(self.rect.x0, self.rect.y1 - hair, self.rect.x1, self.rect.y1), t::MENU_BAR_DIVIDER_COLOR);
        let pad = t::MENU_BAR_TITLE_EDGE_TO_TEXT as f64 * scale; let size = t::BODY_SIZE_M * scale as f32;
        let inset = t::SPACING_50 as f64 * scale;
        for (i, (m, r)) in self.menus.iter().zip(&self.titles).enumerate() {
            let lit = self.open == Some(i) || self.focused == Some(i) || (self.hover_title == Some(i) && (self.open.is_none() || self.open == Some(i)));
            if lit { paint::rounded(scene, Rect::new(r.x0, r.y0 + inset, r.x1, r.y1 - inset - hair), t::PICKER_CORNER_RADIUS as f64 * scale, t::MENU_BAR_TITLE_FILL_OPEN); }
            let (text, mn) = mnemonic(&m.title);
            let c = if lit { t::MENU_BAR_TITLE_COLOR_OPEN } else { t::MENU_BAR_TITLE_COLOR };
            let l = fonts.layout(&text, Family::Sans, size, t::FONT_WEIGHT_REGULAR as f32, paint::color(c));
            let ty = r.y0 + (r.height() - hair - l.height() as f64) / 2.0;
            crate::fonts::draw_layout(scene, &l, Affine::translate((r.x0 + pad, ty)));
            if self.cues { if let Some((u, _)) = mn { underline(scene, fonts, &text, u, r.x0 + pad, ty, &l, size, true, scale); } }
        }
    }

    /// The open popups, over everything (paint last).
    pub fn paint_menus(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64) {
        for (d, l) in self.levels.iter().enumerate() {
            l.pop.paint(scene, fonts, scale);
            for (i, it) in l.items.iter().enumerate() {
                let r = l.item_rect(i);
                let open_sub = self.levels.len() > d + 1 && l.hover == Some(i);
                match it {
                    BarItem::Separator => paint_separator(scene, r, scale),
                    BarItem::Action { label, enabled, shortcut, checked, .. } => { let (text, mn) = mnemonic(label); paint_row(scene, fonts, r, &Row { label: &text, shortcut, enabled: *enabled, hover: l.hover == Some(i), down: false, checked: *checked, gutter: l.gutter, sub: false, underline: if self.cues { mn.map(|m| m.0) } else { None } }, scale) }
                    BarItem::Sub { label, enabled, .. } => { let (text, mn) = mnemonic(label); paint_row(scene, fonts, r, &Row { label: &text, shortcut: "", enabled: *enabled, hover: l.hover == Some(i) || open_sub, down: false, checked: None, gutter: l.gutter, sub: true, underline: if self.cues { mn.map(|m| m.0) } else { None } }, scale) }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bar() -> (MenuBar, Fonts) {
        let mut fonts = Fonts::new();
        let menus = vec![
            BarMenu { title: "&File".into(), items: vec![BarItem::action(1, "Open Media…", true, "Ctrl+O"), BarItem::Sub { label: "Open Recent Media".into(), enabled: true, items: vec![BarItem::action(10, "a.mov", true, ""), BarItem::Separator, BarItem::action(11, "Clear Menu", true, "")] }, BarItem::action(2, "Open Stream…", false, ""), BarItem::Separator, BarItem::action(3, "E&xit", true, "Ctrl+Q")] },
            BarMenu { title: "&View".into(), items: vec![BarItem::Action { id: 20, label: "Compact Mode".into(), enabled: true, shortcut: "Ctrl+Shift+C".into(), checked: Some(true) }, BarItem::action(21, "Default Size && Position", true, "")] },
        ];
        let mut b = MenuBar::new(menus); b.layout(&mut fonts, 0.0, 0.0, 1000.0, Rect::new(0.0, 0.0, 1000.0, 700.0), 1.0); (b, fonts)
    }
    fn mid(r: Rect) -> (f64, f64) { ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0) }

    #[test]
    fn mnemonics_parse() {
        assert_eq!(mnemonic("E&xit"), ("Exit".into(), Some((1, 'x'))));
        assert_eq!(mnemonic("Default Size && Position"), ("Default Size & Position".into(), None));
        assert_eq!(mnemonic("&File"), ("File".into(), Some((0, 'f'))));
    }

    #[test]
    fn click_opens_hover_switches_release_chooses_outside_press_closes() {
        let (mut b, mut f) = bar();
        let (x, y) = mid(b.titles[0]);
        let (_, d, c) = b.event(&Event::Down { x, y, button: Button::Primary }, &mut f); assert!(d && c && b.is_open());
        b.event(&Event::Up { x, y, button: Button::Primary }, &mut f); assert!(b.is_open(), "the release over the title keeps it open");
        assert!(b.levels[0].pop.rect.y0 >= b.rect.y1 - 0.5, "below the bar");
        let (x2, y2) = mid(b.titles[1]); b.event(&Event::Move { x: x2, y: y2 }, &mut f); assert_eq!(b.open, Some(1), "hover switches menus");
        let r = b.levels[0].item_rect(1); let (rx, ry) = mid(r);
        b.event(&Event::Move { x: rx, y: ry }, &mut f);
        let (id, _, _) = b.event(&Event::Up { x: rx, y: ry, button: Button::Primary }, &mut f); assert_eq!(id, Some(21)); assert!(!b.is_active());
        let (x, y) = mid(b.titles[0]); b.event(&Event::Down { x, y, button: Button::Primary }, &mut f);
        let (_, _, c) = b.event(&Event::Down { x: 900.0, y: 600.0, button: Button::Primary }, &mut f); assert!(c && !b.is_open(), "the press outside closes and is eaten");
        let (_, _, c) = b.event(&Event::Down { x: 900.0, y: 600.0, button: Button::Primary }, &mut f); assert!(!c, "inactive: the viewport gets its press");
    }

    #[test]
    fn submenu_cascades_on_hover_and_disabled_rows_are_inert() {
        let (mut b, mut f) = bar();
        let (x, y) = mid(b.titles[0]); b.event(&Event::Down { x, y, button: Button::Primary }, &mut f);
        let (sx, sy) = mid(b.levels[0].item_rect(1)); b.event(&Event::Move { x: sx, y: sy }, &mut f);
        assert_eq!(b.levels.len(), 2); assert!(b.levels[1].pop.rect.x0 >= b.levels[0].pop.rect.x1 - 4.0, "to the right");
        let (cx, cy) = mid(b.levels[1].item_rect(2)); b.event(&Event::Move { x: cx, y: cy }, &mut f);
        let (id, _, _) = b.event(&Event::Up { x: cx, y: cy, button: Button::Primary }, &mut f); assert_eq!(id, Some(11));
        let (x, y) = mid(b.titles[0]); b.event(&Event::Down { x, y, button: Button::Primary }, &mut f);
        let (dx, dy) = mid(b.levels[0].item_rect(2)); b.event(&Event::Move { x: dx, y: dy }, &mut f);
        let (id, _, _) = b.event(&Event::Up { x: dx, y: dy, button: Button::Primary }, &mut f); assert!(id.is_none() && b.is_open(), "a disabled row does nothing");
    }

    #[test]
    fn keyboard_alt_arrows_enter_mnemonics_escape() {
        let (mut b, mut f) = bar();
        assert!(!b.key(&BarKey::Named(Named::Down), &mut f).2, "inactive: keys pass through");
        b.key(&BarKey::AltTap, &mut f); assert_eq!(b.focused, Some(0)); assert!(b.cues);
        b.key(&BarKey::Named(Named::Right), &mut f); assert_eq!(b.focused, Some(1));
        b.key(&BarKey::Named(Named::Down), &mut f); assert_eq!(b.open, Some(1)); assert_eq!(b.levels[0].hover, Some(0));
        let (id, _, _) = b.key(&BarKey::Named(Named::Enter), &mut f); assert_eq!(id, Some(20)); assert!(!b.is_active());
        b.key(&BarKey::Alt('f'), &mut f); assert_eq!(b.open, Some(0)); assert_eq!(b.levels[0].hover, Some(0));
        b.key(&BarKey::Named(Named::Down), &mut f); assert_eq!(b.levels[0].hover, Some(1));
        b.key(&BarKey::Named(Named::Right), &mut f); assert_eq!(b.levels.len(), 2, "Right opens the submenu"); assert_eq!(b.levels[1].hover, Some(0));
        b.key(&BarKey::Named(Named::Left), &mut f); assert_eq!(b.levels.len(), 1);
        b.key(&BarKey::Named(Named::Down), &mut f); assert_eq!(b.levels[0].hover, Some(4), "skips the disabled row and the separator");
        b.key(&BarKey::Named(Named::Escape), &mut f); assert_eq!((b.open, b.focused), (None, Some(0)));
        b.key(&BarKey::Named(Named::Escape), &mut f); assert!(!b.is_active());
        b.key(&BarKey::Alt('f'), &mut f); let (id, _, _) = b.key(&BarKey::Char('x'), &mut f); assert_eq!(id, Some(3), "E&xit by its mnemonic");
    }
}
