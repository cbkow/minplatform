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
    Paragraph, Heading, Bullet, Numbered, Task, Quote, CodeBlock, Outdent, Indent,
    Bold, Italic, Underline, Strike, Code, /// the two colour chips: Type (the text's colour) and Background (behind it)
    Color, Highlight, Link, ClearFormat,
    Table, Columns, Divider, Image, File, Chip,
}
impl Tool {
    /// Built: the button does something today.
    fn built(self) -> bool { !matches!(self, Tool::Link | Tool::Table | Tool::Columns | Tool::File | Tool::Chip) }
    /// (icon, a label when there is no icon, the command's name, its key).
    fn face(self) -> (&'static str, Option<&'static str>, &'static str, &'static str) {
        match self {
                        Tool::Paragraph => ("paragraph", None, "Paragraph", ""), Tool::Heading => ("text-h", None, "Heading (again for the next level)", ""), Tool::Bullet => ("list-bullets", None, "Bulleted list", ""), Tool::Numbered => ("list-numbers", None, "Numbered list", ""),
            Tool::Task => ("list-checks", None, "Task list", ""), Tool::Quote => ("quotes", None, "Quote", ""), Tool::CodeBlock => ("code-block", None, "Code block", ""), Tool::Outdent => ("text-outdent", None, "Outdent", "Shift+Tab"), Tool::Indent => ("text-indent", None, "Indent", "Tab"),
            // the bundled set has no bold glyph: the button carries the letter
            Tool::Bold => ("", Some("B"), "Bold", "B"), Tool::Italic => ("text-italic", None, "Italic", "I"), Tool::Underline => ("text-underline", None, "Underline", "U"), Tool::Strike => ("text-strikethrough", None, "Strikethrough", "Shift+X"),
            Tool::Code => ("code", None, "Inline code", "E"), Tool::Highlight => ("highlighter", None, "Background colour \u{2014} the key applies the last one", "Shift+H"), Tool::Color => ("text-a-underline", None, "Type colour", ""), Tool::Link => ("link", None, "Link", "K"), Tool::ClearFormat => ("text-t-slash", None, "Clear formatting", "\\"),
            Tool::Table => ("table", None, "Table", ""), Tool::Columns => ("columns", None, "Columns", ""), Tool::Divider => ("minus", None, "Divider", ""), Tool::Image => ("image", None, "Picture", ""), Tool::File => ("paperclip", None, "File", ""), Tool::Chip => ("tag", None, "Choice", ""),
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
pub struct Shown<'a> { pub state: &'a mn_edit::State, pub has_note: bool }

pub struct Toolbar { groups: Vec<ActionGroup>, tools: Vec<Vec<Tool>>, /// the groups from here on sit at the right end when everything fits on one row
    right_from: usize, pub rect: Rect, tooltips: Tooltips, pub hidden: bool, scale: f64, /// the colours in effect at the caret for the two chips (Type, Background)
    chips: [Option<peniko::Color>; 2] }

impl Toolbar {
    pub fn new() -> Toolbar {
        use Tool::*;
        let sets: Vec<Vec<Tool>> = vec![
            vec![Paragraph, Heading, Bullet, Numbered, Task, Quote, CodeBlock], vec![Outdent, Indent],
            vec![Bold, Italic, Underline, Strike, Code, Color, Highlight, Link, ClearFormat],
            vec![Table, Columns, Divider, Image, File, Chip],
        ];
        let groups = sets.iter().map(|set| ActionGroup::new(set.iter().map(|tool| { let (icon, label, _, _) = tool.face(); let mut b = ActionButton::new(icon, &tool.tooltip()).enabled(tool.built()); b.label = label.map(|l| l.to_string()); b }).collect()).compact()).collect();
        Toolbar { groups, right_from: sets.len(), tools: sets, rect: Rect::ZERO, tooltips: Tooltips::default(), hidden: false, scale: 1.0, chips: [None, None] }
    }
    /// Places the band from `x0` to `x1` at `y`; returns its height (0 when hidden). The groups flow left to right and
    /// wrap; when they all fit on one row the last groups move to the right end.
    pub fn layout(&mut self, fonts: &mut Fonts, x0: f64, x1: f64, y: f64, scale: f64) -> f64 {
        self.scale = scale;
        if self.hidden { self.rect = Rect::new(x0, y, x1, y); for g in &mut self.groups { for b in &mut g.buttons { b.rect = Rect::ZERO; } g.rect = Rect::ZERO; } return 0.0 }
        let (row, pad, gap) = (t::TRANSPORT_BAR_HEIGHT as f64 * scale, t::SPACING_100 as f64 * scale, t::SPACING_200 as f64 * scale);
        let widths: Vec<f64> = self.groups.iter_mut().map(|g| g.layout(fonts, 0.0, 0.0, scale)).collect();
        let button_h = self.groups.first().map(|g| g.rect.height()).unwrap_or(row);
        let total: f64 = widths.iter().sum::<f64>() + gap * (widths.len().saturating_sub(1)) as f64;
        let one_row = total <= x1 - x0 - 2.0 * pad;
        let (mut cx, mut cy) = (x0 + pad, y);
        let right_w: f64 = widths[self.right_from..].iter().sum::<f64>() + gap * (widths.len() - self.right_from).saturating_sub(1) as f64;
        for (i, g) in self.groups.iter_mut().enumerate() {
            if one_row && i == self.right_from { cx = x1 - pad - right_w; }
            if !one_row && cx + widths[i] > x1 - pad && cx > x0 + pad { cx = x0 + pad; cy += row; }
            g.layout(fonts, cx, cy + (row - button_h) / 2.0, scale);
            cx += widths[i] + gap;
        }
        self.rect = Rect::new(x0, y, x1, cy + row);
        self.rect.height()
    }
    /// Lights and enables the buttons from where the caret is and what the collection's state is.
    pub fn sync(&mut self, s: &Shown) {
        self.chips = [s.state.color.as_deref().and_then(mn_layout::colour), s.state.highlight.as_deref().and_then(mn_layout::colour)];
        for (g, set) in self.groups.iter_mut().zip(&self.tools) { for (b, tool) in g.buttons.iter_mut().zip(set) {
            let k = s.state.kind.as_ref(); let mark = |m: MarkKind| s.state.marks.contains(&m);
            let (enabled, selected) = match tool {
                Tool::Paragraph => (s.has_note, matches!(k, Some(Kind::Paragraph))),
                Tool::Heading => { b.icon = match k { Some(Kind::Heading { level: 1 }) => "text-h-one", Some(Kind::Heading { level: 2 }) => "text-h-two", Some(Kind::Heading { level: 3 }) => "text-h-three", _ => "text-h" }; (s.has_note, matches!(k, Some(Kind::Heading { .. }))) }
                Tool::Bullet => (s.has_note, matches!(k, Some(Kind::Bullet { .. }))), Tool::Numbered => (s.has_note, matches!(k, Some(Kind::Numbered { .. }))), Tool::Task => (s.has_note, matches!(k, Some(Kind::Task { .. }))),
                Tool::Quote => (s.has_note, matches!(k, Some(Kind::Quote))), Tool::CodeBlock => (s.has_note, matches!(k, Some(Kind::Code { .. }))),
                Tool::Outdent | Tool::Indent => (s.has_note && matches!(k, Some(Kind::Bullet { .. } | Kind::Numbered { .. } | Kind::Task { .. })), false),
                Tool::Bold => (s.has_note, mark(MarkKind::Bold)), Tool::Italic => (s.has_note, mark(MarkKind::Italic)), Tool::Underline => (s.has_note, mark(MarkKind::Underline)), Tool::Strike => (s.has_note, mark(MarkKind::Strike)),
                Tool::Code => (s.has_note, mark(MarkKind::Code)), Tool::Color | Tool::Highlight => (s.has_note, false),
                Tool::ClearFormat | Tool::Divider | Tool::Image => (s.has_note, false),
                _ => (false, false),
            };
            b.enabled = enabled && tool.built(); b.selected = selected && b.enabled;
        } }
    }
    /// Where a tool's button is (the anchor of what it opens).
    pub fn rect_of(&self, tool: Tool) -> Rect { self.groups.iter().zip(&self.tools).find_map(|(g, set)| set.iter().position(|x| *x == tool).map(|i| g.buttons[i].rect)).unwrap_or(Rect::ZERO) }
    pub fn hit(&self, x: f64, y: f64) -> bool { !self.hidden && self.rect.contains(kurbo::Point::new(x, y)) }
    /// A pointer event: the tools pressed, and whether the event was the band's.
    pub fn event(&mut self, e: &Event, now: Instant) -> (Vec<Tool>, bool) {
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
        if self.hidden { return }
        paint::rect(scene, self.rect, t::BAND_BACKGROUND_COLOR);
        paint::hline(scene, self.rect.x0, self.rect.x1, self.rect.y1 - t::DIVIDER_THICKNESS as f64 * self.scale, t::DIVIDER_THICKNESS as f64 * self.scale, t::DIVIDER_COLOR);
        for g in &self.groups { g.paint(scene, fonts, self.scale, now); }
        // a chip carries its role's colour as a bar under the glyph (none set: a quiet line)
        for (tool, c) in [(Tool::Color, self.chips[0]), (Tool::Highlight, self.chips[1])] {
            let r = self.rect_of(tool); if r.width() < 1.0 { continue }
            let (w, h) = (14.0 * self.scale, 3.0 * self.scale);
            let bar = kurbo::RoundedRect::new(r.center().x - w / 2.0, r.y1 - h - 2.0 * self.scale, r.center().x + w / 2.0, r.y1 - 2.0 * self.scale, h / 2.0);
            scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, c.unwrap_or(paint::color(t::DIVIDER_COLOR)), None, &bar);
        }
    }
    pub fn paint_tooltip(&self, scene: &mut Scene, fonts: &mut Fonts, bounds: Rect, now: Instant) { self.tooltips.paint(scene, fonts, self.scale, bounds, now); }
}
