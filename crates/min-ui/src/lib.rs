//! `min-ui` — the family's UI toolkit (canon §8.9, §16, §25). Born in `apps/qcv` as row 156's first nodes.
//!
//! The toolkit draws ONE vello scene into a UI layer ([`UiLayer`]: an `Rgba8Unorm` texture at the window's physical
//! size) that `min-present` composites over the finished video as stage 4 (`Engine::set_overlay`). The layer is
//! re-rendered only when something changed (S3's on-demand rule); the video pass runs at the display's cadence anyway.
//! Text is parley over the faces `min-theme` bundles ([`Fonts`]); colours, sizes and motion come from `min_theme::tokens`.
pub mod layer;
pub mod fonts;
pub mod paint;
pub mod input;
pub mod anim;
pub mod action_button;
pub mod action_group;
pub mod tooltip;
pub mod text_field;
pub mod popover;
pub mod scrubber;
pub mod slider;
pub mod switch;
pub mod text_button;
pub mod number_field;
pub mod picker;
pub mod section;
pub mod rail;
pub mod dialog;
pub mod menu;
pub mod menu_bar;
pub mod list_column;
pub mod scrollbar;
pub mod toast;
pub mod color_picker;
pub mod text_area;
pub mod progress;
pub mod checkbox;
pub use layer::UiLayer;
pub use fonts::{Family, Fonts};
pub use input::{Button, Event, ImeEvent, Key, KeyEvent, Named};
pub use text_field::{FieldResponse, TextField};
pub use popover::{Placement, Popover};
pub use scrubber::{NoteMark, ScrubCmd, ScrubState, Scrubber, WaveLayer};
pub use slider::Slider;
pub use switch::Switch;
pub use text_button::{TextButton, Variant};
pub use number_field::NumberField;
pub use picker::Picker;
pub use section::Section;
pub use rail::{Rail, RailResponse, Side};
pub use dialog::Dialog;
pub use color_picker::ColorPicker;
pub use text_area::TextArea;
pub use menu::{Menu, MenuItem};
pub use menu_bar::{BarItem, BarKey, BarMenu, MenuBar};
pub use list_column::{ListColumn, ListResponse, ListRow};
pub use scrollbar::ScrollBar;
pub use toast::{Toast, ToastKind};
pub use action_button::{ActionButton, Corners, Response};
pub use action_group::ActionGroup;
pub use tooltip::Tooltips;
