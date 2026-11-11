//! The toolbar (canon row 297): a band of icon buttons in the manner of QCView's transport row — `min_ui` action
//! buttons in groups, the theme's band colour and sizes, Phosphor icons, a tooltip naming each command and its key.
//! It holds the WHOLE set minNotes is to have; what is not built yet is there, disabled, so the band's shape does not
//! change as features land. A button is lit when what it stands for is on at the caret (bold, a list, an armed mark).
//! When the band is narrower than its buttons, the groups WRAP onto further rows.
use std::time::Instant;
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::{action_button::{ActionButton, Response}, action_group::ActionGroup, paint, tooltip::Tooltips, Event, Fonts};
use mn_doc::{Kind, MarkKind};
use vello::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Undo, Redo,
    Paragraph, Heading, Bullet, Numbered, Task, Quote, CodeBlock, Outdent, Indent,
    Bold, Italic, Underline, Strike, Code, Highlight, Color, Link, ClearFormat,
    Table, Columns, Divider, Image, File, Chip,
    Find, Export, FullWidth, Save,
    /// not a button: the band's own caret was pressed (the page must be laid out again)
    Fold,
}
impl Tool {
    /// Built: the button does something today.
    fn built(self) -> bool { !matches!(self, Tool::Color | Tool::Link | Tool::Table | Tool::Columns | Tool::Image | Tool::File | Tool::Chip | Tool::Find | Tool::Export | Tool::FullWidth) }
    /// (icon, a label when there is no icon, the command's name, its key).
    fn face(self) -> (&'static str, Option<&'static str>, &'static str, &'static str) {
        match self {
            Tool::Undo => ("arrow-counter-clockwise", None, "Undo", "Z"), Tool::Redo => ("arrow-clockwise", None, "Redo", "Shift+Z"),
            Tool::Paragraph => ("paragraph", None, "Paragraph", ""), Tool::Heading => ("text-h", None, "Heading (again for the next level)", ""), Tool::Bullet => ("list-bullets", None, "Bulleted list", ""), Tool::Numbered => ("list-numbers", None, "Numbered list", ""),
            Tool::Task => ("list-checks", None, "Task list", ""), Tool::Quote => ("quotes", None, "Quote", ""), Tool::CodeBlock => ("code-block", None, "Code block", ""), Tool::Outdent => ("text-outdent", None, "Outdent", "Shift+Tab"), Tool::Indent => ("text-indent", None, "Indent", "Tab"),
            // the bundled set has no bold glyph: the button carries the letter
            Tool::Bold => ("", Some("B"), "Bold", "B"), Tool::Italic => ("text-italic", None, "Italic", "I"), Tool::Underline => ("text-underline", None, "Underline", "U"), Tool::Strike => ("text-strikethrough", None, "Strikethrough", "Shift+X"),
            Tool::Code => ("code", None, "Inline code", "E"), Tool::Highlight => ("highlighter", None, "Highlight", "Shift+H"), Tool::Color => ("palette", None, "Text colour", ""), Tool::Link => ("link", None, "Link", "K"), Tool::ClearFormat => ("text-t-slash", None, "Clear formatting", "\\"),
            Tool::Table => ("table", None, "Table", ""), Tool::Columns => ("columns", None, "Columns", ""), Tool::Divider => ("minus", None, "Divider", ""), Tool::Image => ("image", None, "Picture or video", ""), Tool::File => ("paperclip", None, "File", ""), Tool::Chip => ("tag", None, "Choice", ""),
            Tool::Find => ("magnifying-glass", None, "Find", "F"), Tool::Export => ("export", None, "Export", ""), Tool::FullWidth => ("arrows-out-line-horizontal", None, "Full width", ""), Tool::Save => ("floppy-disk", None, "Save", "S"), Tool::Fold => ("", None, "", ""),
        }
    }
    fn tooltip(self) -> String {
        let (_, _, name, key) = self.face();
        let soon = if self.built() { "" } else { " — not yet" };
        if key.is_empty() { return format!("{name}{soon}") }
        let plain = matches!(key, "Tab" | "Shift+Tab");
        let k = if plain { key.to_string() } else if cfg!(target_os = "macos") { format!("{}\u{2318}{}", if key.starts_with("Shift+") { "\u{21e7}" } else { "" }, key.trim_start_matches("Shift+")) } else { format!("Ctrl+{key}") };
        format!("{name} ({k}){soon}")
    }
}

/// What the band needs to know to light and enable its buttons.
pub struct Shown<'a> { pub state: &'a mn_edit::State, pub has_note: bool, pub has_file: bool, pub unpublished: bool, pub saving: bool }

pub struct Toolbar { groups: Vec<ActionGroup>, tools: Vec<Vec<Tool>>, /// the groups from here on sit at the right end when everything fits on one row
    right_from: usize, pub rect: Rect, tooltips: Tooltips, pub hidden: bool, scale: f64,
    /// the floating caret under the band's right end that folds it away and brings it back (the rails' tab, turned)
    tab: Rect, tab_hover: bool, present: bool }

impl Toolbar {
    pub fn new() -> Toolbar {
        use Tool::*;
        let sets: Vec<Vec<Tool>> = vec![
            vec![Undo, Redo],
            vec![Paragraph, Heading, Bullet, Numbered, Task, Quote, CodeBlock], vec![Outdent, Indent],
            vec![Bold, Italic, Underline, Strike, Code, Highlight, Color, Link, ClearFormat],
            vec![Table, Columns, Divider, Image, File, Chip],
            vec![Find, Export, FullWidth], vec![Save],
        ];
        let groups = sets.iter().map(|set| ActionGroup::new(set.iter().map(|tool| { let (icon, label, _, _) = tool.face(); let mut b = ActionButton::new(icon, &tool.tooltip()).enabled(tool.built()); b.label = label.map(|l| l.to_string()); b }).collect()).compact()).collect();
        Toolbar { groups, right_from: sets.len() - 2, tools: sets, rect: Rect::ZERO, tooltips: Tooltips::default(), hidden: false, scale: 1.0, tab: Rect::ZERO, tab_hover: false, present: false }
    }
    /// Places the band from `x0` to `x1` at `y`; returns its height (0 when hidden). The groups flow left to right and
    /// wrap; when they all fit on one row the last groups move to the right end.
    pub fn layout(&mut self, fonts: &mut Fonts, x0: f64, x1: f64, y: f64, scale: f64) -> f64 {
        self.scale = scale;
        let (sz, m) = (t::COMPONENT_HEIGHT_75 as f64 * scale, t::SPACING_75 as f64 * scale);
        let place_tab = |me: &mut Toolbar, bottom: f64| me.tab = Rect::new(x1 - 4.0 * m - sz, bottom + m, x1 - 4.0 * m, bottom + m + sz);
        if self.hidden { place_tab(self, y); self.rect = Rect::new(x0, y, x1, y); for g in &mut self.groups { for b in &mut g.buttons { b.rect = Rect::ZERO; } g.rect = Rect::ZERO; } return 0.0 }
        let (row, pad, gap) = (t::TRANSPORT_BAR_HEIGHT as f64 * scale, t::SPACING_100 as f64 * scale, t::SPACING_200 as f64 * scale);
        let widths: Vec<f64> = self.groups.iter_mut().map(|g| g.layout(fonts, 0.0, 0.0, scale)).collect();
        let button_h = self.groups.first().map(|g| g.rect.height()).unwrap_or(row);
        let total: f64 = widths.iter().sum::<f64>() + gap * (widths.len().saturating_sub(1)) as f64;
        let one_row = total <= x1 - x0 - 2.0 * pad;
        let (mut cx, mut cy) = (x0 + pad, y);
        let right_w: f64 = widths[self.right_from..].iter().sum::<f64>() + gap * (widths.len() - self.right_from - 1) as f64;
        for (i, g) in self.groups.iter_mut().enumerate() {
            if one_row && i == self.right_from { cx = x1 - pad - right_w; }
            if !one_row && cx + widths[i] > x1 - pad && cx > x0 + pad { cx = x0 + pad; cy += row; }
            g.layout(fonts, cx, cy + (row - button_h) / 2.0, scale);
            cx += widths[i] + gap;
        }
        self.rect = Rect::new(x0, y, x1, cy + row); place_tab(self, cy + row);
        self.rect.height()
    }
    /// Lights and enables the buttons from where the caret is and what the collection's state is.
    pub fn sync(&mut self, s: &Shown) {
        for (g, set) in self.groups.iter_mut().zip(&self.tools) { for (b, tool) in g.buttons.iter_mut().zip(set) {
            let k = s.state.kind.as_ref(); let mark = |m: MarkKind| s.state.marks.contains(&m);
            let (enabled, selected) = match tool {
                Tool::Undo => (s.has_note && s.state.can_undo, false), Tool::Redo => (s.has_note && s.state.can_redo, false),
                Tool::Paragraph => (s.has_note, matches!(k, Some(Kind::Paragraph))),
                Tool::Heading => { b.icon = match k { Some(Kind::Heading { level: 1 }) => "text-h-one", Some(Kind::Heading { level: 2 }) => "text-h-two", Some(Kind::Heading { level: 3 }) => "text-h-three", _ => "text-h" }; (s.has_note, matches!(k, Some(Kind::Heading { .. }))) }
                Tool::Bullet => (s.has_note, matches!(k, Some(Kind::Bullet { .. }))), Tool::Numbered => (s.has_note, matches!(k, Some(Kind::Numbered { .. }))), Tool::Task => (s.has_note, matches!(k, Some(Kind::Task { .. }))),
                Tool::Quote => (s.has_note, matches!(k, Some(Kind::Quote))), Tool::CodeBlock => (s.has_note, matches!(k, Some(Kind::Code { .. }))),
                Tool::Outdent | Tool::Indent => (s.has_note && matches!(k, Some(Kind::Bullet { .. } | Kind::Numbered { .. } | Kind::Task { .. })), false),
                Tool::Bold => (s.has_note, mark(MarkKind::Bold)), Tool::Italic => (s.has_note, mark(MarkKind::Italic)), Tool::Underline => (s.has_note, mark(MarkKind::Underline)), Tool::Strike => (s.has_note, mark(MarkKind::Strike)),
                Tool::Code => (s.has_note, mark(MarkKind::Code)), Tool::Highlight => (s.has_note, mark(MarkKind::Highlight)),
                Tool::ClearFormat | Tool::Divider => (s.has_note, false),
                // lit while something is not in the document yet
                Tool::Save => (s.has_file && !s.saving, s.unpublished),
                _ => (false, false),
            };
            b.enabled = enabled && tool.built(); b.selected = selected && b.enabled;
        } }
    }
    pub fn hit(&self, x: f64, y: f64) -> bool { !self.hidden && self.rect.contains(kurbo::Point::new(x, y)) }
    /// A pointer event: the tools pressed, and whether the event was the band's.
    pub fn event(&mut self, e: &Event, now: Instant) -> (Vec<Tool>, bool) {
        self.present = !matches!(e, Event::Leave);
        let on_tab = e.pos().is_some_and(|(x, y)| self.tab.contains(kurbo::Point::new(x, y)));
        self.tab_hover = on_tab;
        if on_tab { if matches!(e, Event::Down { .. }) { self.hidden = !self.hidden; return (vec![Tool::Fold], true) } return (vec![], true) }
        if self.hidden { return (vec![], false) }
        let mut out = vec![];
        for (g, set) in self.groups.iter_mut().zip(&self.tools) { for (i, r) in g.event(e, now) { if r == Response::Clicked { out.push(set[i]); } } }
        if matches!(e, Event::Down { .. }) { self.tooltips.pressed(now); }
        let over = self.groups.iter().enumerate().flat_map(|(gi, g)| g.buttons.iter().enumerate().map(move |(bi, b)| (gi, bi, b))).find(|(_, _, b)| b.hovered()).map(|(gi, bi, b)| ((gi * 100 + bi) as u64, b.rect, b.tooltip.clone()));
        self.tooltips.hover(over.as_ref().map(|(k, r, s)| (*k, *r, s.as_str())), now);
        (out, e.pos().is_some_and(|(x, y)| self.hit(x, y)))
    }
    pub fn tick(&mut self, now: Instant) -> bool { self.tooltips.tick(now) || self.groups.iter().any(|g| g.animating(now)) }
    pub fn next_change(&self, now: Instant) -> Option<Instant> { self.tooltips.next_change(now) }
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, now: Instant) {
        if self.present {
            if self.tab_hover { paint::rounded(scene, self.tab, t::CORNER_RADIUS_SMALL_DEFAULT as f64 * self.scale, t::HOVER_BACKGROUND_COLOR); }
            let c = if self.tab_hover { t::NEUTRAL_CONTENT_COLOR_HOVER } else { t::RAIL_OPEN_TAB_COLOR };
            if let Some(g) = fonts.icon(if self.hidden { "caret-down" } else { "caret-up" }, t::ICON_SIZE_SMALL * self.scale as f32, paint::color(c)) { min_ui::fonts::draw_layout(scene, &g, kurbo::Affine::translate((self.tab.x0 + (self.tab.width() - g.width() as f64) / 2.0, self.tab.y0 + (self.tab.height() - g.height() as f64) / 2.0))); }
        }
        if self.hidden { return }
        paint::rect(scene, self.rect, t::BAND_BACKGROUND_COLOR);
        paint::hline(scene, self.rect.x0, self.rect.x1, self.rect.y1 - t::DIVIDER_THICKNESS as f64 * self.scale, t::DIVIDER_THICKNESS as f64 * self.scale, t::DIVIDER_COLOR);
        for g in &self.groups { g.paint(scene, fonts, self.scale, now); }
    }
    pub fn paint_tooltip(&self, scene: &mut Scene, fonts: &mut Fonts, bounds: Rect, now: Instant) { self.tooltips.paint(scene, fonts, self.scale, bounds, now); }
}
