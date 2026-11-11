//! minNotes — the document core (canon rows 273, 275, 281, 283). No interface, no file: a tree of blocks, the flat
//! text view the desktop text element reads, and (next) the changes that undo and merge both work from.
//!
//! TEXT UNITS (row 283, ruling 3): text is UTF-8 and every offset in this crate is a BYTE offset on a char boundary.
//! UTF-16 exists at two edges only — the file's formatting offsets and the desktop text element — and both go through
//! [`text`].
pub mod change;
pub mod flat;
pub mod id;
pub mod inline;
pub mod rank;
pub mod text;
pub mod tree;

pub use change::{Change, ChangeError, History};
pub use flat::FlatText;
pub use id::BlockId;
pub use rank::Rank;
pub use inline::{Mark, MarkKind, Span, Spans};
pub use tree::{Block, Doc, Kind, TreeError};
