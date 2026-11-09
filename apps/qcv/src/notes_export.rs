//! The Notes drawer's Export (canon row 217): a save dialog, the strokes rendered alone on transparent layers (the GPU, here),
//! then the decode / JPEG / HTML or the Markdown copy on a worker; a toast says how it went.
use crate::{annotate, notes_drawer::ExportFormat};
use qcv_notes::export::{Doc, Item};
use std::{path::{Path, PathBuf}, sync::mpsc};

fn tr(s: &str) -> &str { min_i18n::trc("NotesPanel", s) }

/// A PNG's size from its header (no decode).
fn png_dims(p: &Path) -> Option<(u32, u32)> { let f = std::fs::File::open(p).ok()?; let r = png::Decoder::new(std::io::BufReader::new(f)).read_info().ok()?; let i = r.info(); Some((i.width, i.height)) }

impl crate::App {
    pub(crate) fn export_notes(&mut self, fmt: ExportFormat) {
        let Some(media) = self.annot.media.clone() else { return };
        if self.annot.notes.is_empty() { self.toast(tr("No notes to export yet."), min_ui::ToastKind::Notice); return }
        let item = self.project.item(&self.loaded_item).cloned();
        let media_name = item.as_ref().map(|i| i.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| media.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
        let media_path = item.as_ref().map(|i| i.path.clone()).filter(|p| !p.is_empty()).unwrap_or_else(|| media.to_string_lossy().into_owned());
        let now = jiff::Zoned::now();
        let base = format!("{}-{}", qcv_notes::export::sanitize(&media_name), now.strftime("%Y-%m-%d_%H-%M-%S"));
        let dlg = rfd::FileDialog::new().set_file_name(match fmt { ExportFormat::Html => format!("{base}.html"), ExportFormat::Pdf => format!("{base}.pdf"), ExportFormat::Docx => format!("{base}.docx"), ExportFormat::Markdown => base.clone() });
        let picked = match fmt {
            ExportFormat::Html => dlg.set_title(tr("Save HTML…").trim_end_matches('…')).add_filter(tr("HTML (*.html)"), &["html"]).save_file(),
            ExportFormat::Pdf => dlg.set_title(tr("Save PDF…").trim_end_matches('…')).add_filter(tr("PDF (*.pdf)"), &["pdf"]).save_file(),
            ExportFormat::Docx => dlg.set_title(tr("Save DOCX…").trim_end_matches('…')).add_filter(tr("Word document (*.docx)"), &["docx"]).save_file(),
            ExportFormat::Markdown => dlg.set_title(tr("Export folder (Markdown)")).save_file(),
        };
        let Some(picked) = picked else { return };
        // the notes in frame order, each with its pictures
        let dir = qcv_notes::io::images_dir(&qcv_notes::io::notes_dir(&media));
        let mut notes = self.annot.notes.clone(); notes.sort_by_key(|n| n.frame);
        let mut rows: Vec<(qcv_notes::Note, Option<PathBuf>, Option<PathBuf>, Option<Vec<u8>>)> = vec![];
        for n in notes {
            let clean = Some(dir.join(qcv_notes::io::image_filename(&n.timecode))).filter(|p| p.exists());
            let annotated = Some(dir.join(qcv_notes::io::annotated_filename(&n.timecode))).filter(|p| p.exists());
            let ink = if fmt == ExportFormat::Html { clean.as_ref().and_then(|c| self.ink_layer(&n, c)) } else { None };
            rows.push((n, clean, annotated, ink));
        }
        let exported = now.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string();
        let shown = now.strftime("%Y-%m-%d %H:%M").to_string();
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new().name("notes-export".into()).spawn(move || {
            use min_theme::fonts as f;
            let items = rows.into_iter().map(|(note, clean, annotated, ink)| {
                // HTML's frame: the clean one under its ink layer (as JPEG — a review still, ~10× smaller than the PNG), else the annotated PNG flat
                let frame = if fmt == ExportFormat::Html {
                    match (&clean, &annotated) {
                        (Some(c), _) => crate::image_rgba8(c).ok().and_then(|(px, (w, h))| { let rgb: Vec<u8> = px.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect(); min_image::write::jpeg8_bytes(&rgb, w, h, 92).ok() }).map(|b| (b, "image/jpeg")),
                        (None, Some(a)) => std::fs::read(a).ok().map(|b| (b, "image/png")),
                        _ => None,
                    }
                } else { None };
                let ink = if clean.is_some() { ink } else { None };
                let flat = annotated.or(clean);
                // DOCX / PDF: the flat picture as a JPEG q92 (a page shows it ~6 inches wide)
                let page = if matches!(fmt, ExportFormat::Pdf | ExportFormat::Docx) { flat.as_ref().and_then(|p| crate::image_rgba8(p).ok()).and_then(|(px, (w, h))| { let rgb: Vec<u8> = px.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect(); min_image::write::jpeg8_bytes(&rgb, w, h, 92).ok().map(|b| (b, w, h)) }) } else { None };
                Item { note, frame, ink, flat, page }
            }).collect();
            let doc = Doc { media_name, media_path, exported: if fmt == ExportFormat::Markdown { exported } else { shown }, items, brand_png: include_bytes!("../assets/icons/qcview_export.png"),
                fonts: vec![(f::FAMILY_SANS, 400, "normal", f::GEIST_REGULAR), (f::FAMILY_SANS, 700, "normal", f::GEIST_BOLD), (f::FAMILY_SANS, 400, "italic", f::GEIST_ITALIC), (f::FAMILY_MONO, 400, "normal", f::GEIST_MONO_REGULAR), (f::FAMILY_MONO, 700, "normal", f::GEIST_MONO_BOLD)] };
            let r = match fmt {
                ExportFormat::Html => { let p = if picked.extension().is_some_and(|e| e.eq_ignore_ascii_case("html")) { picked } else { picked.with_extension("html") }; std::fs::write(&p, qcv_notes::export::html(&doc)).map(|_| p).map_err(|e| e.to_string()) }
                ExportFormat::Pdf | ExportFormat::Docx => {
                    let ext = if fmt == ExportFormat::Pdf { "pdf" } else { "docx" };
                    let p = if picked.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)) { picked } else { picked.with_extension(ext) };
                    let bytes = if fmt == ExportFormat::Pdf { qcv_notes::export_doc::pdf(&doc) } else { qcv_notes::export_doc::docx(&doc) };
                    bytes.and_then(|b| std::fs::write(&p, b).map_err(|e| e.to_string())).map(|_| p)
                }
                ExportFormat::Markdown => {
                    let parent = picked.parent().map(Path::to_path_buf).unwrap_or_default();
                    let stem = picked.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    qcv_notes::export::markdown(&doc, &parent, &stem)
                }
            };
            match &r { Ok(p) => log::info!("notes exported: {}", p.display()), Err(e) => log::warn!("notes export failed: {e}") }
            let _ = tx.send(r.map(|p| p.to_string_lossy().into_owned()));
        }).ok();
        self.export_rx.push(rx);
    }
    /// The workers' results (every tick, beside the screenshots').
    pub(crate) fn poll_export(&mut self) {
        let mut done = vec![];
        self.export_rx.retain(|rx| match rx.try_recv() { Ok(r) => { done.push(r); false } Err(mpsc::TryRecvError::Empty) => true, Err(_) => false });
        for r in done { match r { Ok(_) => self.toast(tr("Notes exported"), min_ui::ToastKind::Positive), Err(e) => { let t = tr("Export failed: %1").replace("%1", &e); self.toast(&t, min_ui::ToastKind::Negative) } } }
    }
    /// The note's strokes alone on a transparent PNG at its clean frame's size (the HTML's toggleable layer).
    fn ink_layer(&mut self, n: &qcv_notes::Note, clean: &Path) -> Option<Vec<u8>> {
        let ss = qcv_notes::strokes::parse(n.annotation.as_ref());
        if ss.is_empty() { return None }
        let (w, h) = png_dims(clean)?;
        let g = self.gpu.as_mut()?;
        let (dw, _) = g.presenter.engine.display_dims();
        let (dev, q) = (g.presenter.engine.device.clone(), g.presenter.engine.queue.clone());
        if g.thumb_layer.is_none() { g.thumb_layer = min_ui::UiLayer::new(&dev, w, h).ok(); }
        let layer = g.thumb_layer.as_mut()?;
        if layer.size() != (w, h) { layer.resize(&dev, w, h); }
        let scene = layer.scene_mut(); scene.reset();
        let geo = annotate::Geo { rect: [0.0, 0.0, w as f64, h as f64], px_per_src: w as f64 / dw.max(1) as f64 };
        for s in &ss { annotate::draw(scene, s, &geo); }
        layer.render(&dev, &q);
        let px = min_present::readback::read_rgba8(&dev, &q, layer.texture(), w, h);
        min_image::write::png_rgba8_bytes(&px, w, h).ok()
    }
}
