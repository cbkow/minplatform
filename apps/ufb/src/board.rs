//! The board (canon row 492; UFB 1.4.2's `KanbanBoard`, read into `docs/phase5/step0-projects-metadata.md`, section
//! 5.3): the items of a job's section as cards in LANES - one a value of one column. It stands where the item list
//! stands and shows the same items in the same order; a card dragged to another lane sets the column's value.
//!
//! 1.4.2's rules, kept: a board can be grouped by a checkbox column, by a dropdown or priority column that HAS
//! options, or by the star; a dropdown's lanes are its options in their order, then "Unset", then one lane - nothing
//! can be dropped into it - for each value that is no option; a checkbox's are Checked and Unchecked; the star's are
//! Tracked and Untracked. The board never sorts: the cards of a lane lie in the list's order.
use crate::db::kind_of;
use crate::itemlist::{is_checked, text_of, ItemList};
use crate::jobview::Item;
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use min_ui::{fonts::draw_layout, paint, Button, Event, Family, Fonts, ScrollBar};
use std::time::Instant;
use vello::Scene;

/// What a board is grouped by: a column (its uuid), or the star.
#[derive(Clone, Debug, PartialEq)]
pub enum LaneBy { Col(String), Tracked }
/// What a lane stands for - and what a card dropped into it is given.
#[derive(Clone, Debug, PartialEq)]
pub enum LaneValue { Option(String), Unset, Check(bool), Tracked(bool), /// a value that is no option: shown, not dropped into
    Other(String) }
#[derive(Clone, Debug, PartialEq)]
pub struct Lane { pub title: String, pub color: Option<String>, pub value: LaneValue, /// the items' places in the job's list
    pub items: Vec<usize> }
/// What the pointer did.
#[derive(Clone, Debug, PartialEq)]
pub enum Act { Choose(usize), Star(usize), Menu(usize, (f64, f64)), /// a card was let go on another lane
    Drop(usize, LaneValue) }

/// The columns a board can be grouped by, with their names; the star last (1.4.2's "Tracked").
pub fn eligible(list: &ItemList) -> Vec<(LaneBy, String)> {
    let mut out: Vec<(LaneBy, String)> = list.cols.iter().filter(|c| match kind_of(&c.kind) { "checkbox" => true, "dropdown" | "priority" => !c.options.is_empty(), _ => false }).map(|c| (LaneBy::Col(c.uuid.clone()), c.name.clone())).collect();
    out.push((LaneBy::Tracked, "Tracked".to_string())); out
}
/// The lanes of a board over these items. None: the column is not there any more, or cannot group (the board gives way to the list).
pub fn lanes(by: &LaneBy, items: &[Item], list: &ItemList) -> Option<Vec<Lane>> {
    let order: Vec<usize> = if list.order.len() == items.len() { list.order.clone() } else { (0..items.len()).collect() };
    let lane = |title: &str, color: Option<String>, value: LaneValue, keep: &dyn Fn(&Item) -> bool| Lane { title: title.to_string(), color, value, items: order.iter().copied().filter(|i| keep(&items[*i])).collect() };
    Some(match by {
        LaneBy::Tracked => vec![lane("Tracked", None, LaneValue::Tracked(true), &|it| list.tracked(it)), lane("Untracked", None, LaneValue::Tracked(false), &|it| !list.tracked(it))],
        LaneBy::Col(uuid) => { let c = list.cols.iter().find(|c| &c.uuid == uuid)?;
            match kind_of(&c.kind) {
                "checkbox" => vec![lane("Checked", None, LaneValue::Check(true), &|it| is_checked(list.value(it, c))), lane("Unchecked", None, LaneValue::Check(false), &|it| !is_checked(list.value(it, c)))],
                "dropdown" | "priority" if !c.options.is_empty() => {
                    let val = |it: &Item| text_of(&c.kind, list.value(it, c));
                    let mut out: Vec<Lane> = c.options.iter().map(|(name, color)| lane(name, color.clone(), LaneValue::Option(name.clone()), &|it| val(it) == *name)).collect();
                    out.push(lane("Unset", None, LaneValue::Unset, &|it| val(it).is_empty()));
                    let mut others: Vec<String> = vec![]; for i in &order { let v = val(&items[*i]); if !v.is_empty() && !c.options.iter().any(|o| o.0 == v) && !others.contains(&v) { others.push(v); } }
                    for v in others { out.push(lane(&format!("Other: {v}"), None, LaneValue::Other(v.clone()), &|it| val(it) == v)); }
                    out
                }
                _ => return None,
            } }
    })
}

/// Logical px.
const LANE_MIN: f64 = 168.0; const HEAD: f64 = 34.0; const CARD: f64 = 50.0; const GAP: f64 = 6.0; const PAD: f64 = 8.0; const SLOP: f64 = 6.0;
pub struct Board {
    pub by: LaneBy, pub lanes: Vec<Lane>, pub rect: Rect, pub bar: ScrollBar, pub hbar: ScrollBar,
    /// where the lanes and the cards lie (px): a lane's place; (the item, its card)
    lane_rects: Vec<Rect>, cards: Vec<(usize, Rect)>, hover: Option<usize>,
    /// a card pressed: (the item, where it was pressed, it is being dragged), and where the pointer is
    press: Option<(usize, (f64, f64), bool)>, at: (f64, f64), scale: f64,
    /// the cards chosen together (the items' places; row 530): a drag of one of them takes them all
    pub sel: Vec<usize>,
}
impl Board {
    pub fn new(by: LaneBy) -> Board { Board { by, lanes: vec![], rect: Rect::ZERO, bar: ScrollBar::default(), hbar: ScrollBar::horizontal(), lane_rects: vec![], cards: vec![], hover: None, press: None, at: (0.0, 0.0), scale: 1.0, sel: vec![] } }
    /// The lanes are made anew from the items and what they carry. False: the board cannot stand any more.
    pub fn rebuild(&mut self, items: &[Item], list: &ItemList) -> bool { match lanes(&self.by, items, list) { Some(l) => { self.lanes = l; true } None => false } }
    pub fn dragging(&self) -> bool { self.press.is_some_and(|p| p.2) }
    pub fn animating(&self, now: Instant) -> bool { self.bar.animating(now) || self.hbar.animating(now) || self.dragging() }
    fn lane_at(&self, x: f64, y: f64) -> Option<usize> { self.lane_rects.iter().position(|r| r.contains(kurbo::Point::new(x, y))) }
    pub fn lane_of(&self, item: usize) -> Option<usize> { self.lanes.iter().position(|l| l.items.contains(&item)) }

    pub fn layout(&mut self, rect: Rect, s: f64) {
        self.rect = rect; self.scale = s; let n = self.lanes.len().max(1) as f64; let pad = PAD * s;
        // the lanes share the room when there is enough of it; else each keeps its least width and the board scrolls sideways
        let w = ((rect.width() - pad * (n + 1.0)) / n).max(LANE_MIN * s); let content_w = pad + n * (w + pad);
        self.hbar.set(rect, content_w, s);
        let tallest = self.lanes.iter().map(|l| l.items.len()).max().unwrap_or(0) as f64; self.bar.set(Rect::new(rect.x0, rect.y0 + HEAD * s, rect.x1, rect.y1), tallest * (CARD + GAP) * s + pad * 2.0, s);
        self.lane_rects.clear(); self.cards.clear();
        for (k, l) in self.lanes.iter().enumerate() {
            let x0 = rect.x0 + pad + k as f64 * (w + pad) - self.hbar.offset; let lr = Rect::new(x0, rect.y0 + pad * 0.5, x0 + w, rect.y1 - pad * 0.5); self.lane_rects.push(lr);
            for (j, i) in l.items.iter().enumerate() { let y = rect.y0 + HEAD * s + pad * 0.5 + j as f64 * (CARD + GAP) * s - self.bar.offset; self.cards.push((*i, Rect::new(lr.x0 + 6.0 * s, y, lr.x1 - 6.0 * s, y + CARD * s))); }
        }
    }
    fn card_at(&self, x: f64, y: f64) -> Option<(usize, Rect)> { if y < self.rect.y0 + HEAD * self.scale { return None } self.cards.iter().find(|(_, r)| r.contains(kurbo::Point::new(x, y))).copied() }
    fn star_rect(card: Rect, s: f64) -> Rect { Rect::new(card.x0 + 4.0 * s, card.y0 + 4.0 * s, card.x0 + 28.0 * s, card.y0 + 28.0 * s) }

    pub fn event(&mut self, e: &Event, now: Instant) -> Vec<Act> {
        let mut out = vec![]; let s = self.scale;
        if self.press.is_none() { self.bar.event(e, now); if self.hbar.event(e, now).1 { return out } }
        if let Some(p) = e.pos() { self.at = p; self.hover = self.card_at(p.0, p.1).map(|c| c.0); }
        match *e {
            Event::Down { x, y, button } if self.rect.contains(kurbo::Point::new(x, y)) => if let Some((i, r)) = self.card_at(x, y) {
                out.push(Act::Choose(i));
                if button == Button::Secondary { out.push(Act::Menu(i, (x, y))); }
                else if button == Button::Primary { if Self::star_rect(r, s).contains(kurbo::Point::new(x, y)) { out.push(Act::Star(i)); } else { self.press = Some((i, (x, y), false)); } }
            },
            // a pressed card moved far enough is in hand
            Event::Move { x, y } => if let Some((i, p0, false)) = self.press { if (x - p0.0).hypot(y - p0.1) > SLOP * s { self.press = Some((i, p0, true)); } },
            // let go: on another lane that takes cards, the card is given that lane's value
            Event::Up { x, y, button: Button::Primary } => if let Some((i, _, dragged)) = self.press.take() { if dragged { if let Some(k) = self.lane_at(x, y) { if self.lane_of(i) != Some(k) && !matches!(self.lanes[k].value, LaneValue::Other(_)) { out.push(Act::Drop(i, self.lanes[k].value.clone())); } } } },
            Event::Leave => self.press = None,
            _ => {}
        }
        out
    }
    /// Escape lets a card in hand go back.
    pub fn cancel_drag(&mut self) -> bool { self.press.take().is_some_and(|p| p.2) }

    fn card(&self, scene: &mut Scene, fonts: &mut Fonts, it: &Item, list: &ItemList, r: Rect, chosen: bool, strong: bool, hover: bool, today: crate::format::Civil) {
        let s = self.scale; let (ink, sub) = (paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT)); let small = t::BODY_SIZE_S * s as f32; let radius = t::CORNER_RADIUS_MEDIUM_DEFAULT as f64 * s;
        if chosen { paint::rounded(scene, r.inflate(1.5 * s, 1.5 * s), radius + 1.5 * s, if strong { t::FOCUS_INDICATOR_COLOR } else { t::DIVIDER_COLOR_MEDIUM }); }
        paint::rounded(scene, r, radius, if hover && !chosen { t::HOVER_BACKGROUND_COLOR } else { t::BACKGROUND_LAYER_1_COLOR });
        let tracked = list.tracked(it); let sr = Self::star_rect(r, s);
        if tracked { let c = t::NOTICE_VISUAL_COLOR; let d = 20.0 * s; paint::rounded(scene, Rect::new(sr.center().x - d / 2.0, sr.center().y - d / 2.0, sr.center().x + d / 2.0, sr.center().y + d / 2.0), d / 2.0, min_theme::Color::rgba(c.r, c.g, c.b, 0.28)); }
        if tracked || hover || chosen { if let Some(g) = fonts.icon("star", 14.0 * s as f32, if tracked { paint::color(t::NOTICE_VISUAL_COLOR) } else { sub }) { draw_layout(scene, &g, Affine::translate((sr.center().x - g.width() as f64 / 2.0, sr.center().y - g.height() as f64 / 2.0))); } }
        let l = fonts.layout_fit(&it.name, Family::Sans, small, t::FONT_WEIGHT_BOLD as f32, ink, (r.width() - 40.0 * s).max(10.0)); draw_layout(scene, &l, Affine::translate((r.x0 + 32.0 * s, r.y0 + 8.0 * s)));
        let d = fonts.layout_fit(&crate::format::date_text(it.modified, today), Family::Sans, t::BODY_SIZE_S * 0.92 * s as f32, t::FONT_WEIGHT_REGULAR as f32, sub, (r.width() - 40.0 * s).max(10.0)); draw_layout(scene, &d, Affine::translate((r.x0 + 32.0 * s, r.y1 - 8.0 * s - d.height() as f64)));
    }
    /// Draw the board. `chosen`: the chosen item's place among the job's; `focused`: the list's side holds the arrow keys.
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, items: &[Item], list: &ItemList, chosen: Option<usize>, focused: bool, now: Instant) {
        let s = self.scale; let sub = paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT); let small = t::BODY_SIZE_S * s as f32; let today = crate::format::local(crate::format::now_secs());
        let in_hand = self.press.filter(|p| p.2).map(|p| p.0); let over = in_hand.and_then(|i| self.lane_at(self.at.0, self.at.1).filter(|k| self.lane_of(i) != Some(*k) && !matches!(self.lanes[*k].value, LaneValue::Other(_))));
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &self.rect);
        for (k, l) in self.lanes.iter().enumerate() {
            let lr = self.lane_rects[k]; if lr.x1 < self.rect.x0 || lr.x0 > self.rect.x1 { continue }
            // a lane: a tile on the layer behind; the one a card in hand would land in is marked
            paint::rounded(scene, lr, t::CORNER_RADIUS_MEDIUM_DEFAULT as f64 * s, t::BACKGROUND_LAYER_2_COLOR);
            if over == Some(k) { let q = 2.0 * s; for e in [Rect::new(lr.x0, lr.y0, lr.x1, lr.y0 + q), Rect::new(lr.x0, lr.y1 - q, lr.x1, lr.y1), Rect::new(lr.x0, lr.y0, lr.x0 + q, lr.y1), Rect::new(lr.x1 - q, lr.y0, lr.x1, lr.y1)] { paint::rect(scene, e, t::FOCUS_INDICATOR_COLOR); } }
            let cy = lr.y0 + HEAD * s / 2.0; let count = fonts.layout(&l.items.len().to_string(), Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, sub);
            match (&l.value, &l.color) {
                (LaneValue::Option(_), c) => { crate::chips::paint_chip(scene, fonts, &l.title, c.as_deref(), lr.x0 + 8.0 * s, cy, lr.width() - 40.0 * s, s); }
                _ => { let tl = fonts.layout_fit(&l.title, Family::Sans, small, t::FONT_WEIGHT_BOLD as f32, sub, lr.width() - 44.0 * s); draw_layout(scene, &tl, Affine::translate((lr.x0 + 12.0 * s, cy - tl.height() as f64 / 2.0))); }
            }
            draw_layout(scene, &count, Affine::translate((lr.x1 - 10.0 * s - count.width() as f64, cy - count.height() as f64 / 2.0)));
        }
        // the cards, under the lanes' heads
        let body = Rect::new(self.rect.x0, self.rect.y0 + HEAD * s, self.rect.x1, self.rect.y1); scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &body);
        for (i, r) in &self.cards { if r.y1 < body.y0 || r.y0 > body.y1 || r.x1 < body.x0 || r.x0 > body.x1 || in_hand == Some(*i) { continue } if let Some(it) = items.get(*i) { self.card(scene, fonts, it, list, *r, chosen == Some(*i) || self.sel.contains(i), focused, self.hover == Some(*i) && in_hand.is_none(), today); } }
        scene.pop_layer(); scene.pop_layer();
        self.bar.paint(scene, now); self.hbar.paint(scene, now);
        // the card in hand follows the pointer, over everything of the board
        if let (Some(i), Some((_, p0, _))) = (in_hand, self.press) { if let (Some(it), Some((_, r))) = (items.get(i), self.cards.iter().find(|c| c.0 == i)) {
            let moved = Rect::new(r.x0 + self.at.0 - p0.0, r.y0 + self.at.1 - p0.1, r.x1 + self.at.0 - p0.0, r.y1 + self.at.1 - p0.1);
            for sh in t::POPOVER_SHADOW { scene.draw_blurred_rounded_rect(Affine::translate((sh.x as f64 * s, sh.y as f64 * s)), moved.inflate(sh.spread as f64 * s, sh.spread as f64 * s), paint::color(sh.color), 8.0 * s, (sh.blur as f64 * s / 2.0).max(0.1)); }
            self.card(scene, fonts, it, list, moved, true, true, false, today); } }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ColDef;
    use serde_json::json;
    use std::path::PathBuf;
    fn item(name: &str) -> Item { let p = PathBuf::from("/j/3d").join(name); Item { name: name.into(), path: p.clone(), modified: 0, project: p.clone(), renders: p, proxies: None } }
    #[test]
    fn lanes_are_1_4_2s() {
        let items = [item("a"), item("b"), item("c"), item("d")]; let mut l = ItemList::default();
        l.cols = vec![ColDef { id: 1, name: "Status".into(), kind: "dropdown".into(), order: 0, width: 120.0, visible: true, uuid: "u-s".into(), options: vec![("WIP".into(), Some("#e6b422".into())), ("Final".into(), None)] },
            ColDef { id: 2, name: "Approved".into(), kind: "checkbox".into(), order: 1, width: 120.0, visible: true, uuid: "u-a".into(), options: vec![] }, ColDef { id: 3, name: "Artist".into(), kind: "text".into(), order: 2, width: 120.0, visible: true, uuid: "u-t".into(), options: vec![] },
            ColDef { id: 4, name: "Empty".into(), kind: "dropdown".into(), order: 3, width: 120.0, visible: true, uuid: "u-e".into(), options: vec![] }];
        let mut put = |name: &str, key: &str, v: serde_json::Value| { l.meta.entry(PathBuf::from("/j/3d").join(name)).or_default().values.insert(key.into(), v); };
        put("a", "u-s", json!("Final")); put("b", "u-s", json!("WIP")); put("d", "u-s", json!("Gone")); put("c", "u-a", json!(true));
        l.meta.get_mut(&PathBuf::from("/j/3d/b")).unwrap().tracked = true; l.reorder(&items);
        // a board can be grouped by a checkbox, a dropdown WITH options, or the star - not by text, not by a dropdown with none
        assert_eq!(eligible(&l).iter().map(|e| e.1.as_str()).collect::<Vec<_>>(), ["Status", "Approved", "Tracked"]);
        let by = lanes(&LaneBy::Col("u-s".into()), &items, &l).unwrap();
        assert_eq!(by.iter().map(|x| (x.title.as_str(), x.items.clone())).collect::<Vec<_>>(), [("WIP", vec![1]), ("Final", vec![0]), ("Unset", vec![2]), ("Other: Gone", vec![3])], "the options in their order, Unset, then a lane a value that is no option");
        assert_eq!(by[3].value, LaneValue::Other("Gone".into()));
        let by = lanes(&LaneBy::Col("u-a".into()), &items, &l).unwrap(); assert_eq!(by.iter().map(|x| (x.title.as_str(), x.items.clone())).collect::<Vec<_>>(), [("Checked", vec![2]), ("Unchecked", vec![0, 1, 3])]);
        let by = lanes(&LaneBy::Tracked, &items, &l).unwrap(); assert_eq!(by.iter().map(|x| x.items.clone()).collect::<Vec<_>>(), [vec![1], vec![0, 2, 3]]);
        assert!(lanes(&LaneBy::Col("u-t".into()), &items, &l).is_none()); assert!(lanes(&LaneBy::Col("u-e".into()), &items, &l).is_none()); assert!(lanes(&LaneBy::Col("gone".into()), &items, &l).is_none(), "the column gone: no board");
        // the cards of a lane lie in the list's order
        l.sort = Some((crate::itemlist::SortBy::Name, false)); l.reorder(&items); let by = lanes(&LaneBy::Col("u-a".into()), &items, &l).unwrap(); assert_eq!(by[1].items, [3, 1, 0]);
    }
}
