//! minNotes — the collection file (canon rows 272, 283).
//! [`staging`] is the part that knows nothing of the file's contents: the private working copy, the base copy a merge
//! compares against, publishing by rename, and the copy a crash leaves behind. The database comes next.
pub mod staging;
