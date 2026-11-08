//! The colour engines in the app (canon §18, rows 198–204). Until the Color drawer and its presets land (row 199, the
//! preset UX talk owed), a chain is engaged only from the harness: `QCV_MINCOLOR=<name>` with one of [`TEST_CHAINS`], or
//! `QCV_OCIO=<config>|<input>|<view>[|<look>]` against a bundled config folder (`QCV_OCIO_CONFIGS`, else QCView's
//! checkout beside this repo — where the configs ship from is a packaging question).
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
pub fn build_ahead(device: &wgpu::Device, queue: &wgpu::Queue, chain: Chain) -> mpsc::Receiver<Box<dyn min_present::ColorPass + Send>> {
    let (tx, rx) = mpsc::channel();
    let (d, q) = (device.clone(), queue.clone());
    std::thread::Builder::new().name("mincolor-build".into()).spawn(move || { let _ = tx.send(Box::new(MinColorPass::new(&d, &q, chain)) as Box<dyn min_present::ColorPass + Send>); }).ok();
    rx
}

/// Whichever chain the harness asked for, built ahead.
pub fn wanted_ahead(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<mpsc::Receiver<Box<dyn min_present::ColorPass + Send>>> {
    if let Some((cfg, spec)) = ocio_wanted() { return Some(ocio_build_ahead(device, queue, cfg, spec)) }
    wanted().map(|c| build_ahead(device, queue, c))
}

/// The bundled configs' folder (Blender5.2/ ACES_1.3/ ACES_2.0/).
pub fn ocio_configs_dir() -> std::path::PathBuf {
    std::env::var_os("QCV_OCIO_CONFIGS").map(std::path::PathBuf::from).unwrap_or_else(|| {
        let exe = std::env::current_exe().unwrap_or_default();
        // target/<profile>/qcv → the repo → its sibling QCView-Player
        exe.ancestors().nth(4).map(|r| r.join("QCView-Player/assets/OCIO")).unwrap_or_default()
    })
}

/// `QCV_KNEE=<source nits>,<target nits>[,<start>]` turns the Highlight Knee on for an OCIO harness chain.
pub fn knee_wanted() -> qcv_color::Knee {
    let Ok(v) = std::env::var("QCV_KNEE") else { return qcv_color::Knee::default() };
    let n: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    qcv_color::Knee { enabled: n.len() >= 2, source_nits: *n.first().unwrap_or(&1000.0), target_nits: *n.get(1).unwrap_or(&1000.0), start: *n.get(2).unwrap_or(&-1.0) }
}

/// `QCV_OCIO=<config>|<input>|<view>[|<look>]`.
pub fn ocio_wanted() -> Option<(String, qcv_color::ocio::Spec)> {
    let v = std::env::var("QCV_OCIO").ok()?;
    let p: Vec<&str> = v.split('|').collect();
    if p.len() < 3 { log::warn!("QCV_OCIO wants <config>|<input>|<view>[|<look>], got {v:?}"); return None }
    let cfg = ocio_configs_dir().join(p[0]).join("config.ocio");
    Some((cfg.to_string_lossy().into_owned(), qcv_color::ocio::Spec { input: p[1].into(), view: p[2].into(), look: p.get(3).unwrap_or(&"").to_string(), scene_lut: String::new() }))
}

/// Build an OCIO pass on a worker (OCIO + the backend compile); `None` on the channel when it failed (logged).
pub fn ocio_build_ahead(device: &wgpu::Device, queue: &wgpu::Queue, config: String, spec: qcv_color::ocio::Spec) -> mpsc::Receiver<Box<dyn min_present::ColorPass + Send>> {
    let (tx, rx) = mpsc::channel();
    let (d, q) = (device.clone(), queue.clone());
    std::thread::Builder::new().name("ocio-build".into()).spawn(move || {
        match qcv_color::ocio::Config::open(&config).and_then(|c| qcv_color::ocio::OcioPass::new(&d, &q, &c, spec, knee_wanted())) {
            Ok(p) => { let _ = tx.send(Box::new(p) as Box<dyn min_present::ColorPass + Send>); }
            Err(e) => log::error!("OCIO: {e}"),
        }
    }).ok();
    rx
}
