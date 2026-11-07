//! `MetadataService` — ported from `QCView-Player/src/project/metadata_service.{h,cpp}`: the four requests run on a thread
//! pool, the results come back as events the caller drains on its own thread (QCView: queued signals). Row 132 adds
//! latest-wins GENERATIONS: a newer request for the same media id makes the older result drop on arrival (QCView's
//! `QPointer` guard only covered the service's own death).
use crate::{extract, probe_container, probe_rotation, xmp::read_adobe_metadata, AdobeMetadata, VideoMetadata};
use std::{collections::HashMap, sync::{mpsc, Arc, Mutex}, thread};

#[derive(Debug)]
pub enum MetadataEvent { Video(String, VideoMetadata), Adobe(String, AdobeMetadata), Rotation(String, i32), Container(String, VideoMetadata) }

type Job = Box<dyn FnOnce() + Send>;
pub struct MetadataService { tx: mpsc::Sender<Job>, events: mpsc::Receiver<MetadataEvent>, emit: mpsc::Sender<MetadataEvent>, generations: Arc<Mutex<HashMap<String, u64>>>, workers: Vec<thread::JoinHandle<()>> }

impl MetadataService {
    /// `workers` = the pool size (QCView: QThreadPool's global instance).
    pub fn new(workers: usize) -> MetadataService {
        let (tx, rx) = mpsc::channel::<Job>(); let rx = Arc::new(Mutex::new(rx));
        let (emit, events) = mpsc::channel();
        let workers = (0..workers.max(1)).map(|i| { let rx = rx.clone(); thread::Builder::new().name(format!("qcv-meta-{i}")).spawn(move || loop { let job = { let r = rx.lock().unwrap(); r.recv() }; match job { Ok(j) => j(), Err(_) => break } }).expect("metadata worker") }).collect();
        MetadataService { tx, events, emit, generations: Arc::new(Mutex::new(HashMap::new())), workers }
    }
    fn submit(&self, media_id: &str, path: &str, kind: &'static str, run: impl FnOnce(&str) -> MetadataEvent + Send + 'static) {
        if media_id.is_empty() || path.is_empty() { return }
        let key = format!("{kind}:{media_id}");
        let generation = { let mut g = self.generations.lock().unwrap(); let e = g.entry(key.clone()).or_insert(0); *e += 1; *e };
        let (gens, emit, path) = (self.generations.clone(), self.emit.clone(), path.to_string());
        self.tx.send(Box::new(move || {
            let ev = run(&path);
            if gens.lock().unwrap().get(&key).copied() == Some(generation) { emit.send(ev).ok(); }   // latest wins
        })).ok();
    }
    pub fn request_video_extraction(&self, media_id: &str, path: &str) { let id = media_id.to_string(); self.submit(media_id, path, "video", move |p| MetadataEvent::Video(id, extract(p))) }
    pub fn request_adobe_extraction(&self, media_id: &str, path: &str) { let id = media_id.to_string(); self.submit(media_id, path, "adobe", move |p| MetadataEvent::Adobe(id, read_adobe_metadata(p))) }
    pub fn request_rotation_probe(&self, media_id: &str, path: &str) { let id = media_id.to_string(); self.submit(media_id, path, "rotation", move |p| MetadataEvent::Rotation(id, probe_rotation(p))) }
    pub fn request_container_probe(&self, media_id: &str, path: &str) { let id = media_id.to_string(); self.submit(media_id, path, "container", move |p| MetadataEvent::Container(id, probe_container(p))) }
    /// Drain on the caller's thread (QCView's queued signals).
    pub fn try_recv(&self) -> Option<MetadataEvent> { self.events.try_recv().ok() }
    pub fn recv_timeout(&self, d: std::time::Duration) -> Option<MetadataEvent> { self.events.recv_timeout(d).ok() }
}
impl Drop for MetadataService { fn drop(&mut self) { let (tx, _) = mpsc::channel::<Job>(); let old = std::mem::replace(&mut self.tx, tx); drop(old); for w in self.workers.drain(..) { w.join().ok(); } } }
