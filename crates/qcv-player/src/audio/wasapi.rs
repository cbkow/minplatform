//! `WasapiAudioDevice` (QCView `audio/wasapi_audio_device.cpp`), verbatim: the default render endpoint in SHARED mode,
//! event-driven, `AUTOCONVERTPCM | SRC_DEFAULT_QUALITY` so the engine takes f32 stereo at our rate, a 10-ms buffer
//! request, the render thread on MMCSS "Pro Audio" (time-critical when MMCSS is unavailable) filling
//! `GetBuffer(bufferFrames − padding)` per event; latency = `GetStreamLatency` + one buffer. Default endpoint only.
#![cfg(windows)]
use super::device::{AudioDevice, RenderFn};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::*};
use std::sync::{Arc, Mutex};
use windows::core::Interface;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::*;
use windows::Win32::Media::Multimedia::WAVE_FORMAT_IEEE_FLOAT;
use windows::Win32::Media::{AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, SetThreadPriority, GetCurrentThread, WaitForSingleObject, THREAD_PRIORITY_TIME_CRITICAL};

struct Com { client: IAudioClient, render: IAudioRenderClient, event: HANDLE }
unsafe impl Send for Com {}
unsafe impl Sync for Com {}

pub struct WasapiAudioDevice {
    com: Arc<Com>, sample_rate: i32, channels: u32, buffer_frames: u32,
    render: Arc<Mutex<Option<RenderFn>>>, running: Arc<AtomicBool>, stop: Arc<AtomicBool>, thread: Option<std::thread::JoinHandle<()>>, written: Arc<AtomicU64>,
}

impl WasapiAudioDevice {
    pub fn new(render: RenderFn, sample_rate: i32, buffer_ms: u32) -> Result<WasapiAudioDevice, crate::Error> {
        let err = |what: &str, e: windows::core::Error| crate::Error::Open(format!("WasapiAudioDevice: {what} failed ({e})"));
        unsafe {
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED); let _ = hr;   // S_FALSE / RPC_E_CHANGED_MODE are fine: COM is up either way
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| err("CoCreateInstance(MMDeviceEnumerator)", e))?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).map_err(|e| err("GetDefaultAudioEndpoint", e))?;
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(|e| err("Activate(IAudioClient)", e))?;
            let channels = 2u32;
            let fmt = WAVEFORMATEX { wFormatTag: WAVE_FORMAT_IEEE_FLOAT as u16, nChannels: channels as u16, nSamplesPerSec: sample_rate as u32, wBitsPerSample: 32, nBlockAlign: (channels * 4) as u16, nAvgBytesPerSec: sample_rate as u32 * channels * 4, cbSize: 0 };
            let flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, buffer_ms as i64 * 10_000, 0, &fmt, None).map_err(|e| err("IAudioClient::Initialize", e))?;
            let event = CreateEventW(None, false, false, None).map_err(|e| err("CreateEvent", e))?;
            client.SetEventHandle(event).map_err(|e| err("SetEventHandle", e))?;
            let buffer_frames = client.GetBufferSize().map_err(|e| err("GetBufferSize", e))?;
            let render_client: IAudioRenderClient = client.GetService().map_err(|e| err("GetService(IAudioRenderClient)", e))?;
            log::info!("WasapiAudioDevice: initialized ({sample_rate} Hz, {channels} ch, buffer={buffer_frames} frames, {buffer_ms} ms target)");
            Ok(WasapiAudioDevice { com: Arc::new(Com { client, render: render_client, event }), sample_rate, channels, buffer_frames, render: Arc::new(Mutex::new(Some(render))), running: Arc::new(AtomicBool::new(false)), stop: Arc::new(AtomicBool::new(false)), thread: None, written: Arc::new(AtomicU64::new(0)) })
        }
    }
}

impl AudioDevice for WasapiAudioDevice {
    fn start(&mut self) {
        if self.running.load(Acquire) { return }
        self.stop.store(false, Release); self.written.store(0, Relaxed);
        unsafe {
            // prime the whole buffer with silence, then start
            if let Ok(p) = self.com.render.GetBuffer(self.buffer_frames) { std::ptr::write_bytes(p, 0, (self.buffer_frames * self.channels * 4) as usize); let _ = self.com.render.ReleaseBuffer(self.buffer_frames, AUDCLNT_BUFFERFLAGS_SILENT.0 as u32); }
            if let Err(e) = self.com.client.Start() { log::warn!("WasapiAudioDevice: IAudioClient::Start failed ({e})"); return }
        }
        self.running.store(true, Release);
        let (com, render, stop, written, frames, channels) = (self.com.clone(), self.render.clone(), self.stop.clone(), self.written.clone(), self.buffer_frames, self.channels);
        self.thread = Some(std::thread::Builder::new().name("qcv-audio-wasapi".into()).spawn(move || unsafe {
            let mut task_index = 0u32;
            let mmcss = AvSetMmThreadCharacteristicsW(windows::core::w!("Pro Audio"), &mut task_index).ok();
            if mmcss.is_none() { let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL); }
            let com_here = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            while !stop.load(Relaxed) {
                let w = WaitForSingleObject(com.event, 2000);   // 2-s safety timeout
                if stop.load(Relaxed) { break }
                if w != WAIT_OBJECT_0 { continue }
                let Ok(padding) = com.client.GetCurrentPadding() else { continue };
                let avail = frames.saturating_sub(padding); if avail == 0 { continue }
                let Ok(p) = com.render.GetBuffer(avail) else { continue };
                let out = std::slice::from_raw_parts_mut(p as *mut f32, (avail * channels) as usize);
                let mut guard = render.lock().unwrap();
                if let Some(r) = guard.as_mut() { r(out); let _ = com.render.ReleaseBuffer(avail, 0); } else { out.fill(0.0); let _ = com.render.ReleaseBuffer(avail, AUDCLNT_BUFFERFLAGS_SILENT.0 as u32); }
                written.fetch_add(avail as u64, Relaxed);
            }
            if com_here { CoUninitialize(); }
            if let Some(h) = mmcss { let _ = AvRevertMmThreadCharacteristics(h); }
        }).expect("wasapi render thread"));
    }
    fn stop(&mut self) {
        if !self.running.swap(false, AcqRel) { return }
        self.stop.store(true, Release);
        unsafe { let _ = SetEvent(self.com.event); }
        if let Some(t) = self.thread.take() { let _ = t.join(); }
        unsafe { let _ = self.com.client.Stop(); let _ = self.com.client.Reset(); }
    }
    fn is_running(&self) -> bool { self.running.load(Acquire) }
    fn sample_rate(&self) -> i32 { self.sample_rate }
    fn buffer_frame_count(&self) -> u32 { self.buffer_frames }
    fn buffer_latency_seconds(&self) -> f64 { let lat = unsafe { self.com.client.GetStreamLatency().unwrap_or(0) }; lat as f64 / 10_000_000.0 + self.buffer_frames as f64 / self.sample_rate as f64 }
    fn frames_rendered(&self) -> u64 { self.written.load(Relaxed) }
}
impl Drop for WasapiAudioDevice { fn drop(&mut self) { self.stop(); unsafe { let _ = CloseHandle(self.com.event); } } }
