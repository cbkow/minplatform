//! "New minNote" (canon row 587; UFB 1.4.2's "Add Note", `create_date_prefixed_note`): a minNotes collection with one
//! empty note, named `<YYMMDD><letter>_<name>.mnd` - the letter the first of a-z nothing of that day in the folder
//! carries yet, as a dated item's.
//!
//! The file is made by minNotes' own store (`mn-store`), so it is a collection minNotes opens; 1.4.2 wrote the tables
//! itself. It is made in this machine's temporary folder and then copied to its place: SQLite never works on a share
//! (minNotes' own rule), and nothing is written over.
use std::path::{Path, PathBuf};

/// The file's name for a note called `name`, beside `existing` (the names in the folder).
pub fn file_name(existing: &[String], today: &str, name: &str) -> Result<String, String> {
    let name = name.trim(); let name = if name.to_ascii_lowercase().ends_with(".mnd") { name[..name.len() - 4].trim() } else { name };
    ufb_projtype::check_name("note", name)?;
    Ok(format!("{}.mnd", ufb_projtype::dated_name(existing, today, name)?))
}
/// The names in a folder.
pub fn names_in(dir: &Path) -> Vec<String> { std::fs::read_dir(dir).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default() }

/// Makes the note in `dir`. Its place.
pub fn create(dir: &Path, name: &str, today: &str) -> Result<PathBuf, String> {
    if !dir.is_dir() { return Err(format!("Folder does not exist: {}", dir.display())) }
    let file = file_name(&names_in(dir), today, name)?; let to = dir.join(&file);
    let tmp = std::env::temp_dir().join(format!("ufb-note-{}", uuid::Uuid::new_v4())); std::fs::create_dir(&tmp).map_err(|e| format!("The note could not be made: {e}."))?;
    let made = (|| -> Result<(), String> {
        let why = |e: mn_store::Error| format!("The note could not be made: {e:?}.");
        let whole = tmp.join("whole.mnd");
        {
            let mut c = mn_store::Collection::open(&tmp.join("work.mnd")).map_err(why)?;
            let title = file.strip_suffix(".mnd").unwrap_or(&file);
            let note = c.create(None, mn_store::DocKind::Note, title).map_err(why)?;
            let mut d = mn_doc::Doc::new(); d.insert(None, 0, mn_doc::BlockId::new(), mn_doc::Kind::Paragraph, "").map_err(|e| format!("The note could not be made: {e:?}."))?;
            c.save_note(note, &d).map_err(why)?;
            c.publish(&whole).map_err(why)?;
        }
        // (to its place under a name that must be free: nothing is written over; a torn file is not left behind)
        let io = |e: std::io::Error| format!("The note could not be written: {e}.");
        let bytes = std::fs::read(&whole).map_err(io)?;
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&to).map_err(|e| if e.kind() == std::io::ErrorKind::AlreadyExists { format!("\"{file}\" already exists") } else { io(e) })?;
        use std::io::Write;
        if let Err(e) = f.write_all(&bytes).and_then(|_| min_paths::sync::settle(&f)) { drop(f); let _ = std::fs::remove_file(&to); return Err(io(e)) }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    made.map(|_| to)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_note_is_named_by_its_day_and_is_a_collection_minnotes_reads() {
        let n = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(file_name(&[], "261118", " brief ").unwrap(), "261118a_brief.mnd"); assert_eq!(file_name(&n(&["261118A_x.mnd", "261118b_folder", "261117c_y"]), "261118", "brief.MND").unwrap(), "261118c_brief.mnd");
        assert!(file_name(&[], "261118", "").is_err()); assert!(file_name(&[], "261118", "a/b").is_err());
        let dir = std::env::temp_dir().join(format!("ufb-mnote-{}", std::process::id())); let _ = std::fs::remove_dir_all(&dir); std::fs::create_dir_all(&dir).unwrap();
        let a = create(&dir, "brief", "261118").unwrap(); assert_eq!(a.file_name().unwrap(), "261118a_brief.mnd");
        let b = create(&dir, "brief", "261118").unwrap(); assert_eq!(b.file_name().unwrap(), "261118b_brief.mnd", "the day's next letter: nothing is written over");
        assert_eq!(names_in(&dir).len(), 2, "no journal, no work file beside the notes");
        // minNotes' own reader takes it: one note, one empty paragraph
        let c = mn_store::Collection::open_read(&a).expect("a collection"); let top = c.list(None).unwrap(); assert_eq!(top.len(), 1); assert_eq!(top[0].title, "261118a_brief"); assert_eq!(top[0].kind, mn_store::DocKind::Note);
        let _ = c.load_note(top[0].id).unwrap();
        drop(c); let _ = std::fs::remove_dir_all(&dir);
    }
}
