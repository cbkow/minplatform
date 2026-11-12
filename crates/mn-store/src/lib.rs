//! minNotes — the collection file (canon rows 272, 283).
//! [`staging`] is the part that knows nothing of the file's contents: the private working copy, the base copy a merge
//! compares against, publishing by rename, and the copy a crash leaves behind. The database comes next.
pub mod codec;
pub mod file;
pub mod old;
pub mod share;
pub mod staging;

pub use file::{Collection, DocKind, Entry, Error, Canvas, Element};
pub use old::Gate;
