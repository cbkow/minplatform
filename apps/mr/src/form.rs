//! The New Job card (the spec's 4): a template, a name, the range, and one field for each value the template lets
//! the operator give. The same card edits a job (4.4) and submits a chunk's frames as a job of their own: there it
//! works on the job's stored manifest, and the template only says which values are shown and what they are called. Made of the family's fields and buttons, over the window with a scrim, as its other cards are.
use crate::node::Node;
use kurbo::{Affine, Rect, RoundedRect};
use min_theme::tokens as t;
use min_ui::{fonts::{draw_layout, Fonts}, paint, Button, Event, Family, FieldResponse, Key, KeyEvent, Named, TextButton, TextField, Variant};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use vello::Scene;

struct FlagRow { index: usize, label: String, required: bool, /// (false: shown, not changed)
    editable: bool, file: bool, filter: String, help: String, field: TextField, browse: TextButton }
#[derive(Clone, Copy, PartialEq)]
pub enum Kind { New, Edit, Resubmit }
pub struct Form {
    kind: Kind, /// the job edited, or the one a chunk is taken from; its manifest in this system's paths; the template's line and whether it is gone
    job: String, manifest: Value, template_line: (String, bool), /// Start, End, Chunk as the card opened (an edit that changes them can only start over)
    range0: [String; 3], timeout0: String, apply: [TextButton; 3],
    templates: Vec<Value>, chosen: Option<usize>, template_rects: Vec<Rect>, name: TextField, /// Start, End, Chunk, Priority
    nums: Vec<TextField>, flags: Vec<FlagRow>, focus: usize, cancel: TextButton, submit: TextButton, errors: Vec<String>, busy: bool, card: Rect,
    /// the answer of a submission under way: the new job's id, or why not
    pub result: Arc<Mutex<Option<Result<String, Vec<String>>>>>, /// a file chosen in the system's panel, for a flag's field
    picked: Arc<Mutex<Option<(usize, String)>>>,
}
pub enum Out { None, Close, /// "restart" or "startover": the window asks first, then calls `apply`
    Confirm(&'static str) }
fn field(text: &str, placeholder: &str, fonts: &mut Fonts) -> TextField { let mut f = TextField::new(Family::Sans, t::BODY_SIZE_S); f.placeholder = placeholder.into(); f.set_text_end(text, fonts); f }

impl Form {
    pub fn open(node: &Node, fonts: &mut Fonts) -> Form {
        let templates = crate::submit::form_templates(node.farm());
        let mut f = Form { kind: Kind::New, job: String::new(), manifest: Value::Null, template_line: (String::new(), false), range0: Default::default(), timeout0: String::new(), apply: Form::apply_buttons(), templates, chosen: None, template_rects: Vec::new(), name: field("", "e.g. shot_010_v003", fonts), nums: vec![field("1", "", fonts), field("250", "", fonts), field("1", "", fonts), field("50", "", fonts)], flags: Vec::new(), focus: 0,
            cancel: TextButton::new("Cancel", Variant::Secondary), submit: TextButton::new("Submit", Variant::Primary), errors: Vec::new(), busy: false, card: Rect::ZERO, result: Arc::new(Mutex::new(None)), picked: Arc::new(Mutex::new(None)) };
        // (one template: it is the one - 0.5.x made the operator pick it from a list of one)
        if f.templates.len() == 1 { f.choose(0, fonts); } f.set_focus(0); f
    }
    fn apply_buttons() -> [TextButton; 3] { [TextButton::new("Apply to next chunks", Variant::Primary), TextButton::new("Stop chunks & apply", Variant::Secondary), TextButton::new("Start over", Variant::Negative)] }
    /// The card for a job that exists: to edit it, or (`range`) to submit those frames of it as a job of their own.
    /// `manifest` is the job's, in this system's paths.
    pub fn open_job(node: &Node, fonts: &mut Fonts, kind: Kind, manifest: Value, priority: i64, range: Option<(i64, i64)>) -> Form {
        let job = manifest["job_id"].as_str().unwrap_or("").to_string(); let tid = manifest["template_id"].as_str().unwrap_or("").to_string();
        let tpl = crate::submit::templates(node.farm()).into_iter().find(|t| t["template_id"].as_str() == Some(tid.as_str()));
        let template_line = match &tpl { Some(t) => (t["name"].as_str().unwrap_or("?").to_string(), false), None => (format!("{tid} (template missing \u{2014} editing raw values)"), true) };
        // the stored values, each under the template's entry for it: matched in order by the flag's text
        let none = Vec::new(); let tflags = tpl.as_ref().and_then(|t| t["flags"].as_array()).unwrap_or(&none); let mut cursor = 0; let mut flags = Vec::new();
        for (index, mf) in manifest["flags"].as_array().unwrap_or(&none).iter().enumerate() { let text = mf["flag"].as_str().unwrap_or(""); let value = mf["value"].as_str();
            let browse = TextButton::new("Browse", Variant::Secondary).size_s();
            if tpl.is_none() { if let Some(v) = value { flags.push(FlagRow { index, label: if text.is_empty() { "(positional)".into() } else { text.to_string() }, required: false, editable: true, file: false, filter: String::new(), help: String::new(), field: field(v, "", fonts), browse }); } continue }
            let Some(p) = (cursor..tflags.len()).find(|p| tflags[*p]["flag"].as_str().unwrap_or("") == text) else { continue }; cursor = p + 1; let f = &tflags[p]; let s = |k: &str| f[k].as_str().unwrap_or("").to_string();
            let (editable, required) = (f["editable"] == Value::Bool(true), f["required"] == Value::Bool(true)); if !(editable || required) { continue }
            flags.push(FlagRow { index, label: if !s("info").is_empty() { s("info") } else if !text.is_empty() { text.to_string() } else { "(positional)".to_string() }, required, editable, file: editable && s("type") == "file", filter: s("filter"), help: s("help"), field: field(value.unwrap_or(""), "", fonts), browse }); }
        let n = |k: &str, d: i64| manifest[k].as_i64().unwrap_or(d); let (a, b) = range.unwrap_or((n("frame_start", 1), n("frame_end", 1)));
        let chunk = if range.is_some() { b - a + 1 } else { n("chunk_size", 1) }; let range0 = [a.to_string(), b.to_string(), chunk.to_string()]; let timeout0 = n("timeout_seconds", 0).to_string();
        let mut nums = vec![field(&range0[0], "", fonts), field(&range0[1], "", fonts), field(&range0[2], "", fonts), field(&priority.to_string(), "", fonts)];
        if kind == Kind::Edit { nums.push(field(&n("max_retries", 3).to_string(), "", fonts)); nums.push(field(&timeout0, "", fonts)); }
        let name = if kind == Kind::Resubmit { format!("{job}_f{a}-{b}") } else { job.clone() };
        let mut f = Form { kind, job, manifest, template_line, range0, timeout0, apply: Form::apply_buttons(), templates: Vec::new(), chosen: None, template_rects: Vec::new(), name: field(&name, "e.g. shot_010_v003", fonts), nums, flags, focus: 0,
            cancel: TextButton::new("Cancel", Variant::Secondary), submit: TextButton::new("Submit as new job", Variant::Primary), errors: Vec::new(), busy: false, card: Rect::ZERO, result: Arc::new(Mutex::new(None)), picked: Arc::new(Mutex::new(None)) };
        f.set_focus(0); f
    }
    /// (scripted checks) An edit sets the priority to 77 and the retries to 5 and applies to the next chunks; the other card is sent as it stands.
    pub fn demo_job(&mut self, fonts: &mut Fonts, node: &Arc<Node>) { if self.kind == Kind::Edit { self.nums[3].set_text_end("77", fonts); self.nums[4].set_text_end("5", fonts); } self.send(node); }
    /// Whether Start, End or Chunk were changed in an edit: then only "Start over" can apply it.
    fn range_changed(&self) -> bool { self.kind == Kind::Edit && (0..3).any(|i| self.nums[i].text().trim() != self.range0[i]) }
    /// The job's manifest with what the card holds laid over it, or why not.
    fn edited(&self) -> Result<(Value, i64), String> {
        let n: Vec<i64> = self.nums.iter().map(|f| f.text().trim().parse::<i64>().unwrap_or(i64::MIN)).collect(); if n.iter().any(|x| *x == i64::MIN) { return Err("Every number field must hold a whole number".into()) }
        if self.kind == Kind::Resubmit { if n[0] > n[1] || n[2] < 1 { return Err("Invalid frame range or chunk size".into()) } } else { if n[0] > n[1] { return Err("Frame start must be <= frame end".into()) } if n[2] < 1 { return Err("Chunk size must be at least 1".into()) } }
        if let Some(r) = self.flags.iter().find(|r| r.required && r.editable && r.field.text().trim().is_empty()) { return Err(format!("Required flag is empty: {}", r.label)) }
        let mut m = self.manifest.clone(); let mut out = None;
        if let Some(flags) = m["flags"].as_array_mut() { for r in self.flags.iter().filter(|r| r.editable) { if let Some(f) = flags.get_mut(r.index) { f["value"] = Value::String(r.field.text().trim().to_string()); } }
            out = flags.iter().find(|f| f["is_output"] == Value::Bool(true)).and_then(|f| f["value"].as_str()).filter(|v| !v.is_empty()).map(crate::submit::parent); }
        if let Some(o) = out { m["output_dir"] = Value::String(o); }
        m["frame_start"] = n[0].into(); m["frame_end"] = n[1].into(); m["chunk_size"] = n[2].into();
        if self.kind == Kind::Edit { m["max_retries"] = n[4].clamp(0, 99).into(); if self.nums[5].text().trim() != self.timeout0 { m["timeout_seconds"] = if n[5] <= 0 { Value::Null } else { n[5].into() }; } }
        Ok((m, n[3]))
    }
    /// An edit is sent in one of 0.5.x's three ways: "continue", "restart", "startover".
    pub fn apply(&mut self, mode: &'static str, node: &Arc<Node>) {
        if self.busy || self.kind != Kind::Edit { return } if mode != "startover" && self.range_changed() { return }
        match self.edited() { Ok((m, priority)) => { self.busy = true; self.errors.clear(); node.apply_edit(m, mode, priority, self.result.clone()); } Err(why) => self.errors = vec![why] }
    }
    /// A template is chosen: its defaults fill the range, and its flags that may be given become fields.
    fn choose(&mut self, i: usize, fonts: &mut Fonts) {
        self.chosen = Some(i); let tpl = &self.templates[i]; let d = &tpl["job_defaults"];
        for (n, (k, dflt)) in [("frame_start", 1), ("frame_end", 250), ("chunk_size", 1), ("priority", 50)].iter().enumerate() { self.nums[n].set_text_end(&d[*k].as_i64().unwrap_or(*dflt).to_string(), fonts); }
        self.flags = tpl["flags"].as_array().map(|a| a.iter().enumerate().filter(|(_, f)| f["editable"] == Value::Bool(true) || f["required"] == Value::Bool(true)).map(|(index, f)| { let s = |k: &str| f[k].as_str().unwrap_or("").to_string();
            let label = if !s("info").is_empty() { s("info") } else if !s("flag").is_empty() { s("flag") } else { "(positional)".to_string() };
            FlagRow { index, label, required: f["required"] == Value::Bool(true), editable: true, file: s("type") == "file", filter: s("filter"), help: s("help"), field: field(&s("value"), "", fonts), browse: TextButton::new("Browse", Variant::Secondary).size_s() } }).collect()).unwrap_or_default();
    }
    /// The fields that take typing, in Tab's order (an edit's name and a value the template locks are shown, not typed in).
    fn fields(&mut self) -> Vec<&mut TextField> { let mut v: Vec<&mut TextField> = if self.kind == Kind::Edit { Vec::new() } else { vec![&mut self.name] }; v.extend(self.nums.iter_mut()); v.extend(self.flags.iter_mut().filter(|f| f.editable).map(|f| &mut f.field)); v }
    fn set_focus(&mut self, to: usize) { self.focus = to; for (i, f) in self.fields().into_iter().enumerate() { f.focused = i == to; } }
    fn send(&mut self, node: &Arc<Node>) {
        if self.kind == Kind::Edit { self.apply("continue", node); return }
        if self.kind == Kind::Resubmit { if self.busy { return } match self.edited() { Ok((m, priority)) => { self.busy = true; self.errors.clear(); node.submit_manifest(m, self.name.text().trim().to_string(), priority, self.result.clone()); } Err(why) => self.errors = vec![why] } return }
        let Some(i) = self.chosen else { self.errors = vec!["Choose a template".into()]; return }; if self.busy { return }
        let tpl = self.templates[i].clone(); let n = |f: &TextField| f.text().trim().parse::<i64>().unwrap_or(i64::MIN);
        let nums: Vec<i64> = self.nums.iter().map(n).collect(); if nums.iter().any(|x| *x == i64::MIN) { self.errors = vec!["Start, End, Chunk and Priority must be whole numbers".into()]; return }
        // one value for each of the template's flags: what was typed for those shown, the template's own for the rest
        let values: Vec<String> = tpl["flags"].as_array().map(|a| a.iter().enumerate().map(|(k, f)| self.flags.iter().find(|r| r.index == k).map(|r| r.field.text().trim().to_string()).unwrap_or_else(|| f["value"].as_str().unwrap_or("").to_string())).collect()).unwrap_or_default();
        self.busy = true; self.errors.clear(); node.submit_form(tpl, values, self.name.text().trim().to_string(), nums[0], nums[1], nums[2], nums[3], self.result.clone());
    }
    /// (scripted looks) The template whose name holds `word` is chosen, the name and each field filled, and the form sent - as a hand would.
    pub fn demo(&mut self, word: &str, fonts: &mut Fonts, node: &Arc<Node>) {
        if let Some(i) = self.templates.iter().position(|t| t["name"].as_str().is_some_and(|n| n.contains(word))) { self.choose(i, fonts); }
        self.name.set_text_end("Demo Shot 010 v003", fonts); for r in self.flags.iter_mut() { let v = if r.file { "/Volumes/projects/jobs/demo/scenes/shot_010.blend" } else { "/Volumes/projects/jobs/demo/renders/shot_010/shot_010_####" }; r.field.set_text_end(v, fonts); }
        self.nums[1].set_text_end("48", fonts); self.send(node);
    }
    /// A submission's answer, when it has come: the new job's id (the card then closes).
    pub fn answered(&mut self) -> Option<String> { let r = self.result.lock().unwrap().take()?; self.busy = false; match r { Ok(id) => Some(id), Err(why) => { self.errors = why; None } } }
    pub fn pointer(&mut self, e: &Event, fonts: &mut Fonts, node: &Arc<Node>) -> Out {
        let now = Instant::now();
        if self.cancel.event(e, now).0 { return Out::Close }
        if self.kind == Kind::Edit { let hit: Vec<bool> = self.apply.iter_mut().map(|b| b.event(e, now).0).collect(); if hit[0] { self.apply("continue", node); return Out::None } if hit[1] { return Out::Confirm("restart") } if hit[2] { return Out::Confirm("startover") } }
        else if self.submit.event(e, now).0 { self.send(node); return Out::None }
        let mut browse = None; for r in self.flags.iter_mut() { if r.file && r.browse.event(e, now).0 { browse = Some((r.index, r.filter.clone(), r.field.text().to_string())); } }
        if let Some((index, filter, at)) = browse { let picked = self.picked.clone();
            // the system's panel, from a thread of its own (the window's loop goes on under it)
            std::thread::spawn(move || { let mut d = rfd::FileDialog::new(); if !filter.is_empty() { d = d.add_filter(&filter, &[filter.as_str()]); } d = d.add_filter("All files", &["*"]); if let Some(dir) = std::path::Path::new(&at).parent().filter(|p| p.is_dir()) { d = d.set_directory(dir); }
                if let Some(p) = d.pick_file() { *picked.lock().unwrap() = Some((index, p.to_string_lossy().into_owned())); } }); return Out::None }
        if let Event::Down { x, y, button: Button::Primary } = *e {
            if let Some(i) = self.template_rects.iter().position(|r| r.contains(kurbo::Point::new(x, y))) { if self.chosen != Some(i) { self.choose(i, fonts); } return Out::None }
            let hit = self.fields().iter().position(|f| f.hit(x, y)); if let Some(i) = hit { self.set_focus(i); }
        }
        let focus = self.focus; if let Some(f) = self.fields().into_iter().nth(focus) { f.pointer(e, fonts); }
        Out::None
    }
    /// A key: Tab goes from field to field, Enter submits; the rest is the field's. (As in 0.5.x, Escape does not close the card.)
    pub fn key(&mut self, k: &KeyEvent, pasted: Option<String>, fonts: &mut Fonts, node: &Arc<Node>) {
        let n = self.fields().len();
        match &k.key { Key::Named(Named::Tab) => { let to = if k.shift { (self.focus + n - 1) % n } else { (self.focus + 1) % n }; self.set_focus(to); if let Some(f) = self.fields().into_iter().nth(to) { f.select_all(fonts); } }
            Key::Named(Named::Enter) => self.send(node), Key::Named(Named::Escape) => {}
            _ => { let focus = self.focus; if let Some(f) = self.fields().into_iter().nth(focus) { match pasted { Some(text) => { f.paste(text.lines().next().unwrap_or(""), fonts); } None => { let _: FieldResponse = f.key(k, fonts); } } } } }
    }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, view: Rect, s: f64, now: Instant) {
        if let Some((index, path)) = self.picked.lock().unwrap().take() { if let Some(r) = self.flags.iter_mut().find(|r| r.index == index) { r.field.set_text_end(&path, fonts); } }
        let c = paint::color; let pad = t::DIALOG_PADDING as f64 * s; let (ink, sub) = (c(t::NEUTRAL_CONTENT_COLOR_DEFAULT), c(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
        let row = 38.0 * s; let fh = 30.0 * s; let help_rows = self.flags.iter().filter(|f| !f.help.is_empty()).count() as f64;
        let edit = self.kind == Kind::Edit; let changed = self.range_changed();
        let body_h = 30.0 * s + (self.templates.len().max(1) as f64) * 30.0 * s + 14.0 * s + row + row + 26.0 * s + if edit { row + 26.0 * s } else { 0.0 } + if changed { 24.0 * s } else { 0.0 } + if self.flags.is_empty() { 0.0 } else { 26.0 * s + self.flags.len() as f64 * row + help_rows * 18.0 * s } + self.errors.len() as f64 * 20.0 * s + if self.errors.is_empty() { 0.0 } else { 20.0 * s };
        let (w, h) = ((640.0 * s).min(view.width() - 48.0 * s), (pad * 2.0 + 36.0 * s + body_h + 56.0 * s).min(view.height() - 48.0 * s));
        let (x0, y0) = (view.x0 + (view.width() - w) / 2.0, view.y0 + (view.height() - h) / 2.0); self.card = Rect::new(x0, y0, x0 + w, y0 + h);
        paint::rect(scene, view, t::DIALOG_SCRIM_COLOR); let rad = t::DIALOG_CORNER_RADIUS as f64 * s;
        for sh in t::POPOVER_SHADOW { scene.draw_blurred_rounded_rect(Affine::translate((sh.x as f64 * s, sh.y as f64 * s)), self.card.inflate(sh.spread as f64 * s, sh.spread as f64 * s), c(sh.color), rad, (sh.blur as f64 * s / 2.0).max(0.1)); }
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, c(t::BACKGROUND_LAYER_1_COLOR), None, &RoundedRect::from_rect(self.card, rad));
        let line = |scene: &mut Scene, fonts: &mut Fonts, text: &str, x: f64, cy: f64, size: f32, bold: bool, color: peniko::Color, room: f64| { let l = fonts.layout_fit(text, Family::Sans, size * s as f32, if bold { t::FONT_WEIGHT_BOLD } else { t::FONT_WEIGHT_REGULAR } as f32, color, room.max(8.0)); draw_layout(scene, &l, Affine::translate((x, cy - l.height() as f64 / 2.0))); };
        let (lx, x1) = (x0 + pad, x0 + w - pad); let fx = lx + 148.0 * s; let mut y = y0 + pad;
        let title = match self.kind { Kind::New => "New Job".to_string(), Kind::Edit => format!("Edit Job \u{2014} {}", self.job), Kind::Resubmit => format!("Resubmit chunk as new job \u{2014} {}", self.job) };
        line(scene, fonts, &title, lx, y + 10.0 * s, t::DIALOG_TITLE_SIZE, true, ink, w - 2.0 * pad); y += 36.0 * s;
        // the template: one row for each; the chosen one is filled
        line(scene, fonts, "Template", lx, y + 10.0 * s, t::DETAIL_SIZE_M, true, sub, 140.0 * s); y += 26.0 * s; self.template_rects.clear();
        if self.kind != Kind::New { line(scene, fonts, &self.template_line.0, lx, y + 12.0 * s, t::BODY_SIZE_S, false, if self.template_line.1 { c(t::NOTICE_VISUAL_COLOR) } else { ink }, w - 2.0 * pad); y += 30.0 * s; }
        else if self.templates.is_empty() { line(scene, fonts, "No template on the share", lx, y + 12.0 * s, t::BODY_SIZE_S, false, c(t::NOTICE_VISUAL_COLOR), w - 2.0 * pad); y += 30.0 * s; }
        for (i, tpl) in self.templates.iter().enumerate() { let r = Rect::new(lx, y, x1, y + 26.0 * s); self.template_rects.push(r); let on = self.chosen == Some(i);
            paint::rounded(scene, r, t::CORNER_RADIUS_500 as f64 * s, if on { t::SELECTION_BACKGROUND_COLOR } else { t::BACKGROUND_LAYER_2_COLOR }); if on { paint::rect(scene, Rect::new(r.x0, r.y0 + 4.0 * s, r.x0 + 3.0 * s, r.y1 - 4.0 * s), t::ACCENT_VISUAL_COLOR); }
            line(scene, fonts, tpl["name"].as_str().unwrap_or("?"), r.x0 + 12.0 * s, r.center().y, t::BODY_SIZE_S, false, ink, r.width() - 24.0 * s); y += 30.0 * s; }
        y += 14.0 * s;
        line(scene, fonts, "Job name", lx, y + fh / 2.0, t::DETAIL_SIZE_M, false, ink, 140.0 * s); if edit { line(scene, fonts, &self.job, fx, y + fh / 2.0, t::BODY_SIZE_S, false, sub, x1 - fx); } else { self.name.layout(fonts, Rect::new(fx, y, x1, y + fh), s); self.name.paint(scene, fonts, s); } y += row;
        // the range: four small fields in a row
        let nw = (x1 - fx - 3.0 * 12.0 * s) / 4.0;
        for (i, label) in ["Start", "End", "Chunk", "Priority"].iter().enumerate() { let x = fx + i as f64 * (nw + 12.0 * s); line(scene, fonts, label, x, y + 8.0 * s, t::DETAIL_SIZE_S, false, sub, nw); self.nums[i].layout(fonts, Rect::new(x, y + 18.0 * s, x + nw, y + 18.0 * s + fh), s); self.nums[i].paint(scene, fonts, s); }
        line(scene, fonts, "Range", lx, y + 18.0 * s + fh / 2.0, t::DETAIL_SIZE_M, false, ink, 140.0 * s); y += row + 26.0 * s;
        if edit { for (i, label) in ["Max retries", "Timeout (s, 0 = none)"].iter().enumerate() { let x = fx + i as f64 * 2.0 * (nw + 12.0 * s); let fw = 2.0 * nw + 12.0 * s; line(scene, fonts, label, x, y + 8.0 * s, t::DETAIL_SIZE_S, false, sub, fw); self.nums[4 + i].layout(fonts, Rect::new(x, y + 18.0 * s, x + fw, y + 18.0 * s + fh), s); self.nums[4 + i].paint(scene, fonts, s); }
            line(scene, fonts, "Job settings", lx, y + 18.0 * s + fh / 2.0, t::DETAIL_SIZE_M, false, ink, 140.0 * s); y += row + 26.0 * s; }
        if !self.flags.is_empty() { line(scene, fonts, "Flags", lx, y + 10.0 * s, t::DETAIL_SIZE_M, true, sub, 140.0 * s); y += 26.0 * s; }
        for r in self.flags.iter_mut() {
            line(scene, fonts, &r.label, lx, y + fh / 2.0, t::DETAIL_SIZE_M, false, if r.required { c(t::NOTICE_VISUAL_COLOR) } else { ink }, 140.0 * s);
            let bw = if r.file { r.browse.measure(fonts, s).0 + 8.0 * s } else { 0.0 };
            if r.editable { r.field.layout(fonts, Rect::new(fx, y, x1 - bw, y + fh), s); r.field.paint(scene, fonts, s); } else { line(scene, fonts, r.field.text(), fx, y + fh / 2.0, t::BODY_SIZE_S, false, sub, x1 - fx); }
            if r.file { r.browse.rect = Rect::new(x1 - bw + 8.0 * s, y + 3.0 * s, x1, y + fh - 3.0 * s); r.browse.paint(scene, fonts, s, now); }
            y += row; if !r.help.is_empty() { line(scene, fonts, &r.help, fx, y - 4.0 * s, t::DETAIL_SIZE_S, false, sub, x1 - fx); y += 18.0 * s; }
        }
        // every reason a submission was refused, just over the buttons (0.5.x showed the first alone, at the foot of the scroll)
        for e in &self.errors { line(scene, fonts, e, lx, y + 10.0 * s, t::DETAIL_SIZE_M, false, c(t::NEGATIVE_VISUAL_COLOR), w - 2.0 * pad); y += 20.0 * s; }
        if changed { line(scene, fonts, "Frame range / chunk size changed \u{2014} only Start over can apply this.", lx, y + 10.0 * s, t::DETAIL_SIZE_M, false, c(t::NOTICE_VISUAL_COLOR), w - 2.0 * pad); }
        let bh = t::BUTTON_HEIGHT_M as f64 * s; let by = y0 + h - pad - bh;
        if edit { let mut x = x1; for i in (0..3).rev() { self.apply[i].enabled = !self.busy && (i == 2 || !changed); let bw = self.apply[i].measure(fonts, s).0; self.apply[i].rect = Rect::new(x - bw, by, x, by + bh); x -= bw + 8.0 * s; }
            let wc = self.cancel.measure(fonts, s).0; self.cancel.rect = Rect::new(x - wc, by, x, by + bh); self.cancel.paint(scene, fonts, s, now); for b in self.apply.iter_mut() { b.paint(scene, fonts, s, now); } return }
        let word = if self.kind == Kind::Resubmit { "Submit as new job" } else { "Submit" };
        self.submit.label = if self.busy { "Submitting\u{2026}".into() } else { word.into() }; self.submit.enabled = !self.busy && (self.chosen.is_some() || self.kind == Kind::Resubmit) && !self.name.text().trim().is_empty();
        let (ws, _) = self.submit.measure(fonts, s); let (wc, _) = self.cancel.measure(fonts, s);
        self.submit.rect = Rect::new(x1 - ws, by, x1, by + bh); self.cancel.rect = Rect::new(self.submit.rect.x0 - 8.0 * s - wc, by, self.submit.rect.x0 - 8.0 * s, by + bh);
        self.cancel.paint(scene, fonts, s, now); self.submit.paint(scene, fonts, s, now);
    }
    pub fn animating(&self, now: Instant) -> bool { self.busy || self.cancel.animating(now) || self.submit.animating(now) || self.apply.iter().any(|b| b.animating(now)) }
}
