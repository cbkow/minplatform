//! `qcv-mxf` — the MXF header-metadata scanner (canon row 137, step 2d). See [`mxf`].
pub mod mxf;
mod mxf_tags;
pub use mxf::{read, Entry, Value};
