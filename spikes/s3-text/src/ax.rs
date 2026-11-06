//! S3 — the AccessKit tree for the editor: Window → MultilineTextInput (value, bounds, selection, actions) → one TextRun per
//! VISIBLE laid-out line across all blocks (value, character lengths / positions / widths, bounds). Logical window px.
use accesskit::{Action, Node, NodeId, Rect, Role, TextDirection, TextPosition, TextSelection, TreeId, TreeInfo, TreeUpdate};
use parley::{Affinity, Cursor, Layout, Selection};

pub const ROOT: NodeId = NodeId(0);
pub const INPUT: NodeId = NodeId(1);

/// One block of the document as the tree sees it: its layout, its text, its top in logical px, and whether it holds the caret.
pub struct BlockView<'a, B: parley::Brush> { pub layout: &'a Layout<B>, pub text: &'a str, pub y: f64, pub focused: bool }

pub struct LineRef { pub block: usize, pub line: usize, pub range: std::ops::Range<usize>, pub char_starts: Vec<usize> }

pub fn tree<B: parley::Brush>(blocks: &[BlockView<'_, B>], sel: &Selection, pad_x: f64, scale: f64, size: winit::dpi::PhysicalSize<u32>, visible: (f64, f64), doc_value: &str) -> (TreeUpdate, Vec<LineRef>) {
    let mut nodes = Vec::new();
    let mut refs: Vec<LineRef> = vec![];
    let mut line_nodes = vec![];
    let mut selection: Option<TextSelection> = None;
    let (anchor_b, focus_b) = (sel.anchor().index(), sel.focus().index());
    let mut next_id = 2u64;
    let mut first_line_of_focused: Option<(usize, usize)> = None;
    for (bi, b) in blocks.iter().enumerate() {
        for (li, line) in b.layout.lines().enumerate() {
            let m = line.metrics();
            let top = b.y + ((m.baseline - m.line_height + m.offset) / scale as f32) as f64;
            let bottom = b.y + ((m.baseline + m.offset) / scale as f32) as f64;
            let in_view = bottom >= visible.0 && top <= visible.1;
            if !in_view && !b.focused { continue }
            let id = NodeId(next_id); next_id += 1;
            let range = line.text_range();
            let char_starts: Vec<usize> = b.text[range.clone()].char_indices().map(|(x, _)| range.start + x).collect();
            let line_text = &b.text[range.clone()];
            let mut n = Node::new(Role::TextRun);
            n.set_value(line_text.to_string());
            n.set_text_direction(TextDirection::LeftToRight);
            let xs: Vec<f32> = char_starts.iter().chain(std::iter::once(&range.end)).map(|&x| (Cursor::from_byte_index(b.layout, x, Affinity::Downstream).geometry(b.layout, 1.0).x0 / scale) as f32).collect();
            let x_left = xs.iter().cloned().fold(f32::INFINITY, f32::min).min(0.0);
            n.set_character_lengths(line_text.chars().map(|c| c.len_utf8() as u8).collect::<Vec<_>>());
            n.set_character_positions(xs.iter().take(char_starts.len()).map(|x| x - x_left).collect::<Vec<_>>());
            n.set_character_widths(xs.windows(2).map(|w| (w[1] - w[0]).max(0.0)).collect::<Vec<_>>());
            n.set_bounds(Rect { x0: pad_x + x_left as f64, y0: top.min(bottom), x1: pad_x + xs.last().copied().unwrap_or(0.0) as f64, y1: top.max(bottom) });
            if b.focused {
                if first_line_of_focused.is_none() { first_line_of_focused = Some((refs.len(), li)); }
                let pos = |byte: usize| -> Option<TextPosition> {
                    let last = li + 1 == b.layout.len();
                    if byte < range.end || (last && byte <= range.end) || byte == range.end { Some(TextPosition { node: id, character_index: char_starts.iter().position(|&s| s >= byte).unwrap_or(char_starts.len()) }) } else { None }
                };
                let (a, f) = (pos(anchor_b), pos(focus_b));
                if selection.is_none() { if let (Some(a), Some(f)) = (a, f) { selection = Some(TextSelection { anchor: a, focus: f }); } }
                else if let Some(s) = selection.as_mut() { if s.anchor.node == s.focus.node && a.is_none() && f.is_none() { /* keep */ } }
                // a selection spanning lines: anchor from an earlier line, focus here
                if let Some(s) = selection.as_mut() { if let Some(f) = f { if focus_b >= range.start && focus_b < range.end || (li + 1 == b.layout.len() && focus_b == range.end) { s.focus = f; } } if let Some(a) = a { if anchor_b >= range.start && anchor_b < range.end || (li + 1 == b.layout.len() && anchor_b == range.end) { s.anchor = a; } } }
            }
            refs.push(LineRef { block: bi, line: li, range, char_starts });
            line_nodes.push(id);
            nodes.push((id, n));
        }
    }
    let mut root = Node::new(Role::Window); root.set_label("S3 text"); root.set_children([INPUT]);
    let mut input = Node::new(Role::MultilineTextInput);
    input.set_label("Editor");
    input.set_value(doc_value.to_string());
    input.set_bounds(Rect { x0: pad_x, y0: 0.0, x1: size.width as f64 / scale - pad_x, y1: size.height as f64 / scale });
    input.add_action(Action::Focus); input.add_action(Action::SetTextSelection); input.add_action(Action::ReplaceSelectedText);
    input.set_children(line_nodes);
    if let Some(s) = selection { input.set_text_selection(s); }
    nodes.push((ROOT, root)); nodes.push((INPUT, input));
    (TreeUpdate { nodes, tree: Some(TreeInfo::new(ROOT)), tree_id: TreeId::ROOT, focus: INPUT }, refs)
}

/// An AccessKit selection (from a SetTextSelection action) → (block, anchor byte, focus byte), using the refs of the last tree.
pub fn selection_to_bytes(refs: &[LineRef], sel: &TextSelection) -> Option<(usize, usize, usize)> {
    let byte = |p: &TextPosition| -> Option<(usize, usize)> { let r = refs.get((p.node.0 as usize).checked_sub(2)?)?; Some((r.block, r.char_starts.get(p.character_index).copied().unwrap_or(r.range.end))) };
    let (ba, a) = byte(&sel.anchor)?; let (bf, f) = byte(&sel.focus)?;
    if ba != bf { return Some((bf, f, f)) }
    Some((bf, a, f))
}
