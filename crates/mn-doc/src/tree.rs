//! The document: an ordered tree of blocks, each with an id and a parent (row 273). Leaves hold text or one media
//! item; two containers exist — Columns (layout) and Table (data) — with their inner levels (Column; Row, Cell) as
//! blocks of their own so that every part has an id a merge can address.
use std::collections::HashMap;
use crate::{id::BlockId, inline::Spans, rank::{self, Rank}};

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Paragraph,
    Heading { level: u8 },
    Quote,
    Code { lang: String },
    Bullet { depth: u8 },
    Numbered { depth: u8 },
    /// 0 to do, 1 doing, 2 done
    Task { depth: u8, state: u8 },
    Divider,
    /// One media item; the descriptor is carried as the file's JSON until the media step gives it a type.
    Media { descriptor: String },
    Columns,
    /// `width`: this column's share of the row, the shares of a Columns block summing to 1.
    Column { width: f32 },
    Table { header_rows: u8, columns: Vec<ColumnSpec> },
    /// Colours are strings as the file gives them (hex in practice); empty = none.
    Row { bg: String, fg: String },
    Cell { bg: String, fg: String },
}

/// What a table column holds (row 273: text, choice, checkmark, timecode).
#[derive(Clone, Debug, PartialEq)]
pub enum ColumnKind { Text, Choice { options: Vec<ChoiceOption> }, Check, Timecode { fps: f64 } }
#[derive(Clone, Debug, PartialEq)]
pub struct ChoiceOption { pub id: String, pub label: String, pub color: String }
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnSpec {
    /// A manual width in px; None = sized to its content.
    pub width: Option<u32>,
    /// 0 left, 1 centre, 2 right
    pub align: u8,
    pub bg: String,
    pub fg: String,
    pub kind: ColumnKind,
}
impl Default for ColumnSpec { fn default() -> Self { ColumnSpec { width: None, align: 0, bg: String::new(), fg: String::new(), kind: ColumnKind::Text } } }
impl Kind {
    pub fn column() -> Kind { Kind::Column { width: 0.0 } }
    pub fn table() -> Kind { Kind::Table { header_rows: 0, columns: vec![] } }
    pub fn row() -> Kind { Kind::Row { bg: String::new(), fg: String::new() } }
    pub fn cell() -> Kind { Kind::Cell { bg: String::new(), fg: String::new() } }
    /// Holds text the caret can enter.
    pub fn is_text(&self) -> bool { matches!(self, Kind::Paragraph | Kind::Heading { .. } | Kind::Quote | Kind::Code { .. } | Kind::Bullet { .. } | Kind::Numbered { .. } | Kind::Task { .. }) }
    pub fn is_container(&self) -> bool { matches!(self, Kind::Columns | Kind::Column { .. } | Kind::Table { .. } | Kind::Row { .. } | Kind::Cell { .. }) }
    /// What may sit directly inside what (row 273): a Columns block holds Columns' columns, a Table rows, a Row cells;
    /// a column or a cell holds leaves; a table may sit in a column; no columns in columns, no tables in tables, no
    /// Columns block in a cell. `parent` None is the document itself.
    pub fn may_hold(parent: Option<&Kind>, child: &Kind) -> bool {
        match (parent, child) {
            (Some(Kind::Columns), c) => matches!(c, Kind::Column { .. }),
            (Some(Kind::Table { .. }), c) => matches!(c, Kind::Row { .. }),
            (Some(Kind::Row { .. }), c) => matches!(c, Kind::Cell { .. }),
            (_, Kind::Column { .. } | Kind::Row { .. } | Kind::Cell { .. }) => false,
            (None, _) => true,
            (Some(Kind::Column { .. }), c) => !matches!(c, Kind::Columns),
            (Some(Kind::Cell { .. }), c) => !matches!(c, Kind::Columns | Kind::Table { .. }),
            (Some(_), _) => false,   // a leaf holds nothing
        }
    }
}

#[derive(Clone, Debug)]
pub struct Block {
    pub id: BlockId,
    pub parent: Option<BlockId>,
    /// Where the block sorts among its siblings in the file ([`rank`]). The document keeps it in step with the order
    /// of `children`; it is how order is STORED, not part of what a document says — two documents with the same blocks
    /// in the same order are equal whatever their ranks.
    pub rank: Rank,
    pub kind: Kind,
    /// UTF-8; empty for media, dividers and containers.
    pub text: String,
    /// Inline formatting over `text` (byte offsets); always empty for a code block and for blocks without text.
    pub spans: Spans,
    pub children: Vec<BlockId>,
}

impl PartialEq for Block {
    fn eq(&self, o: &Block) -> bool { self.id == o.id && self.parent == o.parent && self.kind == o.kind && self.text == o.text && self.spans == o.spans && self.children == o.children }
}

#[derive(Debug, PartialEq)]
pub enum TreeError { UnknownBlock(BlockId), DuplicateId(BlockId), NotAllowed { parent: Option<BlockId>, child: BlockId }, IndexPastEnd, WouldNestInItself }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Doc {
    blocks: HashMap<BlockId, Block>,
    root: Vec<BlockId>,
}

impl Doc {
    pub fn new() -> Self { Self::default() }
    pub fn get(&self, id: BlockId) -> Option<&Block> { self.blocks.get(&id) }
    pub fn len(&self) -> usize { self.blocks.len() }
    pub fn is_empty(&self) -> bool { self.blocks.is_empty() }
    /// The children of `parent` in order; None is the document.
    pub fn children(&self, parent: Option<BlockId>) -> &[BlockId] {
        match parent { None => &self.root, Some(p) => self.blocks.get(&p).map(|b| b.children.as_slice()).unwrap_or(&[]) }
    }
    fn children_mut(&mut self, parent: Option<BlockId>) -> Result<&mut Vec<BlockId>, TreeError> {
        match parent { None => Ok(&mut self.root), Some(p) => self.blocks.get_mut(&p).map(|b| &mut b.children).ok_or(TreeError::UnknownBlock(p)) }
    }
    /// Inserts a new block as child `index` of `parent`. The containment rules of row 273 are enforced here.
    pub fn insert(&mut self, parent: Option<BlockId>, index: usize, id: BlockId, kind: Kind, text: impl Into<String>) -> Result<(), TreeError> {
        if self.blocks.contains_key(&id) { return Err(TreeError::DuplicateId(id)) }
        self.check(parent, &kind, id)?;
        let list = self.children_mut(parent)?;
        if index > list.len() { return Err(TreeError::IndexPastEnd) }
        list.insert(index, id);
        self.blocks.insert(id, Block { id, parent, rank: Rank::first(), kind, text: text.into(), spans: Spans::new(), children: vec![] });
        self.place(parent, index, None);
        Ok(())
    }
    fn check(&self, parent: Option<BlockId>, kind: &Kind, child: BlockId) -> Result<(), TreeError> {
        let pk = match parent { None => None, Some(p) => Some(&self.blocks.get(&p).ok_or(TreeError::UnknownBlock(p))?.kind) };
        if !Kind::may_hold(pk, kind) { return Err(TreeError::NotAllowed { parent, child }) }
        // the two "no X in X" rules reach past the direct parent
        let mut up = parent;
        while let Some(p) = up {
            let b = &self.blocks[&p];
            if matches!((kind, &b.kind), (Kind::Table { .. }, Kind::Cell { .. }) | (Kind::Columns, Kind::Column { .. }) | (Kind::Columns, Kind::Cell { .. })) { return Err(TreeError::NotAllowed { parent, child }) }
            up = b.parent;
        }
        Ok(())
    }
    /// Gives child `index` of `parent` a rank between its neighbours — `keep` if that still fits (a block put back by
    /// undo returns to its old rank). When the new rank has grown past [`rank::MAX_LEN`], the whole list is re-spread.
    fn place(&mut self, parent: Option<BlockId>, index: usize, keep: Option<Rank>) {
        let list = self.children(parent);
        let id = list[index];
        let before = index.checked_sub(1).map(|i| self.blocks[&list[i]].rank.clone());
        let after = list.get(index + 1).map(|n| self.blocks[n].rank.clone());
        let fits = |r: &Rank| before.as_ref().is_none_or(|b| b < r) && after.as_ref().is_none_or(|a| r < a);
        let new = match keep { Some(k) if fits(&k) => Some(k), _ => Rank::between(before.as_ref(), after.as_ref()).ok() };
        match new {
            Some(r) if r.len() <= rank::MAX_LEN => self.blocks.get_mut(&id).unwrap().rank = r,
            _ => { let ids = list.to_vec(); for (id, r) in ids.iter().zip(rank::spread(ids.len())) { self.blocks.get_mut(id).unwrap().rank = r; } }
        }
    }
    /// Takes ranks read from a file, if they agree with the order of every list (each list strictly ascending) and
    /// every block has one; returns false and changes nothing otherwise.
    pub fn adopt_ranks(&mut self, ranks: impl IntoIterator<Item = (BlockId, Rank)>) -> bool {
        let given: HashMap<BlockId, Rank> = ranks.into_iter().collect();
        if given.len() != self.blocks.len() || !self.blocks.keys().all(|k| given.contains_key(k)) { return false }
        let ordered = |list: &[BlockId]| list.windows(2).all(|w| given[&w[0]] < given[&w[1]]) && list.iter().all(|c| given[c].len() <= rank::MAX_LEN);
        if !ordered(&self.root) || !self.blocks.values().all(|b| ordered(&b.children)) { return false }
        for (id, r) in given { self.blocks.get_mut(&id).unwrap().rank = r; }
        true
    }
    /// Sets every list's ranks from the order of its children (a document assembled from a file whose ranks are not
    /// to be trusted, or from another format).
    pub fn rerank(&mut self) {
        let parents: Vec<Option<BlockId>> = std::iter::once(None).chain(self.blocks.keys().map(|k| Some(*k))).collect();
        for p in parents { let ids = self.children(p).to_vec(); for (id, r) in ids.iter().zip(rank::spread(ids.len())) { self.blocks.get_mut(id).unwrap().rank = r; } }
    }
    /// Removes a block and everything under it; returns the removed blocks, the block itself first.
    pub fn remove(&mut self, id: BlockId) -> Result<Vec<Block>, TreeError> {
        let parent = self.blocks.get(&id).ok_or(TreeError::UnknownBlock(id))?.parent;
        self.children_mut(parent)?.retain(|c| *c != id);
        let mut out = vec![]; let mut todo = vec![id];
        while let Some(x) = todo.pop() { if let Some(b) = self.blocks.remove(&x) { todo.extend(b.children.iter().rev().copied()); out.push(b); } }
        Ok(out)
    }
    /// Moves a block (with its subtree) to child `index` of `parent`; `index` counts the list as it is AFTER the block
    /// has left it.
    pub fn move_to(&mut self, id: BlockId, parent: Option<BlockId>, index: usize) -> Result<(), TreeError> {
        let (kind, old) = { let b = self.blocks.get(&id).ok_or(TreeError::UnknownBlock(id))?; (b.kind.clone(), b.parent) };
        let mut up = parent;
        while let Some(p) = up { if p == id { return Err(TreeError::WouldNestInItself) } up = self.blocks.get(&p).ok_or(TreeError::UnknownBlock(p))?.parent; }
        self.check(parent, &kind, id)?;
        let at = self.children(old).iter().position(|c| *c == id).unwrap();
        self.children_mut(old)?.remove(at);
        let list = self.children_mut(parent)?;
        if index > list.len() { self.children_mut(old)?.insert(at, id); return Err(TreeError::IndexPastEnd) }
        list.insert(index, id);
        self.blocks.get_mut(&id).unwrap().parent = parent;
        self.place(parent, index, None);
        Ok(())
    }
    /// Puts back a subtree that [`Doc::remove`] returned (the top block first, every block with its `children` list as
    /// it was): the top block becomes child `index` of its recorded parent.
    pub fn insert_tree(&mut self, index: usize, blocks: &[Block]) -> Result<(), TreeError> {
        let Some(top) = blocks.first() else { return Ok(()) };
        for b in blocks { if self.blocks.contains_key(&b.id) { return Err(TreeError::DuplicateId(b.id)) } }
        self.check(top.parent, &top.kind, top.id)?;
        let list = self.children_mut(top.parent)?;
        if index > list.len() { return Err(TreeError::IndexPastEnd) }
        list.insert(index, top.id);
        for b in blocks { self.blocks.insert(b.id, b.clone()); }
        self.place(top.parent, index, Some(top.rank.clone()));
        Ok(())
    }
    pub fn index_of(&self, id: BlockId) -> Option<usize> { let p = self.blocks.get(&id)?.parent; self.children(p).iter().position(|c| *c == id) }
    pub fn set_kind(&mut self, id: BlockId, kind: Kind) -> Result<(), TreeError> {
        let b = self.blocks.get(&id).ok_or(TreeError::UnknownBlock(id))?;
        // a kind change stays within leaves that hold text, or changes a container's own attributes (a column's width,
        // a table's columns, a colour): it never makes or unmakes a container or a media block
        if !((b.kind.is_text() && kind.is_text()) || (b.kind.is_container() && std::mem::discriminant(&b.kind) == std::mem::discriminant(&kind))) { return Err(TreeError::NotAllowed { parent: b.parent, child: id }) }
        self.blocks.get_mut(&id).unwrap().kind = kind; Ok(())
    }
    pub fn set_text(&mut self, id: BlockId, text: impl Into<String>) -> Result<(), TreeError> { self.blocks.get_mut(&id).ok_or(TreeError::UnknownBlock(id))?.text = text.into(); Ok(()) }
    pub fn set_spans(&mut self, id: BlockId, spans: Spans) -> Result<(), TreeError> { self.blocks.get_mut(&id).ok_or(TreeError::UnknownBlock(id))?.spans = spans; Ok(()) }
    /// Every block in document order (a parent before its children).
    pub fn walk(&self) -> Vec<&Block> {
        let mut out = Vec::with_capacity(self.blocks.len());
        let mut stack: Vec<BlockId> = self.root.iter().rev().copied().collect();
        while let Some(id) = stack.pop() { let b = &self.blocks[&id]; out.push(b); stack.extend(b.children.iter().rev().copied()); }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id() -> BlockId { BlockId::new() }
    #[test]
    fn blocks_keep_their_order_and_walk_parent_first() {
        let mut d = Doc::new(); let (a, b, c) = (id(), id(), id());
        d.insert(None, 0, a, Kind::Paragraph, "one").unwrap();
        d.insert(None, 1, c, Kind::Paragraph, "three").unwrap();
        d.insert(None, 1, b, Kind::Heading { level: 2 }, "two").unwrap();
        assert_eq!(d.walk().iter().map(|b| b.text.as_str()).collect::<Vec<_>>(), ["one", "two", "three"]);
        assert_eq!(d.insert(None, 9, id(), Kind::Paragraph, ""), Err(TreeError::IndexPastEnd));
        assert_eq!(d.insert(None, 0, a, Kind::Paragraph, ""), Err(TreeError::DuplicateId(a)));
    }
    /// Row 273: a table may sit in a column; no columns in columns, no tables in tables, no Columns block in a cell.
    #[test]
    fn the_containment_rules_of_the_two_containers_hold() {
        let mut d = Doc::new();
        let (cols, col, table, row, cell) = (id(), id(), id(), id(), id());
        d.insert(None, 0, cols, Kind::Columns, "").unwrap();
        assert!(matches!(d.insert(Some(cols), 0, id(), Kind::Paragraph, "x"), Err(TreeError::NotAllowed { .. })));   // a Columns block holds columns only
        d.insert(Some(cols), 0, col, Kind::column(), "").unwrap();
        d.insert(Some(col), 0, table, Kind::table(), "").unwrap();                                                      // a table in a column
        d.insert(Some(table), 0, row, Kind::row(), "").unwrap();
        d.insert(Some(row), 0, cell, Kind::cell(), "").unwrap();
        d.insert(Some(cell), 0, id(), Kind::Paragraph, "in a cell").unwrap();
        d.insert(Some(cell), 1, id(), Kind::Media { descriptor: "{}".into() }, "").unwrap();
        for bad in [Kind::table(), Kind::Columns] { assert!(matches!(d.insert(Some(cell), 0, id(), bad, ""), Err(TreeError::NotAllowed { .. }))); }
        assert!(matches!(d.insert(Some(col), 0, id(), Kind::Columns, ""), Err(TreeError::NotAllowed { .. })));
        assert!(matches!(d.insert(None, 0, id(), Kind::cell(), ""), Err(TreeError::NotAllowed { .. })));
        let p = id(); d.insert(None, 0, p, Kind::Paragraph, "leaf").unwrap();
        assert!(matches!(d.insert(Some(p), 0, id(), Kind::Paragraph, ""), Err(TreeError::NotAllowed { .. })));         // a leaf holds nothing
    }
    #[test]
    fn a_move_carries_the_subtree_and_refuses_what_an_insert_would() {
        let mut d = Doc::new();
        let (cols, c1, c2, p, t) = (id(), id(), id(), id(), id());
        d.insert(None, 0, cols, Kind::Columns, "").unwrap();
        d.insert(Some(cols), 0, c1, Kind::column(), "").unwrap();
        d.insert(Some(cols), 1, c2, Kind::column(), "").unwrap();
        d.insert(Some(c1), 0, p, Kind::Paragraph, "text").unwrap();
        d.move_to(p, Some(c2), 0).unwrap();
        assert_eq!((d.children(Some(c1)).len(), d.children(Some(c2)), d.get(p).unwrap().parent), (0, &[p][..], Some(c2)));
        assert_eq!(d.move_to(cols, Some(c2), 0), Err(TreeError::WouldNestInItself));
        d.insert(None, 1, t, Kind::table(), "").unwrap();
        d.move_to(t, Some(c1), 0).unwrap();
        let removed = d.remove(cols).unwrap();
        assert_eq!(removed.len(), 5);
        assert!(d.is_empty());
    }
    /// The ranks always sort the way the children are listed, through inserts anywhere, moves, undo-style re-inserts
    /// and a rebalance.
    #[test]
    fn ranks_follow_the_order_of_the_children() {
        fn check(d: &Doc) { for p in std::iter::once(None).chain(d.walk().iter().map(|b| Some(b.id))) { let r: Vec<&Rank> = d.children(p).iter().map(|c| &d.get(*c).unwrap().rank).collect(); assert!(r.windows(2).all(|w| w[0] < w[1]), "{r:?}"); assert!(r.iter().all(|x| x.len() <= rank::MAX_LEN)); } }
        let mut d = Doc::new(); let mut ids = vec![];
        for i in 0..40 { let x = id(); d.insert(None, i, x, Kind::Paragraph, "").unwrap(); ids.push(x); }       // appended
        for _ in 0..40 { d.insert(None, 0, id(), Kind::Paragraph, "").unwrap(); }                                 // prepended
        for _ in 0..600 { d.insert(None, 41, id(), Kind::Paragraph, "").unwrap(); check(&d); }                    // always at one place: forces a rebalance
        d.move_to(ids[39], None, 0).unwrap(); d.move_to(ids[0], None, 300).unwrap(); check(&d);
        let before = d.get(ids[5]).unwrap().rank.clone();
        let at = d.index_of(ids[5]).unwrap();
        let gone = d.remove(ids[5]).unwrap(); d.insert_tree(at, &gone).unwrap();
        assert_eq!(d.get(ids[5]).unwrap().rank, before);                                                          // put back where it was: its old rank
        let order: Vec<BlockId> = d.children(None).to_vec();
        d.rerank(); check(&d);
        assert_eq!(d.children(None), &order[..]);
    }
}
