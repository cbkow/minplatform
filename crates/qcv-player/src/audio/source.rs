//! `IAudioSource` (QCView `audio/i_audio_source.h`): what the player drives, so the single-stream and the multi-stream
//! decoders are interchangeable. The consumer side (`drain::AudioSource`) is the device callback's; this is the UI thread's.
use super::drain::AudioSource;
use super::routing::RoutingMode;

pub trait AudioSourceCtl: AudioSource {
    fn start(&self);
    fn stop(&self);
    fn seek(&self, seconds: f64);
    fn duration(&self) -> f64;
    fn seconds_since_last_seek(&self) -> f64;
    fn set_routing_mode(&self, mode: RoutingMode);
    fn routing_mode(&self) -> RoutingMode;
    fn set_tempo(&self, tempo: f64);
    fn tempo(&self) -> f64;
    fn source_channels(&self) -> usize;
    fn layout_name(&self) -> String;
    fn peak_levels(&self) -> [f32; 16];
    /// A QCBridge host's audio (QCView `isLive`): no timeline, the player's servo stays off it.
    fn is_live(&self) -> bool { false }
}
