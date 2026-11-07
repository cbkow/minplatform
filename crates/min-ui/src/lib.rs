//! `min-ui` — the family's UI toolkit (canon §8.9, §16, §25). Born in `apps/qcv` as row 156's first nodes.
//!
//! The toolkit draws ONE vello scene into a UI layer ([`UiLayer`]: an `Rgba8Unorm` texture at the window's physical
//! size) that `min-present` composites over the finished video as stage 4 (`Engine::set_overlay`). The layer is
//! re-rendered only when something changed (S3's on-demand rule); the video pass runs at the display's cadence anyway.
//! Text is parley over the faces `min-theme` bundles ([`Fonts`]); colours, sizes and motion come from `min_theme::tokens`.
pub mod layer;
pub mod fonts;
pub mod paint;
pub use layer::UiLayer;
pub use fonts::{Family, Fonts};
