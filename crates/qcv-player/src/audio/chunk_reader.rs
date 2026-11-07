//! `AudioChunkReader` (QCView `audio/audio_chunk_reader.cpp`): a SYNCHRONOUS, thread-confined reader of stereo f32
//! 48 kHz at any source second — the shuttle grains and the waveform probes. A read within 2 ms of where the last one
//! ended continues without a seek (the forward-continuation fast path); otherwise it seeks backward, primes the FIFO
//! from the first decoded frame's pts and drops to the target sample.
use super::ffmpeg_audio::{AudioCtx, OUT_CHANNELS, OUT_RATE};
use super::routing::RoutingMode;
use crate::Error;
use ffmpeg_sys_next as ff;

const CONTINUITY_SECONDS: f64 = 0.002;

pub struct AudioChunkReader {
    ctx: AudioCtx,
    path: String,
    fifo: Vec<f32>, fifo_offset: usize, fifo_start_sec: f64, fifo_time_valid: bool,
    eof: bool, pos_sec: f64, pos_valid: bool,
}
unsafe impl Send for AudioChunkReader {}

impl AudioChunkReader {
    pub fn open(path: &str, routing: RoutingMode) -> Result<Option<AudioChunkReader>, Error> {
        let Some(ctx) = AudioCtx::open(path, routing)? else { return Ok(None) };
        Ok(Some(AudioChunkReader { ctx, path: path.to_string(), fifo: vec![], fifo_offset: 0, fifo_start_sec: 0.0, fifo_time_valid: false, eof: false, pos_sec: 0.0, pos_valid: false }))
    }
    pub fn path(&self) -> &str { &self.path }
    pub fn duration(&self) -> f64 { self.ctx.duration }
    fn fifo_clear(&mut self) { self.fifo.clear(); self.fifo_offset = 0; self.fifo_time_valid = false; }
    fn fifo_frames(&self) -> usize { self.fifo.len() / OUT_CHANNELS - self.fifo_offset }
    /// Up to `frames` stereo frames from `src_start_sec` into `dst`; returns the frames delivered (short at EOF).
    pub fn read_at(&mut self, src_start_sec: f64, dst: &mut [f32], frames: usize) -> usize {
        if frames == 0 { return 0 }
        if !self.pos_valid || (src_start_sec - self.pos_sec).abs() > CONTINUITY_SECONDS { if !self.seek_and_prime(src_start_sec) { return 0 } }
        let mut delivered = 0;
        while delivered < frames {
            let mut avail = self.fifo_frames();
            if avail == 0 {
                if self.eof { break }
                if !self.decode_more() { break }
                avail = self.fifo_frames();
                if avail == 0 { continue }
            }
            let n = (frames - delivered).min(avail);
            let from = self.fifo_offset * OUT_CHANNELS;
            dst[delivered * OUT_CHANNELS..(delivered + n) * OUT_CHANNELS].copy_from_slice(&self.fifo[from..from + n * OUT_CHANNELS]);
            delivered += n; self.fifo_offset += n; self.fifo_start_sec += n as f64 / OUT_RATE as f64;
        }
        if self.fifo_offset > 0 { self.fifo.drain(..self.fifo_offset * OUT_CHANNELS); self.fifo_offset = 0; }
        self.pos_sec = src_start_sec + delivered as f64 / OUT_RATE as f64; self.pos_valid = true;
        delivered
    }
    fn seek_and_prime(&mut self, target: f64) -> bool {
        self.ctx.seek(target);
        if !self.ctx.setup_swr() { return false }
        self.fifo_clear(); self.eof = false;
        loop {
            let avail = self.fifo_frames();
            if self.fifo_time_valid && avail > 0 {
                let end = self.fifo_start_sec + avail as f64 / OUT_RATE as f64;
                if end > target {
                    let ahead = (target - self.fifo_start_sec).max(0.0);
                    let drop = ((ahead * OUT_RATE as f64).round() as usize).min(avail);
                    self.fifo_offset += drop; self.fifo_start_sec += drop as f64 / OUT_RATE as f64;
                    self.pos_sec = self.fifo_start_sec; self.pos_valid = true;
                    return true
                }
                self.fifo_clear();
            }
            if self.eof { self.pos_sec = target; self.pos_valid = true; return true }
            if !self.decode_more() && !self.eof { return false }
        }
    }
    fn decode_more(&mut self) -> bool {
        unsafe {
            loop {
                let rc = ff::av_read_frame(self.ctx.fmt, self.ctx.pkt);
                if rc < 0 { self.eof = true; return false }
                if (*self.ctx.pkt).stream_index != self.ctx.stream_idx { ff::av_packet_unref(self.ctx.pkt); continue }
                let src = ff::avcodec_send_packet(self.ctx.cctx, self.ctx.pkt);
                ff::av_packet_unref(self.ctx.pkt);
                if src < 0 { return false }
                let mut produced = false;
                while ff::avcodec_receive_frame(self.ctx.cctx, self.ctx.frame) == 0 {
                    let converted = self.ctx.convert();
                    if converted > 0 {
                        if !self.fifo_time_valid {
                            self.fifo_start_sec = match self.ctx.frame_start_sec() { Some(s) => s, None => if self.pos_valid { self.pos_sec } else { 0.0 } };
                            self.fifo_time_valid = true;
                        }
                        let n = converted as usize * OUT_CHANNELS;
                        let pcm = std::slice::from_raw_parts(self.ctx.swr_buf.as_ptr() as *const f32, n);
                        self.fifo.extend_from_slice(pcm);
                        produced = true;
                    }
                    ff::av_frame_unref(self.ctx.frame);
                }
                if produced { return true }
            }
        }
    }
}
