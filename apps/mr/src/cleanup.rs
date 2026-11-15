//! Farm Cleanup (the spec's 1.11, contract 6.7): what can be cleared from the farm, in six sections, each with rows to
//! tick and one act. Scanned when it opens and after every act, off the window's thread. As in 0.5.x no act asks first.
use crate::node::Node;
use kurbo::{Affine, Rect, RoundedRect};
use min_theme::tokens as t;
use min_ui::{checkbox, fonts::{draw_layout, Fonts}, paint, Button, Event, Family, ScrollBar, TextButton, Variant};
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use vello::Scene;

/// (the scan's key, the section's title, its act's label, the act, what it says when empty)
const SECTIONS: [(&str, &str, &str, &str, &str); 6] = [("finished_jobs", "Finished jobs (completed / cancelled)", "Archive selected", "archive", "No finished jobs."), ("archived_jobs", "Archived jobs", "Delete selected", "delete_jobs", "No archived jobs."),
    ("orphaned_dirs", "Orphaned job directories (no matching record)", "Delete selected", "delete_dirs", "No orphaned job directories."), ("stale_peers", "Offline peers", "Remove selected", "remove_peers", "No offline peers."),
    ("stale_staging_dirs", "Empty staging directories", "Delete selected", "delete_dirs", "No empty staging directories."), ("failed_staging_copies", "Non-empty staging copies (failed or in-progress)", "Delete selected", "delete_dirs", "No lingering staging copies.")];
pub struct Cleanup { scan: Arc<Mutex<Option<Value>>>, data: Option<Value>, busy: bool, ticked: BTreeSet<(usize, String)>, acts: Vec<TextButton>, rescan: TextButton, close: TextButton, bar: ScrollBar, list: Rect, /// where each row and each section's tick-all box lies: (rect, section, the row's id or none for the box)
    hits: Vec<(Rect, usize, Option<String>)> }
pub enum Out { None, Close }

impl Cleanup {
    pub fn open(node: &Arc<Node>) -> Cleanup { let scan = Arc::new(Mutex::new(None)); node.cleanup_scan_for_window(scan.clone());
        Cleanup { scan, data: None, busy: true, ticked: BTreeSet::new(), acts: SECTIONS.iter().map(|x| TextButton::new(x.2, if x.3 == "archive" { Variant::Secondary } else { Variant::Negative }).size_s()).collect(), rescan: TextButton::new("Rescan", Variant::Secondary).size_s(), close: TextButton::new("Close", Variant::Secondary), bar: ScrollBar::default(), list: Rect::ZERO, hits: Vec::new() } }
    fn rows(&self, i: usize) -> Vec<(String, String, String)> { self.data.as_ref().and_then(|d| d[SECTIONS[i].0].as_array()).map(|a| a.iter().map(|x| (x["id"].as_str().unwrap_or("").to_string(), x["label"].as_str().unwrap_or("").to_string(), x["detail"].as_str().unwrap_or("").to_string())).collect()).unwrap_or_default() }
    pub fn wheel(&mut self, dy: f64) { self.bar.wheel(dy, Instant::now()); }
    pub fn pointer(&mut self, e: &Event, node: &Arc<Node>) -> Out {
        let now = Instant::now(); if self.close.event(e, now).0 { return Out::Close } if self.bar.event(e, now).1 { return Out::None }
        if !self.busy && self.rescan.event(e, now).0 { self.busy = true; node.cleanup_scan_for_window(self.scan.clone()); return Out::None }
        let mut act = None; for (i, b) in self.acts.iter_mut().enumerate() { if b.event(e, now).0 { act = Some(i); } }
        if let Some(i) = act { let ids: Vec<String> = self.ticked.iter().filter(|(s, _)| *s == i).map(|(_, id)| id.clone()).collect(); if !ids.is_empty() && !self.busy { self.busy = true; self.ticked.retain(|(s, _)| *s != i); node.cleanup_for_window(SECTIONS[i].3, ids, self.scan.clone()); } return Out::None }
        if let Event::Down { x, y, button: Button::Primary } = *e { if self.list.contains(kurbo::Point::new(x, y)) { if let Some((_, sec, id)) = self.hits.iter().find(|(r, _, _)| r.contains(kurbo::Point::new(x, y))).cloned() { match id {
            Some(id) => { if !self.ticked.remove(&(sec, id.clone())) { self.ticked.insert((sec, id)); } }
            // the section's own box: all of it, or none of it
            None => { let rows = self.rows(sec); let all = !rows.is_empty() && rows.iter().all(|r| self.ticked.contains(&(sec, r.0.clone()))); for r in rows { if all { self.ticked.remove(&(sec, r.0)); } else { self.ticked.insert((sec, r.0)); } } } } } } }
        Out::None
    }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, view: Rect, s: f64, now: Instant) {
        if let Some(v) = self.scan.lock().unwrap().take() { self.data = Some(v); self.busy = false; }
        let c = paint::color; let pad = t::DIALOG_PADDING as f64 * s; let (ink, sub) = (c(t::NEUTRAL_CONTENT_COLOR_DEFAULT), c(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
        let (w, h) = ((720.0 * s).min(view.width() - 48.0 * s), (760.0 * s).min(view.height() - 48.0 * s)); let (x0, y0) = (view.x0 + (view.width() - w) / 2.0, view.y0 + (view.height() - h) / 2.0); let card = Rect::new(x0, y0, x0 + w, y0 + h);
        paint::rect(scene, view, t::DIALOG_SCRIM_COLOR); let rad = t::DIALOG_CORNER_RADIUS as f64 * s;
        for sh in t::POPOVER_SHADOW { scene.draw_blurred_rounded_rect(Affine::translate((sh.x as f64 * s, sh.y as f64 * s)), card.inflate(sh.spread as f64 * s, sh.spread as f64 * s), c(sh.color), rad, (sh.blur as f64 * s / 2.0).max(0.1)); }
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, c(t::BACKGROUND_LAYER_1_COLOR), None, &RoundedRect::from_rect(card, rad));
        let line = |scene: &mut Scene, fonts: &mut Fonts, text: &str, x: f64, cy: f64, size: f32, bold: bool, mono: bool, color: peniko::Color, room: f64| -> f64 { let l = fonts.layout_fit(text, if mono { Family::Mono } else { Family::Sans }, size * s as f32, if bold { t::FONT_WEIGHT_BOLD } else { t::FONT_WEIGHT_REGULAR } as f32, color, room.max(8.0)); draw_layout(scene, &l, Affine::translate((x, cy - l.height() as f64 / 2.0))); l.width() as f64 };
        let (lx, x1) = (x0 + pad, x0 + w - pad); let mut y = y0 + pad;
        line(scene, fonts, "Farm Cleanup", lx, y + 10.0 * s, t::DIALOG_TITLE_SIZE, true, false, ink, w); y += 34.0 * s;
        let leads = self.data.as_ref().is_some_and(|d| d["is_leader"] == Value::Bool(true)); let leader_answered = self.data.as_ref().is_none_or(|d| d["leader_answered"] != Value::Bool(false));
        let top = if self.data.is_none() { "Scanning\u{2026}" } else if leads { "You are the leader \u{2014} every section is this node's own." } else if leader_answered { "The jobs' sections come from the leader; peers and staging are this node's." } else { "The leader does not answer: the jobs' sections are empty until it does." };
        line(scene, fonts, top, lx, y + 8.0 * s, t::DETAIL_SIZE_M, false, false, sub, w - 2.0 * pad - 90.0 * s); let rw = self.rescan.measure(fonts, s).0; self.rescan.rect = Rect::new(x1 - rw, y - 4.0 * s, x1, y + 20.0 * s); self.rescan.enabled = !self.busy; self.rescan.paint(scene, fonts, s, now); y += 30.0 * s;
        let bh = t::BUTTON_HEIGHT_M as f64 * s; self.list = Rect::new(lx, y, x1, y0 + h - pad - bh - 12.0 * s); let (row, head, bx) = (24.0 * s, 34.0 * s, checkbox::size(s));
        let total: f64 = (0..6).map(|i| head + self.rows(i).len().max(1) as f64 * row + 10.0 * s).sum(); self.bar.set(self.list, total, s);
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &self.list); self.hits.clear(); let mut yy = self.list.y0 - self.bar.offset; let mut act_rects = vec![Rect::ZERO; 6];
        for i in 0..6 { let rows = self.rows(i); let n_ticked = rows.iter().filter(|r| self.ticked.contains(&(i, r.0.clone()))).count(); let sec_h = head + rows.len().max(1) as f64 * row + 4.0 * s;
            if yy + sec_h > self.list.y0 && yy < self.list.y1 {
                paint::rounded(scene, Rect::new(self.list.x0, yy, self.list.x1 - 10.0 * s, yy + sec_h), t::CORNER_RADIUS_500 as f64 * s, t::BACKGROUND_LAYER_2_COLOR);
                let b = Rect::new(self.list.x0 + 10.0 * s, yy + (head - bx) / 2.0, self.list.x0 + 10.0 * s + bx, yy + (head + bx) / 2.0); checkbox::paint_box(scene, fonts, b, if rows.is_empty() || n_ticked == 0 { Some(false) } else if n_ticked == rows.len() { Some(true) } else { None }, !rows.is_empty(), false, s); self.hits.push((b.inflate(4.0 * s, 4.0 * s), i, None));
                let tw = line(scene, fonts, SECTIONS[i].1, b.x1 + 10.0 * s, yy + head / 2.0, t::DETAIL_SIZE_M, true, false, ink, self.list.width() - 220.0 * s); line(scene, fonts, &format!("\u{b7} {}", rows.len()), b.x1 + 16.0 * s + tw, yy + head / 2.0, t::DETAIL_SIZE_M, false, false, sub, 60.0 * s);
                let aw = self.acts[i].measure(fonts, s).0; act_rects[i] = Rect::new(self.list.x1 - 20.0 * s - aw, yy + 5.0 * s, self.list.x1 - 20.0 * s, yy + 29.0 * s);
                if rows.is_empty() { line(scene, fonts, SECTIONS[i].4, b.x1 + 10.0 * s, yy + head + row / 2.0, t::DETAIL_SIZE_M, false, false, sub, self.list.width() - 60.0 * s); }
                for (k, (id, label, detail)) in rows.iter().enumerate() { let ry = yy + head + k as f64 * row; if ry + row < self.list.y0 || ry > self.list.y1 { continue }
                    let rb = Rect::new(b.x0, ry + (row - bx) / 2.0, b.x1, ry + (row + bx) / 2.0); checkbox::paint_box(scene, fonts, rb, Some(self.ticked.contains(&(i, id.clone()))), true, false, s); self.hits.push((Rect::new(self.list.x0, ry, self.list.x1 - 10.0 * s, ry + row), i, Some(id.clone())));
                    let lw = line(scene, fonts, label, rb.x1 + 10.0 * s, ry + row / 2.0, t::DETAIL_SIZE_M, false, true, ink, self.list.width() * 0.6); line(scene, fonts, detail, rb.x1 + 24.0 * s + lw, ry + row / 2.0, t::DETAIL_SIZE_M, false, false, sub, self.list.width() * 0.3); }
            }
            self.acts[i].enabled = n_ticked > 0 && !self.busy; yy += sec_h + 10.0 * s; }
        for (i, r) in act_rects.iter().enumerate() { self.acts[i].rect = *r; if r.width() > 0.0 { self.acts[i].paint(scene, fonts, s, now); } }
        scene.pop_layer(); self.bar.paint(scene, now);
        let by = y0 + h - pad - bh; let (wc, _) = self.close.measure(fonts, s); self.close.rect = Rect::new(x1 - wc, by, x1, by + bh); self.close.paint(scene, fonts, s, now);
    }
    pub fn animating(&self, now: Instant) -> bool { self.busy || self.bar.animating(now) || self.close.animating(now) || self.acts.iter().any(|b| b.animating(now)) }
}
