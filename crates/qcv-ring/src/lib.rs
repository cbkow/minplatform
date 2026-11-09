//! QCView's end of the QCBridge contracts (canon §20; rows 101–104 frozen byte-exact, 227–228 the step-1 ruling): the
//! `ringslot` discipline, the three wire layouts (QCAE v3 frames, QCAA v1 audio, QCVO v1 output), the shared mapping
//! (POSIX `shm_open` / Windows `Local\` pagefile sections), the QCAE frame-ring and QCAA audio-ring consumers, the
//! `qcbae://` URLs, and — macOS — the zero-copy Metal wrap of a ring slot. Promoted from spike S4 (its verdict: GO on both
//! OSes against the shipped QCBridgeAE device and QCBridgeN sink); the companions are never touched.
pub mod layout;
pub mod ringslot;
pub mod mapping;
pub mod qcae;
pub mod audio;
pub mod qcvo;
pub mod url;
#[cfg(target_os = "macos")] pub mod mtl;

pub use mapping::process_alive;
