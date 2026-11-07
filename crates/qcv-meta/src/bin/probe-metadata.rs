//! `probe-metadata FILE…` — every `VideoMetadata` + `AdobeMetadata` field the family crate reads, for parity checks against
//! QCView's Inspector and exiftool. `--exiftool PATH` runs exiftool with `AdobeMetadataExtractor`'s tag list beside it.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exiftool = args.iter().position(|a| a == "--exiftool").and_then(|i| args.get(i + 1).cloned());
    let files: Vec<&String> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && !(*i > 0 && args[i - 1] == "--exiftool")).map(|(_, a)| a).collect();
    if files.is_empty() { eprintln!("usage: probe-metadata [--exiftool PATH] FILE…"); std::process::exit(2) }
    for f in files {
        let name = std::path::Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let m = qcv_meta::extract(f);
        println!("== {name}");
        println!("{m:#?}");
        let a = qcv_meta::xmp::read_adobe_metadata(f);
        println!("{a:#?}");
        if let Some(tool) = &exiftool {
            let tags = ["-XMP:AeProjectLinkFullPath", "-XMP:WindowsAtomUncProjectPath", "-XMP:MacAtomPosixProjectPath", "-QuickTime:StartTimecode", "-QuickTime:TimeCode", "-QuickTime:CreationDate", "-QuickTime:MediaCreateDate", "-MXF:StartTimecode", "-MXF:TimecodeAtStart", "-XMP:StartTimecode", "-XMP:AltTimecode", "-UserData:TimeCode"];
            match std::process::Command::new(tool).arg("-s").args(tags).arg(f).output() {
                Ok(o) => { println!("-- exiftool:"); print!("{}", String::from_utf8_lossy(&o.stdout)); }
                Err(e) => println!("-- exiftool: {e}"),
            }
        }
    }
}
