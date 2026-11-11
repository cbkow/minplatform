//! The flat text view (rows 276, 281): the whole note as ONE text, which is what a hand-written desktop text element
//! exposes — with the map from a UTF-16 offset in that text back to a block and a byte offset in it.
//!
//! Every LEAF contributes one line, in document order, lines joined by "\n": a text block its text, a media block or a
//! divider the object-replacement character (so it has a position a tool can select around and a caret can sit
//! beside). Containers contribute nothing themselves. A "\n" inside a block's own text (a code block) stays as it is.
use crate::{id::BlockId, text, tree::Doc};

/// U+FFFC, the placeholder line of a block that holds no text.
pub const OBJECT: char = '\u{FFFC}';

#[derive(Clone, Debug, PartialEq)]
pub struct Line { pub block: BlockId, pub start8: usize, pub len8: usize, pub start16: usize, pub len16: usize, pub is_text: bool }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlatText { pub text: String, pub lines: Vec<Line>, pub len16: usize }

impl FlatText {
    pub fn build(doc: &Doc) -> Self {
        let mut out = FlatText::default();
        for b in doc.walk() {
            if b.kind.is_container() { continue }
            if !out.lines.is_empty() { out.text.push('\n'); out.len16 += 1; }
            let (start8, start16) = (out.text.len(), out.len16);
            let is_text = b.kind.is_text();
            if is_text { out.text.push_str(&b.text) } else { out.text.push(OBJECT) }
            let len8 = out.text.len() - start8; let len16 = text::len16(&out.text[start8..]);
            out.len16 += len16;
            out.lines.push(Line { block: b.id, start8, len8, start16, len16, is_text });
        }
        out
    }
    /// The line holding UTF-16 offset `o` (the line's end, before its "\n", belongs to it), and the byte offset inside
    /// that block's text. A placeholder line gives 0 (before the object) or 1 (after it).
    pub fn locate16(&self, o: usize) -> Option<(&Line, usize)> {
        if self.lines.is_empty() { return None }
        let o = o.min(self.len16);
        let i = self.lines.partition_point(|l| l.start16 + l.len16 < o);
        let l = &self.lines[i.min(self.lines.len() - 1)];
        let within = o.saturating_sub(l.start16).min(l.len16);
        let byte = if l.is_text { text::to8(&self.text[l.start8..l.start8 + l.len8], within) } else { within };
        Some((l, byte))
    }
    /// The UTF-16 offset in the flat text of byte offset `byte` in `block` (for a placeholder line, 0 or 1).
    pub fn offset16(&self, block: BlockId, byte: usize) -> Option<usize> {
        let l = self.lines.iter().find(|l| l.block == block)?;
        Some(l.start16 + if l.is_text { text::to16(&self.text[l.start8..l.start8 + l.len8], byte) } else { byte.min(1) })
    }
    /// The text of UTF-16 range [start, start + len), clamped; an end inside a surrogate pair moves to its start.
    pub fn slice16(&self, start: usize, len: usize) -> &str {
        let a = text::to8(&self.text, start); let b = text::to8(&self.text, start.saturating_add(len));
        &self.text[a..b.max(a)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Kind;
    fn sample() -> (Doc, Vec<BlockId>) {
        let mut d = Doc::new(); let ids: Vec<BlockId> = (0..8).map(|_| BlockId::new()).collect();
        d.insert(None, 0, ids[0], Kind::Heading { level: 1 }, "Caf\u{e9} \u{1F600}").unwrap();       // 6 chars, 7 UTF-16 units, 10 bytes
        d.insert(None, 1, ids[1], Kind::Media { descriptor: "{}".into() }, "").unwrap();
        d.insert(None, 2, ids[2], Kind::Columns, "").unwrap();
        d.insert(Some(ids[2]), 0, ids[3], Kind::Column, "").unwrap();
        d.insert(Some(ids[3]), 0, ids[4], Kind::Paragraph, "left").unwrap();
        d.insert(Some(ids[2]), 1, ids[5], Kind::Column, "").unwrap();
        d.insert(Some(ids[5]), 0, ids[6], Kind::Code { lang: "txt".into() }, "a\nb").unwrap();
        d.insert(None, 3, ids[7], Kind::Paragraph, "").unwrap();
        (d, ids)
    }
    #[test]
    fn leaves_become_lines_and_containers_none() {
        let (d, ids) = sample(); let f = FlatText::build(&d);
        assert_eq!(f.text, "Caf\u{e9} \u{1F600}\n\u{FFFC}\nleft\na\nb\n");
        assert_eq!(f.lines.iter().map(|l| l.block).collect::<Vec<_>>(), [ids[0], ids[1], ids[4], ids[6], ids[7]]);
        assert_eq!(f.len16, f.text.encode_utf16().count());
        assert_eq!(f.lines.iter().map(|l| (l.start16, l.len16)).collect::<Vec<_>>(), [(0, 7), (8, 1), (10, 4), (15, 3), (19, 0)]);
    }
    #[test]
    fn a_utf16_offset_finds_its_block_and_byte_and_back() {
        let (d, ids) = sample(); let f = FlatText::build(&d);
        for o in 0..=f.len16 {
            let (l, byte) = f.locate16(o).unwrap();
            let back = f.offset16(l.block, byte).unwrap();
            // the low half of the emoji's surrogate pair (offset 6) maps to the emoji's start
            assert_eq!(back, if o == 6 { 5 } else { o }, "offset {o}");
        }
        assert_eq!(f.locate16(7).map(|(l, b)| (l.block, b)), Some((ids[0], 10)));     // the heading's end, before its line break
        assert_eq!(f.locate16(8).map(|(l, b)| (l.block, b)), Some((ids[1], 0)));      // before the media placeholder
        assert_eq!(f.locate16(9).map(|(l, b)| (l.block, b)), Some((ids[1], 1)));      // after it
        assert_eq!(f.locate16(17).map(|(l, b)| (l.block, b)), Some((ids[6], 2)));     // inside the code block, after its own "\n"
        assert_eq!(f.locate16(999).map(|(l, b)| (l.block, b)), Some((ids[7], 0)));
        assert_eq!(FlatText::build(&Doc::new()).locate16(0), None);
    }
    #[test]
    fn a_range_reads_across_blocks_in_utf16_units() {
        let (d, _) = sample(); let f = FlatText::build(&d);
        assert_eq!(f.slice16(4, 3), " \u{1F600}");
        assert_eq!(f.slice16(8, 6), "\u{FFFC}\nleft");
        assert_eq!(f.slice16(15, 999), "a\nb\n");
        assert_eq!(f.slice16(5, 1), "");            // half an emoji is nothing, never a broken character
    }
}
