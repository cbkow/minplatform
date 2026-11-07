//! Step 2b — audio, QCView's design whole (canon row 139; §24's audio row): video is the master, audio follows through
//! a PI servo on the consumption ratio; a stale-marking SPSC ring; a Catmull-Rom drain with 5-ms fades at every break;
//! direct CoreAudio (WASAPI on the box) behind one trait; grains for the shuttle; a probe engine for the waveform.
pub mod ring;
pub mod resampler;
pub mod servo;
pub mod drain;
pub mod routing;
pub mod ffmpeg_audio;
pub mod decoder;
pub mod chunk_reader;
pub mod device;
#[cfg(target_os = "macos")] pub mod coreaudio;
pub mod shuttle;
pub mod waveform;
pub mod player;
pub use player::{AudioPlayer, AudioStats};
pub use routing::RoutingMode;
pub use device::{AudioDevice, NullDevice};
pub use waveform::{ColumnPeak, WaveformProbeEngine};
pub use shuttle::ShuttleAudioEngine;
