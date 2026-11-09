//! `min-present` — the family's wgpu video presenter (canon §11). Decode produces frames + colour metadata; only the last
//! stage knows what the display can do:
//!
//! ```text
//!  planes / float → [1] decode to LINEAR working RGB → [2] colour chain (a SLOT: `ColorPass`) → [3] display transform → [4] composite
//! ```
//! Stages 1 + 3 are FUSED into one fragment pass when no chain is bound (the YUV fast path — the SDR apps never pay for
//! a working buffer, row 40); with a chain, stage 1 renders into an `Rgba16Float` working texture at the video size,
//! the chain runs, and a blit applies stage 3 with the fit. Stage 4 composites the UI layer (`Engine::set_overlay`: vello's
//! `Rgba8Unorm` output, decoded and placed at SDR reference white, blended premultiplied over the finished video).
//! No FFmpeg and no window toolkit in here: a `Frame` is planes or a platform handle, and the app owns the surface.
pub mod frame;
pub mod engine;
pub mod dual;
pub mod presenter;
pub mod readback;
pub mod capture;
pub mod import;
pub use frame::*;
pub use engine::{ColorPass, Engine, IdentityColorPass, ScopeView, Space, Snapshot, ViewerAids};
pub use presenter::{Output, OutputInfo, Presenter};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("no compatible GPU adapter")] NoAdapter,
    #[error("device request failed: {0}")] Device(String),
    #[error("the surface offers neither the wanted format nor a usable one")] NoFormat,
    #[error("zero-copy import: {0}")] Import(String),
    #[error("nothing is shown yet")] NoFrame,
}
