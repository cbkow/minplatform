//! The colour engines in the app (canon §18, rows 198–204). Until the Color drawer and its presets land (row 199, the
//! preset UX talk owed), a chain is engaged only from the harness: `QCV_MINCOLOR=<name>` with one of [`TEST_CHAINS`].
//! The minColor pipeline builds OFF the main thread at startup (515 ms cold on macOS; QCView built it ahead too) and
//! binds to the slot when ready.
use qcv_color::{ids, Chain, MinColorPass};
use std::sync::mpsc;

/// Harness chains: (name, what it is for).
pub const TEST_CHAINS: &[(&str, &str)] = &[
    ("rec709", "Rec.709 / Rec.1886 in, un-tone-mapped (QCView's minColor default)"),
    ("rec709-drt", "Rec.709 / Rec.1886 in, OpenDRT"),
    ("linear-agx", "Linear Rec.709 in, AgX (renders, EXR)"),
    ("linear-drt", "Linear Rec.709 in, OpenDRT"),
    ("knee", "Linear Rec.709 in, Knee 1000 → 100, un-tone-mapped"),
];

pub fn test_chain(name: &str) -> Option<Chain> {
    let d = Chain::default();
    let lin = ids::OETF_LINEAR;
    Some(match name {
        "rec709" => d,
        "rec709-drt" => Chain { open_drt: 1, ..d },
        "linear-agx" => Chain { in_transfer: lin, agx_on: 1, ..d },
        "linear-drt" => Chain { in_transfer: lin, open_drt: 1, ..d },
        "knee" => Chain { in_transfer: lin, knee_on: 1, ..d },
        _ => return None,
    })
}

/// The chain `QCV_MINCOLOR` asks for (a warning names the choices when it is unknown).
pub fn wanted() -> Option<Chain> {
    let name = std::env::var("QCV_MINCOLOR").ok()?;
    let c = test_chain(&name);
    if c.is_none() { log::warn!("QCV_MINCOLOR={name}: not one of {:?}", TEST_CHAINS.iter().map(|(n, _)| *n).collect::<Vec<_>>()); }
    c
}

/// Build the pass on a worker; the receiver yields it once.
pub fn build_ahead(device: &wgpu::Device, queue: &wgpu::Queue, chain: Chain) -> mpsc::Receiver<MinColorPass> {
    let (tx, rx) = mpsc::channel();
    let (d, q) = (device.clone(), queue.clone());
    std::thread::Builder::new().name("mincolor-build".into()).spawn(move || { let _ = tx.send(MinColorPass::new(&d, &q, chain)); }).ok();
    rx
}
