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
    let (mut sel_anchor, mut sel_focus): (Option<TextPosition>, Option<TextPosition>) = (None, None);
    let (anchor_b, focus_b) = (sel.anchor().index(), sel.focus().index());
    let mut next_id = 2u64;
    let mut first_line_of_focused: Option<(usize, usize)> = None;
    for (bi, b) in blocks.iter().enumerate() {
        for (li, line) in b.layout.lines().enumerate() {
            let m = line.metrics();
            let top = b.y + ((m.baseline - m.line_height + m.offset) / scale as f32) as f64;
            let bottom = b.y + ((m.baseline + m.offset) / scale as f32) as f64;
            let in_view = bottom >= visible.0 && top <= visible.1;
            // S9: S9_ALL_LINES=1 exposes every line (the offsets an outside tool computes only add up when nothing is left out)
            if !in_view && !b.focused && std::env::var_os("S9_ALL_LINES").is_none() { continue }
            let id = NodeId(next_id); next_id += 1;
            let range = line.text_range(); let range = range.start.min(b.text.len())..range.end.min(b.text.len());   // an empty block's line reads 0..1
            // S9: the runs must add up to the document value, which joins blocks with "\n" — the block's last line carries it
            let joiner = li + 1 == b.layout.len() && bi + 1 < blocks.len();
            let char_starts: Vec<usize> = b.text[range.clone()].char_indices().map(|(x, _)| range.start + x).collect();
            let line_text = &b.text[range.clone()];
            let mut n = Node::new(Role::TextRun);
            n.set_value(if joiner { format!("{line_text}\n") } else { line_text.to_string() });
            n.set_text_direction(TextDirection::LeftToRight);
            let xs: Vec<f32> = char_starts.iter().chain(std::iter::once(&range.end)).map(|&x| (Cursor::from_byte_index(b.layout, x, Affinity::Downstream).geometry(b.layout, 1.0).x0 / scale) as f32).collect();
            let x_left = xs.iter().cloned().fold(f32::INFINITY, f32::min).min(0.0);
            n.set_character_lengths(line_text.chars().map(|c| c.len_utf8() as u8).chain(joiner.then_some(1u8)).collect::<Vec<_>>());
            n.set_character_positions(xs.iter().take(char_starts.len() + joiner as usize).map(|x| x - x_left).collect::<Vec<_>>());
            n.set_character_widths(xs.windows(2).map(|w| (w[1] - w[0]).max(0.0)).chain(joiner.then_some(0.0)).collect::<Vec<_>>());
            n.set_bounds(Rect { x0: pad_x + x_left as f64, y0: top.min(bottom), x1: pad_x + xs.last().copied().unwrap_or(0.0) as f64, y1: top.max(bottom) });
            if b.focused {
                if first_line_of_focused.is_none() { first_line_of_focused = Some((refs.len(), li)); }
                // S9: the anchor and the focus are placed INDEPENDENTLY, each on the one line that holds its byte (a line
                // owns [start, end); the block's last line owns its end too). The first version built both from one
                // line and reported a selection that crossed a wrap as starting at the next line — an outside tool
                // that reads the selection back before typing (Grammarly does) then saw the wrong text.
                let last = li + 1 == b.layout.len();
                let owns = |byte: usize| byte >= range.start && (byte < range.end || (last && byte == range.end));
                let at = |byte: usize| TextPosition { node: id, character_index: char_starts.iter().position(|&s| s >= byte).unwrap_or(char_starts.len()) };
                if owns(anchor_b) { sel_anchor = Some(at(anchor_b)); }
                if owns(focus_b) { sel_focus = Some(at(focus_b)); }
            }
            refs.push(LineRef { block: bi, line: li, range, char_starts });
            line_nodes.push(id);
            nodes.push((id, n));
        }
    }
    let mut root = Node::new(Role::Window); root.set_label("S3 text"); root.set_children([INPUT]);
    // S9: AccessKit's macOS adapter reads coordinates in BACKING pixels; everything above is in points — one transform on the root
    // (without it every rectangle was half-size and half-offset on a 2x screen, so an outside tool underlined the wrong place)
    root.set_transform(accesskit::Affine::scale(scale));
    let mut input = Node::new(Role::MultilineTextInput);
    input.set_label("Editor");
    input.set_value(doc_value.to_string());
    input.set_bounds(Rect { x0: pad_x, y0: 0.0, x1: size.width as f64 / scale - pad_x, y1: size.height as f64 / scale });
    input.add_action(Action::Focus); input.add_action(Action::SetTextSelection); input.add_action(Action::ReplaceSelectedText);
    input.set_children(line_nodes);
    if let (Some(anchor), Some(focus)) = (sel_anchor, sel_focus) { input.set_text_selection(TextSelection { anchor, focus }); }
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
