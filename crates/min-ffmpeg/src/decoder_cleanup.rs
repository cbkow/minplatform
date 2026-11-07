//! Process-wide cleanup thread — ported from `QCView-Player/src/decode/decoder_cleanup_queue.h`. Decoders post their slow
//! shutdown work (FFmpeg context releases, especially the hwframes unref which takes ~50 ms on VideoToolbox) here so the
//! caller's thread doesn't block when a file is closed. The caller must have JOINED the decode thread first.
use std::sync::{mpsc, Mutex, OnceLock};

type Task = Box<dyn FnOnce() + Send>;
static QUEUE: OnceLock<Mutex<mpsc::Sender<Task>>> = OnceLock::new();

pub fn post(task: Task) {
    let tx = QUEUE.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Task>();
        std::thread::Builder::new().name("decoder-cleanup".into()).spawn(move || { while let Ok(t) = rx.recv() { t(); } }).expect("cleanup thread");
        Mutex::new(tx)
    });
    tx.lock().unwrap().send(task).ok();
}
