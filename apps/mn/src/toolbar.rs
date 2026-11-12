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
    /// the second set (rows 301, 316): shown while a video block is the current one - QCView's seven tools for drawing
    /// on the frame, the pen's colour and width, and the frame's note (add, its words, addressed, delete)
    DrawSelect, DrawPen, DrawRect, DrawOval, DrawArrow, DrawLine, DrawErase, PenColor, PenThinner, PenThicker, NoteText, NoteDone, NoteDelete,
}
/// What the band shows of the current video block's notes (None: no video block is current - the text's tools show).
#[derive(Clone, Copy, PartialEq)]
pub struct VideoShown { pub tool: Option<Tool>, pub pen: peniko::Color, /// the video stands on a note's frame; that note is addressed
    pub on_note: bool, pub addressed: bool }
impl Tool {
    /// Built: the button does something today.
    fn built(self) -> bool { !matches!(self, Tool::File | Tool::Chip) }
    /// (icon, a label when there is no icon, the command's name, its key).
    fn face(self) -> (&'static str, Option<&'static str>, &'static str, &'static str) {
        match self {
                        Tool::Paragraph => ("paragraph", None, "Paragraph", ""), Tool::Heading => ("text-h", None, "Heading (again for the next level)", ""), Tool::Bullet => ("list-bullets", None, "Bulleted list", ""), Tool::Numbered => ("list-numbers", None, "Numbered list", ""),
            Tool::Task => ("list-checks", None, "Task list", ""), Tool::Quote => ("quotes", None, "Quote", ""), Tool::CodeBlock => ("code-block", None, "Code block", ""), Tool::Outdent => ("text-outdent", None, "Outdent", "Shift+Tab"), Tool::Indent => ("text-indent", None, "Indent", "Tab"),
            // the bundled set has no bold glyph: the button carries the letter
            Tool::Bold => ("", Some("B"), "Bold", "B"), Tool::Italic => ("text-italic", None, "Italic", "I"), Tool::Underline => ("text-underline", None, "Underline", "U"), Tool::Strike => ("text-strikethrough", None, "Strikethrough", "Shift+X"),
            Tool::Code => ("code", None, "Inline code", "E"), Tool::Highlight => ("", None, "Background colour \u{2014} the key applies the last one", "Shift+H"), Tool::Color => ("", None, "Type colour", ""), Tool::Link => ("link", None, "Link", "K"), Tool::ClearFormat => ("text-t-slash", None, "Clear formatting", "\\"),
            Tool::DrawSelect => ("cursor", None, "Select a drawing (drag to move, its corners to scale, Delete to remove; again to stop)", ""), Tool::DrawPen => ("pencil-simple", None, "Draw on the frame (again to stop)", ""), Tool::DrawRect => ("rectangle", None, "Rectangle", ""), Tool::DrawOval => ("circle", None, "Oval", ""),
            Tool::DrawArrow => ("arrow-up-right", None, "Arrow", ""), Tool::DrawLine => ("line-segment", None, "Line", ""), Tool::DrawErase => ("eraser", None, "Erase a drawing", ""),
            Tool::PenColor => ("", None, "The colour of what is drawn", ""), Tool::PenThinner => ("minus", None, "Thinner line", ""), Tool::PenThicker => ("plus", None, "Thicker line", ""),
            Tool::NoteText => ("chat-text", None, "Write a note on this frame", ""), Tool::NoteDone => ("check-circle", None, "Mark the note addressed", ""), Tool::NoteDelete => ("trash", None, "Delete the note", ""),
            Tool::Table => ("table", None, "Table (Tab: the next cell; \u{2318}\u{21b5}: a row below)", ""), Tool::Columns => ("columns", None, "Columns \u{2014} a column beside this block (again: one more)", ""), Tool::Divider => ("minus", None, "Divider", ""), Tool::Image => ("image", None, "Picture", ""), Tool::File => ("paperclip", None, "File", ""), Tool::Chip => ("tag", None, "Choice", ""),
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
pub struct Shown<'a> { pub state: &'a mn_edit::State, pub has_note: bool, /// the caret is in a table's cell
    pub in_cell: bool, pub video: Option<VideoShown> }

pub struct Toolbar { /// the set that is not shown now (the text's while a video block is current, else the video's)
    other: (Vec<ActionGroup>, Vec<Vec<Tool>>, usize), video: Option<VideoShown>, groups: Vec<ActionGroup>, tools: Vec<Vec<Tool>>, /// the groups from here on sit at the right end when everything fits on one row
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
        let make = |sets: &Vec<Vec<Tool>>| -> Vec<ActionGroup> { sets.iter().map(|set| ActionGroup::new(set.iter().map(|tool| { let (icon, label, _, _) = tool.face(); let mut b = ActionButton::new(icon, &tool.tooltip()).enabled(tool.built()); b.label = label.map(|l| l.to_string()); b }).collect()).compact()).collect() };
        let groups = make(&sets);
        // a video's set, as one reads it (the user 2026-11-12): what goes ON the frame together - the drawing tools and the
        // note's words, as the last of them; then how a line looks - its colour as a swatch (not a pen: it is no tool) and
        // its width; and, apart at the right end, what is done WITH the note - addressed, delete
        let video: Vec<Vec<Tool>> = vec![vec![DrawSelect, DrawPen, DrawLine, DrawArrow, DrawRect, DrawOval, DrawErase, NoteText], vec![PenColor, PenThinner, PenThicker], vec![NoteDone, NoteDelete]];
        Toolbar { other: (make(&video), video.clone(), 2), video: None, groups, right_from: sets.len(), tools: sets, rect: Rect::ZERO, tooltips: Tooltips::default(), hidden: false, scale: 1.0, chips: [None, None] }
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
    /// True: the set was swapped - it must be laid out before it is shown (the caller draws again).
    pub fn sync(&mut self, s: &Shown) -> bool {
        let swapped = s.video.is_some() != self.video.is_some();
        // the set follows what is current: a video block's tools, else the text's (the caller lays the band out again)
        if s.video.is_some() != self.video.is_some() { std::mem::swap(&mut self.groups, &mut self.other.0); std::mem::swap(&mut self.tools, &mut self.other.1); std::mem::swap(&mut self.right_from, &mut self.other.2); }
        self.video = s.video;
        if let Some(v) = s.video { for (g, set) in self.groups.iter_mut().zip(&self.tools) { for (b, tool) in g.buttons.iter_mut().zip(set) {
            b.enabled = match tool { Tool::NoteDone | Tool::NoteDelete => v.on_note, _ => true };
            b.selected = Some(*tool) == v.tool || (*tool == Tool::NoteDone && v.on_note && v.addressed); } }
            return swapped }
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
                // a link: never in a code block
                Tool::Link => (s.has_note && !matches!(k, Some(Kind::Code { .. })), mark(MarkKind::Link)),
                // a cell holds no table and no columns
                Tool::Table | Tool::Columns => (s.has_note && !s.in_cell, false),
                _ => (false, false),
            };
            b.enabled = enabled && tool.built(); b.selected = selected && b.enabled;
        } }
        swapped
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
        // the two colours of the text are SWATCHES, as the drawing colour is (the user 2026-11-12): Type is a disc of the
        // type's colour with the letter A on it (the default: the text's own colour); Background is a disc of the colour
        // behind the text - an empty ring when there is none
        if self.video.is_none() {
            let (text, page, edge) = (paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), paint::color(t::BACKGROUND_BASE_COLOR), paint::color(t::DIVIDER_COLOR));
            let lit = self.groups.iter().zip(&self.tools).any(|(g, set)| set.iter().zip(&g.buttons).any(|(tool, b)| *tool == Tool::Color && b.enabled));
            let dim = |c: peniko::Color| if lit { c } else { c.multiply_alpha(0.35) };
            let r = self.rect_of(Tool::Color); if r.width() > 1.0 { let fill = self.chips[0].unwrap_or(text); let disc = kurbo::Circle::new(r.center(), 9.0 * self.scale);
                scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, dim(fill), None, &disc);
                // the letter is dark on a light colour and light on a dark one
                let x = fill.to_rgba8(); let light = (x.r as f32 * 0.299 + x.g as f32 * 0.587 + x.b as f32 * 0.114) > 150.0;
                let a = fonts.layout("A", min_ui::Family::Sans, (11.0 * self.scale) as f32, 700.0, dim(if light { peniko::Color::BLACK } else { peniko::Color::WHITE }));
                min_ui::fonts::draw_layout(scene, &a, kurbo::Affine::translate((r.center().x - a.width() as f64 / 2.0, r.center().y - a.height() as f64 / 2.0))); }
            let r = self.rect_of(Tool::Highlight); if r.width() > 1.0 { let disc = kurbo::Circle::new(r.center(), 9.0 * self.scale);
                match self.chips[1] { Some(c) => scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, dim(c), None, &disc), None => scene.fill(peniko::Fill::NonZero, kurbo::Affine::IDENTITY, dim(page), None, &disc) }
                scene.stroke(&kurbo::Stroke::new(1.5 * self.scale), kurbo::Affine::IDENTITY, dim(if self.chips[1].is_some() { edge } else { text.multiply_alpha(0.7) }), None, &disc); }
        }
    }
    pub fn paint_tooltip(&self, scene: &mut Scene, fonts: &mut Fonts, bounds: Rect, now: Instant) { self.tooltips.paint(scene, fonts, self.scale, bounds, now); }
}
