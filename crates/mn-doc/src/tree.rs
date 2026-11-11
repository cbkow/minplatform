//! The document: an ordered tree of blocks, each with an id and a parent (row 273). Leaves hold text or one media
//! item; two containers exist — Columns (layout) and Table (data) — with their inner levels (Column; Row, Cell) as
//! blocks of their own so that every part has an id a merge can address.
use std::collections::HashMap;
use crate::id::BlockId;

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
    Column,
    Table,
    Row,
    Cell,
}
impl Kind {
    /// Holds text the caret can enter.
    pub fn is_text(&self) -> bool { matches!(self, Kind::Paragraph | Kind::Heading { .. } | Kind::Quote | Kind::Code { .. } | Kind::Bullet { .. } | Kind::Numbered { .. } | Kind::Task { .. }) }
    pub fn is_container(&self) -> bool { matches!(self, Kind::Columns | Kind::Column | Kind::Table | Kind::Row | Kind::Cell) }
    /// What may sit directly inside what (row 273): a Columns block holds Columns' columns, a Table rows, a Row cells;
    /// a column or a cell holds leaves; a table may sit in a column; no columns in columns, no tables in tables, no
    /// Columns block in a cell. `parent` None is the document itself.
    pub fn may_hold(parent: Option<&Kind>, child: &Kind) -> bool {
        match (parent, child) {
            (Some(Kind::Columns), c) => matches!(c, Kind::Column),
            (Some(Kind::Table), c) => matches!(c, Kind::Row),
            (Some(Kind::Row), c) => matches!(c, Kind::Cell),
            (_, Kind::Column | Kind::Row | Kind::Cell) => false,
            (None, _) => true,
            (Some(Kind::Column), c) => !matches!(c, Kind::Columns),
            (Some(Kind::Cell), c) => !matches!(c, Kind::Columns | Kind::Table),
            (Some(_), _) => false,   // a leaf holds nothing
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub id: BlockId,
    pub parent: Option<BlockId>,
    pub kind: Kind,
    /// UTF-8; empty for media, dividers and containers.
    pub text: String,
    pub children: Vec<BlockId>,
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
        self.blocks.insert(id, Block { id, parent, kind, text: text.into(), children: vec![] });
        Ok(())
    }
    fn check(&self, parent: Option<BlockId>, kind: &Kind, child: BlockId) -> Result<(), TreeError> {
        let pk = match parent { None => None, Some(p) => Some(&self.blocks.get(&p).ok_or(TreeError::UnknownBlock(p))?.kind) };
        if !Kind::may_hold(pk, kind) { return Err(TreeError::NotAllowed { parent, child }) }
        // the two "no X in X" rules reach past the direct parent
        let mut up = parent;
        while let Some(p) = up {
            let b = &self.blocks[&p];
            if matches!((kind, &b.kind), (Kind::Table, Kind::Cell) | (Kind::Columns, Kind::Column) | (Kind::Columns, Kind::Cell)) { return Err(TreeError::NotAllowed { parent, child }) }
            up = b.parent;
        }
        Ok(())
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
        Ok(())
    }
    pub fn index_of(&self, id: BlockId) -> Option<usize> { let p = self.blocks.get(&id)?.parent; self.children(p).iter().position(|c| *c == id) }
    pub fn set_kind(&mut self, id: BlockId, kind: Kind) -> Result<(), TreeError> {
        let b = self.blocks.get(&id).ok_or(TreeError::UnknownBlock(id))?;
        // a kind change stays within leaves that hold text: it never makes or unmakes a container or a media block
        if !(b.kind.is_text() && kind.is_text()) { return Err(TreeError::NotAllowed { parent: b.parent, child: id }) }
        self.blocks.get_mut(&id).unwrap().kind = kind; Ok(())
    }
    pub fn set_text(&mut self, id: BlockId, text: impl Into<String>) -> Result<(), TreeError> { self.blocks.get_mut(&id).ok_or(TreeError::UnknownBlock(id))?.text = text.into(); Ok(()) }
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
        d.insert(Some(cols), 0, col, Kind::Column, "").unwrap();
        d.insert(Some(col), 0, table, Kind::Table, "").unwrap();                                                      // a table in a column
        d.insert(Some(table), 0, row, Kind::Row, "").unwrap();
        d.insert(Some(row), 0, cell, Kind::Cell, "").unwrap();
        d.insert(Some(cell), 0, id(), Kind::Paragraph, "in a cell").unwrap();
        d.insert(Some(cell), 1, id(), Kind::Media { descriptor: "{}".into() }, "").unwrap();
        for bad in [Kind::Table, Kind::Columns] { assert!(matches!(d.insert(Some(cell), 0, id(), bad, ""), Err(TreeError::NotAllowed { .. }))); }
        assert!(matches!(d.insert(Some(col), 0, id(), Kind::Columns, ""), Err(TreeError::NotAllowed { .. })));
        assert!(matches!(d.insert(None, 0, id(), Kind::Cell, ""), Err(TreeError::NotAllowed { .. })));
        let p = id(); d.insert(None, 0, p, Kind::Paragraph, "leaf").unwrap();
        assert!(matches!(d.insert(Some(p), 0, id(), Kind::Paragraph, ""), Err(TreeError::NotAllowed { .. })));         // a leaf holds nothing
    }
    #[test]
    fn a_move_carries_the_subtree_and_refuses_what_an_insert_would() {
        let mut d = Doc::new();
        let (cols, c1, c2, p, t) = (id(), id(), id(), id(), id());
        d.insert(None, 0, cols, Kind::Columns, "").unwrap();
        d.insert(Some(cols), 0, c1, Kind::Column, "").unwrap();
        d.insert(Some(cols), 1, c2, Kind::Column, "").unwrap();
        d.insert(Some(c1), 0, p, Kind::Paragraph, "text").unwrap();
        d.move_to(p, Some(c2), 0).unwrap();
        assert_eq!((d.children(Some(c1)).len(), d.children(Some(c2)), d.get(p).unwrap().parent), (0, &[p][..], Some(c2)));
        assert_eq!(d.move_to(cols, Some(c2), 0), Err(TreeError::WouldNestInItself));
        d.insert(None, 1, t, Kind::Table, "").unwrap();
        d.move_to(t, Some(c1), 0).unwrap();
        let removed = d.remove(cols).unwrap();
        assert_eq!(removed.len(), 5);
        assert!(d.is_empty());
    }
}
