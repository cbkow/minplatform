//! `probe-metadata FILE…` — every `VideoMetadata` + `AdobeMetadata` field the family crate reads, for parity checks against
//! QCView's Inspector.
fn main() {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() { eprintln!("usage: probe-metadata FILE…"); std::process::exit(2) }
    for f in &files {
        let name = std::path::Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let m = qcv_meta::extract(f);
        println!("== {name}");
        println!("{m:#?}");
        let a = qcv_meta::xmp::read_adobe_metadata(f);
        println!("{a:#?}");
    }
}
