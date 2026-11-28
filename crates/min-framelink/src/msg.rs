//! The link's messages, version 1. One JSON object a line, in both directions. Pixels and sound never travel here:
//! they land in a slot of a shared block the host made.
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;
/// The pixel format every frame has: RGBA, four 16-bit half floats a pixel, rows packed, the top row first.
pub const FORMAT: &str = "rgba16f";
pub const ENCODING: &str = "scene-linear";
pub const GAMUTS: [&str; 6] = ["rec2020", "rec709", "p3-d65", "aces-ap0", "aces-ap1", "camera"];

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Purpose { Frames, Sound }

/// Host to provider. Every message with an `id` is answered exactly once by a message carrying that `id`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum ToProvider {
    /// The first message. Answered by the provider's `hello`.
    Hello { version: u32, #[serde(default)] host: String },
    /// Answered by `clip`, or by `error`.
    Open { id: u64, path: String, #[serde(default, skip_serializing_if = "Option::is_none")] gamut: Option<String> },
    /// A block the host made: `slots` slots of `slot_bytes` each, one after the other. Answered by `attached`.
    Attach { id: u64, block: u32, name: String, slots: u32, slot_bytes: u64, #[serde(rename = "use")] purpose: Purpose },
    /// Answered by `detached`, after every request naming the block has been answered.
    Detach { id: u64, block: u32 },
    /// `scale` is 1, 2, 4 or 8. Answered by `frame-done`, always, whatever became of it.
    Frame { id: u64, clip: u32, frame: u64, scale: u32, generation: u64, block: u32, slot: u32 },
    /// Every `frame` of this clip carrying an older generation is answered as dropped from now on. No answer of its own.
    Jump { clip: u32, generation: u64 },
    /// `count` sample frames from `first` on. Answered by `sound-done`, or by `error`.
    Sound { id: u64, clip: u32, first: u64, count: u32, block: u32, slot: u32 },
    /// The settings the clip's source offers and their present values. Answered by `controls`.
    Controls { id: u64, clip: u32 },
    /// Have the provider show its settings panel for the clip. The settings are the provider's: a host never sets one.
    /// Answered by `settings-shown`, or by `error` where the provider has no screen to show it on.
    ShowSettings { id: u64, clip: u32 },
    /// Answered by `closed`, after every request for the clip has been answered.
    Close { id: u64, clip: u32 },
    /// The provider ends. So it does when its input closes.
    Bye,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FrameResult { Made, Dropped, Failed }

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoundFacts { pub bits: u32, pub channels: u32, pub rate: u32, pub samples: u64 }

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Size { pub scale: u32, pub width: u32, pub height: u32, /** what a slot must hold for it */ pub bytes: u64 }

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Picture { pub format: String, pub encoding: String, pub gamut: String, pub gamuts: Vec<String>, pub sizes: Vec<Size> }

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClipFacts {
    pub id: u64,
    /// The number the host names the clip by from now on.
    pub clip: u32,
    pub width: u32,
    pub height: u32,
    /// Frames a second as a fraction.
    pub rate: (u32, u32),
    pub frames: u64,
    pub timecode: String,
    pub timecode_base_frame: u32,
    pub drop_frame: bool,
    pub sound: Option<SoundFacts>,
    pub camera: String,
    pub metadata: Vec<(String, String)>,
    pub picture: Picture,
}

/// One setting of a clip's source. `kind` is `choice` (one of `options`, each a value and the text shown for it), `range`
/// (`min` to `max` in steps of `step`) or `switch` (0 or 1).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ControlFacts {
    pub id: String,
    pub label: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")] pub options: Vec<(f64, String)>,
    #[serde(default)] pub min: f64,
    #[serde(default)] pub max: f64,
    #[serde(default)] pub step: f64,
    /// What the clip was shot with.
    pub as_shot: f64,
    /// What frames are decoded with now.
    pub value: f64,
}

/// Provider to host.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum ToHost {
    Hello { version: u32, provider: String, decoders: Vec<String>, /** slots of a frames block are whole multiples of it */ page: u64 },
    Clip(ClipFacts),
    Attached { id: u64, block: u32 },
    Detached { id: u64, block: u32 },
    /// The slot is the host's again. `width` and `height` are stated when the frame was made.
    FrameDone { id: u64, result: FrameResult, #[serde(default)] width: u32, #[serde(default)] height: u32, #[serde(default, skip_serializing_if = "String::is_empty")] message: String },
    /// Sample frames written to the slot: fewer than asked at the clip's end.
    SoundDone { id: u64, samples: u32 },
    Closed { id: u64, clip: u32 },
    Controls { id: u64, clip: u32, controls: Vec<ControlFacts> },
    SettingsShown { id: u64, clip: u32 },
    /// The user changed the clip's settings in the provider. Frames made from now on carry them: a host asks again for
    /// those it shows, with a new generation. Not an answer to anything.
    SettingsChanged { clip: u32 },
    /// A request that could not be done (`id` says which), or a line that was not understood. After `fatal` the provider ends.
    Error { #[serde(default, skip_serializing_if = "Option::is_none")] id: Option<u64>, message: String, #[serde(default)] fatal: bool },
}
