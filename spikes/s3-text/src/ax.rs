//! S3 — the AccessKit tree for the editor: Window → MultilineTextInput (value, bounds, selection, actions) → one TextRun per
//! laid-out line (value, character lengths / positions / widths, word lengths, bounds). Coordinates in LOGICAL window px.
use accesskit::{Action, Node, NodeId, Rect, Role, TextDirection, TextPosition, TextSelection, TreeId, TreeInfo, TreeUpdate};
use parley::{Affinity, Cursor, Layout, Selection};

pub const ROOT: NodeId = NodeId(0);
pub const INPUT: NodeId = NodeId(1);
fn line_id(i: usize) -> NodeId { NodeId(2 + i as u64) }

struct LineInfo { id: NodeId, range: std::ops::Range<usize>, char_starts: Vec<usize> }

fn lines<B: parley::Brush>(layout: &Layout<B>, text: &str) -> Vec<LineInfo> {
    layout.lines().enumerate().map(|(i, line)| {
        let range = line.text_range();
        let char_starts: Vec<usize> = text[range.clone()].char_indices().map(|(b, _)| range.start + b).collect();
        LineInfo { id: line_id(i), range, char_starts }
    }).collect()
}

/// A byte offset → (line, character index) — the line containing it (end-of-line positions belong to the line they end).
fn position(lines: &[LineInfo], byte: usize) -> TextPosition {
    for (i, l) in lines.iter().enumerate() {
        let last = i + 1 == lines.len();
        if byte < l.range.end || (last && byte <= l.range.end) || (byte == l.range.end && !text_ends_with_newline(l)) {
            let idx = l.char_starts.iter().position(|&s| s >= byte).unwrap_or(l.char_starts.len());
            return TextPosition { node: l.id, character_index: idx };
        }
    }
    TextPosition { node: lines.last().map(|l| l.id).unwrap_or(INPUT), character_index: lines.last().map(|l| l.char_starts.len()).unwrap_or(0) }
}
fn text_ends_with_newline(_l: &LineInfo) -> bool { false }

pub fn tree<B: parley::Brush>(layout: &Layout<B>, text: &str, sel: &Selection, pad_x: f64, pad_y: f64, scale: f64, size: winit::dpi::PhysicalSize<u32>) -> TreeUpdate {
    let s = scale as f32;
    let ls = lines(layout, text);
    let mut nodes = Vec::with_capacity(ls.len() + 2);
    let mut root = Node::new(Role::Window);
    root.set_label("S3 text");
    root.set_children([INPUT]);
    let mut input = Node::new(Role::MultilineTextInput);
    input.set_label("Editor");
    input.set_value(text.to_string());
    input.set_bounds(Rect { x0: pad_x, y0: pad_y, x1: size.width as f64 / scale - pad_x, y1: size.height as f64 / scale - pad_y });
    input.add_action(Action::Focus); input.add_action(Action::SetTextSelection); input.add_action(Action::ReplaceSelectedText);
    input.set_children(ls.iter().map(|l| l.id).collect::<Vec<_>>());
    input.set_text_selection(TextSelection { anchor: position(&ls, sel.anchor().index()), focus: position(&ls, sel.focus().index()) });
    nodes.push((ROOT, root));
    for (line, info) in layout.lines().zip(&ls) {
        let m = line.metrics();
        let mut n = Node::new(Role::TextRun);
        let line_text = &text[info.range.clone()];
        n.set_value(line_text.to_string());
        n.set_text_direction(TextDirection::LeftToRight);
        // per-character geometry from caret positions (logical px, relative to the line's left edge)
        let xs: Vec<f32> = info.char_starts.iter().chain(std::iter::once(&info.range.end)).map(|&b| (Cursor::from_byte_index(layout, b, Affinity::Downstream).geometry(layout, 1.0).x0 / scale) as f32).collect();
        let x_left = xs.iter().cloned().fold(f32::INFINITY, f32::min).min(0.0);
        n.set_character_lengths(line_text.chars().map(|c| c.len_utf8() as u8).collect::<Vec<_>>());
        n.set_character_positions(xs.iter().take(info.char_starts.len()).map(|x| x - x_left).collect::<Vec<_>>());
        n.set_character_widths(xs.windows(2).map(|w| (w[1] - w[0]).max(0.0)).collect::<Vec<_>>());
        let (top, bottom) = ((m.baseline - m.line_height + m.offset) / s, (m.baseline + m.offset) / s);
        n.set_bounds(Rect { x0: pad_x + x_left as f64, y0: pad_y + top.min(bottom) as f64, x1: pad_x + xs.last().copied().unwrap_or(0.0) as f64, y1: pad_y + top.max(bottom) as f64 });
        nodes.push((info.id, n));
    }
    nodes.push((INPUT, input));
    TreeUpdate { nodes, tree: Some(TreeInfo::new(ROOT)), tree_id: TreeId::ROOT, focus: INPUT }
}

/// An AccessKit selection (from a SetTextSelection action) → byte offsets (anchor, focus).
pub fn selection_to_bytes<B: parley::Brush>(layout: &Layout<B>, text: &str, sel: &TextSelection) -> Option<(usize, usize)> {
    let byte = |p: &TextPosition| -> Option<usize> {
        let i = (p.node.0 as usize).checked_sub(2)?;
        let line = layout.lines().nth(i)?; let r = line.text_range();
        let starts: Vec<usize> = text[r.clone()].char_indices().map(|(b, _)| r.start + b).collect();
        Some(starts.get(p.character_index).copied().unwrap_or(r.end))
    };
    Some((byte(&sel.anchor)?, byte(&sel.focus)?))
}
