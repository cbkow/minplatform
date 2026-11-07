//! `CoreAudioDevice` (QCView `audio/coreaudio_device.mm`), verbatim: the default-output AudioUnit (AUHAL) with an
//! interleaved f32 stereo stream format at the target rate, a render callback, the device's buffer frame size and the
//! unit's latency property for `buffer_latency_seconds`. Default output only — no device picker (QCView's rule).
#![cfg(target_os = "macos")]
use super::device::{AudioDevice, RenderFn};
use objc2_audio_toolbox::*;
use objc2_core_audio_types::{AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp, kAudioFormatFlagIsFloat, kAudioFormatFlagIsPacked, kAudioFormatLinearPCM};
use std::ffi::c_void;
type OSStatus = i32;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::*};

/// `kAudioDevicePropertyBufferFrameSize` ('fsiz', CoreAudio/AudioHardware.h) — read on the output scope as QCView does.
const K_AUDIO_DEVICE_PROPERTY_BUFFER_FRAME_SIZE: AudioUnitPropertyID = 0x6673697A;

struct Ctx { render: RenderFn, should_play: AtomicBool, written: AtomicU64 }

pub struct CoreAudioDevice { unit: AudioUnit, ctx: *mut Ctx, sample_rate: i32, buffer_frames: u32, running: bool }
unsafe impl Send for CoreAudioDevice {}

unsafe extern "C-unwind" fn render_cb(ref_con: NonNull<c_void>, _flags: NonNull<AudioUnitRenderActionFlags>, _ts: NonNull<AudioTimeStamp>, _bus: u32, frames: u32, io: *mut AudioBufferList) -> OSStatus {
    let ctx = unsafe { &mut *(ref_con.as_ptr() as *mut Ctx) };
    let list = unsafe { &mut *io };
    let n = list.mNumberBuffers as usize;
    let bufs = unsafe { std::slice::from_raw_parts_mut(list.mBuffers.as_mut_ptr(), n) };
    if !ctx.should_play.load(Acquire) || bufs.is_empty() { for b in bufs { unsafe { std::ptr::write_bytes(b.mData as *mut u8, 0, b.mDataByteSize as usize); } } return 0 }
    let out = unsafe { std::slice::from_raw_parts_mut(bufs[0].mData as *mut f32, (frames as usize * 2).min(bufs[0].mDataByteSize as usize / 4)) };
    (ctx.render)(out);
    ctx.written.fetch_add(frames as u64, Relaxed);
    0
}

impl CoreAudioDevice {
    pub fn new(render: RenderFn, sample_rate: i32, buffer_ms: u32) -> Result<CoreAudioDevice, crate::Error> {
        let err = |what: &str, s: OSStatus| crate::Error::Open(format!("CoreAudioDevice: {what} failed ({s})"));
        unsafe {
            let mut desc = AudioComponentDescription { componentType: kAudioUnitType_Output, componentSubType: kAudioUnitSubType_DefaultOutput, componentManufacturer: kAudioUnitManufacturer_Apple, componentFlags: 0, componentFlagsMask: 0 };
            let component = AudioComponentFindNext(std::ptr::null_mut(), NonNull::from(&mut desc));
            if component.is_null() { return Err(crate::Error::Open("CoreAudioDevice: no default output AudioComponent".into())) }
            let mut unit: AudioUnit = std::ptr::null_mut();
            let s = AudioComponentInstanceNew(component, NonNull::from(&mut unit)); if s != 0 { return Err(err("AudioComponentInstanceNew", s)) }
            let fmt = AudioStreamBasicDescription { mSampleRate: sample_rate as f64, mFormatID: kAudioFormatLinearPCM, mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked, mBitsPerChannel: 32, mChannelsPerFrame: 2, mFramesPerPacket: 1, mBytesPerFrame: 8, mBytesPerPacket: 8, mReserved: 0 };
            let s = AudioUnitSetProperty(unit, kAudioUnitProperty_StreamFormat, kAudioUnitScope_Input, 0, &fmt as *const _ as *const c_void, std::mem::size_of::<AudioStreamBasicDescription>() as u32);
            if s != 0 { AudioComponentInstanceDispose(unit); return Err(err("setStreamFormat", s)) }
            let ctx = Box::into_raw(Box::new(Ctx { render, should_play: AtomicBool::new(false), written: AtomicU64::new(0) }));
            let cb = AURenderCallbackStruct { inputProc: Some(render_cb), inputProcRefCon: ctx as *mut c_void };
            let s = AudioUnitSetProperty(unit, kAudioUnitProperty_SetRenderCallback, kAudioUnitScope_Input, 0, &cb as *const _ as *const c_void, std::mem::size_of::<AURenderCallbackStruct>() as u32);
            if s != 0 { AudioComponentInstanceDispose(unit); drop(Box::from_raw(ctx)); return Err(err("setRenderCallback", s)) }
            let s = AudioUnitInitialize(unit); if s != 0 { AudioComponentInstanceDispose(unit); drop(Box::from_raw(ctx)); return Err(err("AudioUnitInitialize", s)) }
            let mut buf_size: u32 = 0; let mut size: u32 = 4;
            AudioUnitGetProperty(unit, K_AUDIO_DEVICE_PROPERTY_BUFFER_FRAME_SIZE, kAudioUnitScope_Output, 0, NonNull::from(&mut buf_size).cast(), NonNull::from(&mut size));
            let buffer_frames = if buf_size > 0 { buf_size } else { sample_rate as u32 * buffer_ms / 1000 };
            log::info!("CoreAudioDevice: initialized ({sample_rate} Hz, 2 ch, buffer={buffer_frames} frames)");
            Ok(CoreAudioDevice { unit, ctx, sample_rate, buffer_frames, running: false })
        }
    }
}
impl AudioDevice for CoreAudioDevice {
    fn start(&mut self) {
        if self.running { return }
        unsafe { (*self.ctx).should_play.store(true, Release); let s = AudioOutputUnitStart(self.unit); if s == 0 { self.running = true; } else { log::warn!("CoreAudioDevice: AudioOutputUnitStart failed ({s})"); } }
    }
    fn stop(&mut self) { if !self.running { return } unsafe { (*self.ctx).should_play.store(false, Release); AudioOutputUnitStop(self.unit); } self.running = false; }
    fn is_running(&self) -> bool { self.running }
    fn sample_rate(&self) -> i32 { self.sample_rate }
    fn buffer_frame_count(&self) -> u32 { self.buffer_frames }
    fn buffer_latency_seconds(&self) -> f64 {
        let mut latency: f64 = 0.0; let mut size: u32 = 8;
        unsafe { AudioUnitGetProperty(self.unit, kAudioUnitProperty_Latency, kAudioUnitScope_Output, 0, NonNull::from(&mut latency).cast(), NonNull::from(&mut size)); }
        latency + self.buffer_frames as f64 / self.sample_rate as f64
    }
    fn frames_rendered(&self) -> u64 { unsafe { (*self.ctx).written.load(Relaxed) } }
}
impl Drop for CoreAudioDevice {
    fn drop(&mut self) { self.stop(); unsafe { AudioUnitUninitialize(self.unit); AudioComponentInstanceDispose(self.unit); drop(Box::from_raw(self.ctx)); } }
}
