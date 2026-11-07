//! A streaming decoder slot (S2): its own thread decodes ahead into a bounded channel and takes serial-numbered seek
//! commands; a frame carrying a stale serial is from before the seek and is dropped by the consumer.
use crate::{ffmpeg::{Decoded, Decoder, OpenOptions}, Error};
use min_present::ColorMeta;
use qcv_timeline::Rate;
use std::time::{Duration, Instant};

enum Cmd { Seek { t: f64, serial: u32 }, Quit }
pub struct Ready { pub serial: u32, pub decoded: Decoded, pub at: Instant }

#[derive(Clone, Debug)]
pub struct SlotInfo { pub width: u32, pub height: u32, pub codec: String, pub pix_fmt: String, pub rate: Rate, pub fps: f64, pub start: f64, pub duration: f64, pub frames: i64, pub meta: ColorMeta, pub hw_active: bool, pub intra: bool }

pub struct Slot { cmd: crossbeam_channel::Sender<Cmd>, pub rx: crossbeam_channel::Receiver<Ready>, pub serial: u32, pub info: SlotInfo }

impl Slot {
    /// Open on the calling thread (errors come back), then decode on a named thread from `src_in` to `src_out` (exclusive; None = the end).
    pub fn open(path: &str, opts: &OpenOptions, src_in: i64, src_out: Option<i64>, depth: usize) -> Result<Slot, Error> {
        let mut dec = Decoder::open(path, opts)?;
        dec.loop_at_eof = false;
        let info = SlotInfo { width: dec.width, height: dec.height, codec: dec.codec.clone(), pix_fmt: dec.pix_fmt.clone(), rate: dec.rate, fps: dec.fps, start: dec.start, duration: dec.duration, frames: dec.frames(), meta: dec.meta, hw_active: dec.hw_active, intra: dec.is_intra() };
        let (ctx, crx) = crossbeam_channel::unbounded::<Cmd>();
        let (tx, rx) = crossbeam_channel::bounded::<Ready>(depth.max(1));
        let (fps, start) = (dec.fps, dec.start);
        let end = src_out.unwrap_or(i64::MAX);
        std::thread::Builder::new().name("qcv-slot".into()).spawn(move || {
            let mut serial = 0u32; let mut eof = false;
            let mut pending: Option<(Decoded, Instant)> = None;
            if src_in > 0 { if let Some((f, _)) = dec.decode_to(src_in as f64 / fps + start) { pending = Some((f, Instant::now())); } }
            loop {
                match crx.try_recv() {
                    Ok(Cmd::Quit) | Err(crossbeam_channel::TryRecvError::Disconnected) => return,
                    Ok(Cmd::Seek { t, serial: s }) => { serial = s; eof = false; pending = dec.decode_to(t).map(|(f, _)| (f, Instant::now())); }
                    Err(crossbeam_channel::TryRecvError::Empty) => {}
                }
                if let Some((f, t)) = pending.take() {
                    match tx.send_timeout(Ready { serial, decoded: f, at: t }, Duration::from_millis(2)) { Ok(()) => {}, Err(crossbeam_channel::SendTimeoutError::Timeout(v)) => { pending = Some((v.decoded, v.at)); continue; }, Err(_) => return }
                }
                if eof { std::thread::sleep(Duration::from_millis(2)); continue; }
                match dec.next() { Some(f) => { if dec.frame_no(f.pts) >= end { eof = true; } else { pending = Some((f, Instant::now())); } }, None => eof = true }
            }
        }).map_err(|e| Error::Open(e.to_string()))?;
        Ok(Slot { cmd: ctx, rx, serial: 0, info })
    }
    /// Seek to a source frame; frames with the previous serial are stale.
    pub fn seek(&mut self, src_frame: i64) -> u32 { self.serial += 1; self.cmd.send(Cmd::Seek { t: src_frame as f64 / self.info.fps + self.info.start, serial: self.serial }).ok(); self.serial }
    pub fn frame_no(&self, pts: f64) -> i64 { ((pts - self.info.start) * self.info.fps).round() as i64 }
}
impl Drop for Slot { fn drop(&mut self) { self.cmd.send(Cmd::Quit).ok(); } }
