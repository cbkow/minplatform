//! Annotation exports (canon row 217, ruled 2026-11-08). Markdown = QCView's (`annotation_exporter.cpp`): a folder
//! `<name>/<name>.md` + `images/`, one section per note. HTML = the fancy one: the family look (minNotes' export — Geist
//! embedded, CSS tokens, the quiet mono header) with Spectrum 2's greys, a SYNOPSIS TABLE as the document (no per-note list
//! under it), and minNotes' lightbox for the full frames, the strokes on their own layer over the clean frame (a toggle
//! hides them). The caller prepares the images (it owns the GPU): the frame, the ink layer, their sizes.
use crate::note::Note;
use std::path::{Path, PathBuf};

/// One note and its pictures (encoded files' bytes).
pub struct Item {
    pub note: Note,
    /// the clean frame (HTML: a JPEG or PNG; `frame_mime` says), or the annotated PNG when there is no clean one
    pub frame: Option<(Vec<u8>, &'static str)>,
    /// the strokes alone on a transparent PNG at the frame's size (HTML's toggleable layer)
    pub ink: Option<Vec<u8>>,
    /// the file Markdown / DOCX / PDF use: the annotated PNG, else the clean one (QCView)
    pub flat: Option<PathBuf>,
}

pub struct Doc {
    pub media_name: String,
    pub media_path: String,
    /// local time, ISO 8601 with offset (QCView's `Qt::ISODate`)
    pub exported: String,
    pub items: Vec<Item>,
    pub brand_png: &'static [u8],
    /// (family, weight, style, ttf bytes) for the HTML's @font-face
    pub fonts: Vec<(&'static str, u16, &'static str, &'static [u8])>,
}

/// QCView `headingFor`: "01:02:03:04" + "frame 1500" for a file; the capture time + "TC …" for a stream note.
pub fn heading(n: &Note) -> (String, String) {
    if !n.stream.is_empty() || !n.captured_at.is_empty() {
        let main = if n.captured_at.len() >= 23 { n.captured_at[..23].replace('T', " ") } else if n.captured_at.is_empty() { n.timecode.clone() } else { n.captured_at.replace('T', " ") };
        return (main, if n.host_timecode.is_empty() { String::new() } else { format!("TC {}", n.host_timecode) })
    }
    (n.timecode.clone(), format!("frame {}", n.frame))
}

pub fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char); s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' }); s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}
fn esc(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;") }

/// QCView's Markdown sanitizer: letters, digits, `_ - . space`; the rest → `_`.
pub fn sanitize(s: &str) -> String { s.chars().map(|c| if c.is_ascii_alphanumeric() || "_-. ".contains(c) { c } else { '_' }).collect::<String>().trim().to_string() }

/// QCView's Markdown folder: `<dir>/<base>/<base>.md` + `images/` (the brand icon and each note's picture copied in).
/// Returns the .md's path.
pub fn markdown(d: &Doc, dir: &Path, base: &str) -> Result<PathBuf, String> {
    let mut safe = sanitize(base); if safe.is_empty() { safe = "notes".into(); }
    let folder = dir.join(&safe); let images = folder.join("images");
    std::fs::create_dir_all(&images).map_err(|e| format!("{}: {e}", images.display()))?;
    let mut md: Vec<String> = vec![];
    if std::fs::write(images.join("qcview_export.png"), d.brand_png).is_ok() { md.push("![](images/qcview_export.png)".into()); md.push(String::new()); }
    md.push(format!("# {}", if d.media_name.is_empty() { "Notes" } else { &d.media_name })); md.push(String::new());
    md.push(format!("Exported: {}", d.exported));
    if !d.media_path.is_empty() { md.push(format!("Source: `{}`", d.media_path)); }
    md.push(String::new());
    for it in &d.items {
        let n = &it.note;
        let rel = it.flat.as_ref().and_then(|p| { let name = p.file_name()?.to_string_lossy().into_owned(); std::fs::copy(p, images.join(&name)).ok().map(|_| format!("images/{name}")) });
        let (main, sub) = heading(n);
        md.push(format!("## {main}{}{}", if sub.is_empty() { String::new() } else { format!(" ({sub})") }, if n.addressed { " \u{2705}" } else { "" }));
        md.push(String::new());
        if let Some(rel) = rel {
            md.push(format!("![{}]({rel})", n.timecode)); md.push(String::new());
            if !n.ocio.is_empty() { md.push(format!("*{}*", n.ocio)); md.push(String::new()); }
            if !n.viewer.is_empty() { md.push(format!("*{}*", n.viewer)); md.push(String::new()); }
        }
        if !n.text.trim().is_empty() { md.push(n.text.clone()); md.push(String::new()); }
        md.push(String::new());
    }
    let path = folder.join(format!("{safe}.md"));
    std::fs::write(&path, md.join("\n")).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// The single-file HTML: header, the synopsis table, the lightbox.
pub fn html(d: &Doc) -> String {
    let title = if d.media_name.is_empty() { "Notes".to_string() } else { d.media_name.clone() };
    let total = d.items.len(); let done = d.items.iter().filter(|i| i.note.addressed).count();
    let mut o = String::with_capacity(64 * 1024);
    o += "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n";
    o += &format!("<title>{}</title>\n<style>\n", esc(&title));
    for (fam, w, st, bytes) in &d.fonts { o += &format!("@font-face{{font-family:'{fam}';font-weight:{w};font-style:{st};font-display:block;src:url(data:font/ttf;base64,{}) format('truetype')}}\n", b64(bytes)); }
    o += CSS;
    o += "</style></head><body>\n<main>\n<header class=\"doc\">";
    o += &format!("<img class=\"brand\" src=\"data:image/png;base64,{}\" alt=\"\">", b64(d.brand_png));
    o += &format!("<div class=\"titles\"><h1>{}</h1><div class=\"meta\">", esc(&title));
    o += &format!("<span>Exported {}</span>", esc(&d.exported));
    if !d.media_path.is_empty() { o += &format!("<span class=\"path\">{}</span>", esc(&d.media_path)); }
    o += "</div></div>";
    o += &format!("<div class=\"filters\" role=\"tablist\"><button class=\"on\" data-f=\"all\">All <b>{total}</b></button><button data-f=\"open\">Open <b>{}</b></button><button data-f=\"done\">Addressed <b>{done}</b></button></div>", total - done);
    o += "</header>\n";
    if d.items.is_empty() { o += "<p class=\"empty\">No notes.</p>\n"; }
    else {
        o += "<table class=\"syn\"><colgroup><col class=\"c-n\"><col class=\"c-f\"><col class=\"c-t\"><col class=\"c-x\"><col class=\"c-s\"></colgroup>\n<thead><tr><th>#</th><th>Frame</th><th>Timecode</th><th>Note</th><th>Status</th></tr></thead><tbody>\n";
        for (i, it) in d.items.iter().enumerate() {
            let n = &it.note; let (main, sub) = heading(n);
            o += &format!("<tr id=\"n{}\" class=\"{}\"><td class=\"num\">{}</td><td class=\"fr\">", i + 1, if n.addressed { "done" } else { "open" }, i + 1);
            match &it.frame {
                Some((bytes, mime)) => {
                    o += &format!("<div class=\"thumb\" tabindex=\"0\" data-tc=\"{}\" data-sub=\"{}\"><img class=\"base\" src=\"data:{mime};base64,{}\" alt=\"{}\">", esc(&main), esc(&sub), b64(bytes), esc(&n.timecode));
                    if let Some(ink) = &it.ink { o += &format!("<img class=\"ink\" src=\"data:image/png;base64,{}\" alt=\"\">", b64(ink)); }
                    o += "</div>";
                }
                None => o += "<div class=\"nothumb\"></div>",
            }
            o += &format!("</td><td class=\"tc\"><div class=\"main\">{}</div>{}</td>", esc(&main), if sub.is_empty() { String::new() } else { format!("<div class=\"sub\">{}</div>", esc(&sub)) });
            o += &format!("<td class=\"tx\"><div class=\"text\">{}</div>", if n.text.trim().is_empty() { "<span class=\"none\">No text</span>".to_string() } else { esc(n.text.trim_end()) });
            for r in [&n.ocio, &n.viewer] { if !r.is_empty() { o += &format!("<div class=\"rec\">{}</div>", esc(r)); } }
            o += &format!("</td><td class=\"st\"><span class=\"chip {}\">{}</span></td></tr>\n", if n.addressed { "done" } else { "open" }, if n.addressed { "Addressed" } else { "Open" });
        }
        o += "</tbody></table>\n";
    }
    o += "</main>\n";
    o += LIGHTBOX;
    o += "</body></html>\n";
    o
}

/// Spectrum 2's dark greys (min-theme): base #111 · layer-1 #1b1b1b · layer-2 #222 · gray-100…900; blue-800 the accent,
/// blue-400 the selected fill (the Media bin's loaded row); green for Addressed. Prints light.
const CSS: &str = r#"
:root{--base:#111111;--l1:#1b1b1b;--l2:#222222;--g100:#2c2c2c;--g200:#323232;--g300:#393939;--g400:#444444;--g500:#6d6d6d;--g600:#8a8a8a;--g700:#afafaf;--g800:#dbdbdb;--g900:#f2f2f2;
--accent:#4069fd;--accent-hi:#6995fe;--sel:#122d9a;--pos:#099d59;--r:8px;--rs:4px;
--sans:Geist,-apple-system,BlinkMacSystemFont,"Segoe UI",Inter,Roboto,sans-serif;--mono:'Geist Mono',ui-monospace,Menlo,Consolas,monospace}
*{box-sizing:border-box}
html{background:var(--base)}
body{margin:0;color:var(--g800);font:400 14px/20px var(--sans);-webkit-font-smoothing:antialiased}
::selection{background:var(--sel);color:var(--g900)}
main{padding:40px 48px 96px;max-width:1600px;margin:0 auto}
header.doc{display:flex;align-items:center;gap:16px;padding-bottom:20px;margin-bottom:24px;border-bottom:1px solid var(--g300)}
header.doc .brand{width:40px;height:40px;flex:none;filter:invert(1)}   /* the black mark, inverted for the dark page (the user 2026-11-08) */
header.doc .titles{min-width:0;flex:1}
h1{margin:0;font-size:24px;line-height:32px;font-weight:400;color:var(--g900);overflow-wrap:anywhere}
.meta{display:flex;flex-wrap:wrap;gap:4px 16px;margin-top:2px;font:12px/18px var(--mono);color:var(--g600)}
.meta .path{word-break:break-all}
.filters{display:flex;gap:2px;padding:2px;background:var(--l2);border:1px solid var(--g300);border-radius:var(--r);flex:none}
.filters button{font:500 13px/18px var(--sans);color:var(--g700);background:transparent;border:0;border-radius:6px;padding:5px 10px;cursor:pointer;transition:background-color 100ms,color 100ms}
.filters button b{font:12px var(--mono);color:var(--g600);margin-left:4px;font-weight:400}
.filters button:hover{background:var(--g200);color:var(--g900)}
.filters button.on{background:var(--g400);color:var(--g900)}
.filters button.on b{color:var(--g800)}
table.syn{width:100%;border-collapse:separate;border-spacing:0;background:var(--l1);border:1px solid var(--g300);border-radius:var(--r);overflow:hidden}
col.c-n{width:44px}col.c-f{width:272px}col.c-t{width:150px}col.c-s{width:116px}
th{font:500 12px/16px var(--sans);color:var(--g600);text-align:left;padding:10px 12px;background:var(--l2);border-bottom:1px solid var(--g300);text-transform:none}
td{padding:12px;vertical-align:top;border-bottom:1px solid var(--g200)}
tbody tr:last-child td{border-bottom:0}
tbody tr{transition:background-color 100ms}
tbody tr:hover{background:var(--l2)}
tbody tr:target{background:var(--l2);box-shadow:inset 3px 0 0 var(--accent)}
td.num{font:12px/20px var(--mono);color:var(--g500);text-align:right}
.thumb{position:relative;width:248px;border-radius:var(--rs);overflow:hidden;background:#000;cursor:zoom-in;outline:none;line-height:0}
.thumb img{display:block;width:100%;height:auto}
.thumb .ink{position:absolute;inset:0;pointer-events:none}
.thumb:focus-visible{box-shadow:0 0 0 2px var(--accent)}
.thumb::after{content:"";position:absolute;inset:0;border:1px solid rgba(255,255,255,.08);border-radius:var(--rs);pointer-events:none}
.nothumb{width:248px;aspect-ratio:16/9;border-radius:var(--rs);background:var(--l2)}
html.noink .ink,html.noink #lb .stage .ink{display:none}   /* the lightbox's img rule outranks the plain one */
td.tc .main{font:500 13px/20px var(--mono);color:var(--g900);white-space:nowrap}
td.tc .sub{font:12px/18px var(--mono);color:var(--g600)}
.text{white-space:pre-wrap;overflow-wrap:anywhere;color:var(--g800)}
.text .none{color:var(--g500);font-style:italic}
.rec{margin-top:8px;font:12px/16px var(--mono);color:var(--g600);overflow-wrap:anywhere}
.rec+.rec{margin-top:2px}
.chip{display:inline-flex;align-items:center;gap:6px;height:24px;padding:0 9px;border-radius:12px;font:500 12px/16px var(--sans);white-space:nowrap;border:1px solid var(--g400);color:var(--g700)}
.chip::before{content:"";width:6px;height:6px;border-radius:50%;background:var(--g500)}
.chip.done{color:var(--g800);border-color:rgba(9,157,89,.55);background:rgba(9,157,89,.12)}
.chip.done::before{background:var(--pos)}
tr.done .text,tr.done td.tc .main{color:var(--g600)}
tr.hide{display:none}
.empty{color:var(--g600)}
/* the lightbox (minNotes' export): click a frame; ‹ › / arrows step, Esc / the backdrop closes, A hides the strokes */
#lb{position:fixed;inset:0;background:rgba(0,0,0,.94);z-index:99;display:flex;flex-direction:column;align-items:center;justify-content:center;opacity:0;visibility:hidden;pointer-events:none;transition:opacity 160ms ease,visibility 0s linear 160ms}
#lb.open{opacity:1;visibility:visible;pointer-events:auto;transition:opacity 160ms ease}
@keyframes lbin{from{opacity:0}to{opacity:1}}
#lb .stage{position:relative;line-height:0;animation:lbin 160ms ease}
#lb .stage img{display:block;max-width:90vw;max-height:calc(100vh - 150px)}
#lb .stage .ink{position:absolute;inset:0;width:100%;height:100%;max-width:none;max-height:none}
#lb .cap{margin-top:14px;max-width:min(90vw,960px);display:flex;gap:16px;align-items:baseline;color:var(--g800)}
#lb .cap .tc{font:500 13px var(--mono);color:var(--g900);white-space:nowrap}
#lb .cap .sub{font:12px var(--mono);color:var(--g600);white-space:nowrap}
#lb .cap .txt{white-space:pre-wrap;overflow-wrap:anywhere;max-height:72px;overflow:auto}
#lb button{position:fixed;z-index:100;background:rgba(27,27,27,.8);color:var(--g800);border:1px solid var(--g300);border-radius:var(--r);cursor:pointer;padding:0;display:flex;align-items:center;justify-content:center;transition:background-color 100ms,color 100ms}
#lb button:hover{background:var(--g300);color:var(--g900)}
#lb button svg{display:block}
#lb .prev,#lb .next{top:50%;transform:translateY(-50%);width:44px;height:64px}
#lb .prev{left:16px}#lb .next{right:16px}
#lb .close{top:14px;right:16px;width:40px;height:40px}
#lb .strokes{top:14px;right:64px;height:40px;padding:0 12px;gap:8px;font:500 13px var(--sans)}
#lb .strokes.off{color:var(--g600)}
#lb .strokes kbd{font:11px var(--mono);color:var(--g600);border:1px solid var(--g400);border-radius:3px;padding:0 4px}
#lb .cnt{position:fixed;top:24px;left:50%;transform:translateX(-50%);font:12px var(--mono);color:var(--g600)}
@media (max-width:900px){main{padding:24px 16px 64px}header.doc{flex-wrap:wrap}col.c-f{width:140px}.thumb,.nothumb{width:128px}col.c-t{width:110px}}
@media print{
:root{--base:#fff;--l1:#fff;--l2:#f4f4f4;--g200:#e6e6e6;--g300:#dadada;--g400:#c6c6c6;--g500:#8f8f8f;--g600:#6d6d6d;--g700:#464646;--g800:#222;--g900:#000}
#lb,.filters{display:none!important}header.doc .brand{filter:none}main{padding:0;max-width:none}tr{break-inside:avoid}tr.hide{display:table-row}tbody tr:hover{background:none}}
"#;

const LIGHTBOX: &str = r#"<div id="lb" aria-hidden="true"><span class="cnt"></span>
<button class="strokes" title="Show or hide the annotations (A)">Annotations <kbd>A</kbd></button>
<button class="close" aria-label="Close"><svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg></button>
<button class="prev" aria-label="Previous"><svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 5 8 12l7 7"/></svg></button>
<div class="stage"></div><div class="cap"><span class="tc"></span><span class="sub"></span><span class="txt"></span></div>
<button class="next" aria-label="Next"><svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 5l7 7-7 7"/></svg></button></div>
<script>
(function(){
var root=document.documentElement,lb=document.getElementById('lb'),stage=lb.querySelector('.stage'),cnt=lb.querySelector('.cnt'),cap=lb.querySelector('.cap'),sb=lb.querySelector('.strokes'),cur=-1;
try{if(localStorage.getItem('qcv.noink')==='1')root.classList.add('noink')}catch(e){}
function paintSb(){sb.classList.toggle('off',root.classList.contains('noink'))}paintSb();
function toggleInk(){root.classList.toggle('noink');try{localStorage.setItem('qcv.noink',root.classList.contains('noink')?'1':'0')}catch(e){}paintSb()}
function rows(){return [].slice.call(document.querySelectorAll('tbody tr')).filter(function(r){return !r.classList.contains('hide')&&r.querySelector('.thumb')})}
var list=[];
function show(i){list=rows();if(!list.length)return;cur=(i+list.length)%list.length;var r=list[cur],t=r.querySelector('.thumb');
stage.innerHTML='';[].forEach.call(t.querySelectorAll('img'),function(im){var o=document.createElement('img');o.src=im.src;o.className=im.className;stage.appendChild(o)});
stage.style.animation='none';void stage.offsetWidth;stage.style.animation='';
cap.querySelector('.tc').textContent=t.dataset.tc;cap.querySelector('.sub').textContent=t.dataset.sub;cap.querySelector('.txt').textContent=r.querySelector('.text').textContent;
sb.style.display=t.querySelector('.ink')?'':'none';
cnt.textContent=(cur+1)+' / '+list.length;lb.classList.add('open');lb.setAttribute('aria-hidden','false')}
function close(){lb.classList.remove('open');lb.setAttribute('aria-hidden','true');cur=-1}
[].forEach.call(document.querySelectorAll('.thumb'),function(t){function go(){var l=rows();show(l.indexOf(t.closest('tr')))}t.addEventListener('click',go);t.addEventListener('keydown',function(e){if(e.key==='Enter'||e.key===' '){e.preventDefault();go()}})});
lb.addEventListener('click',function(e){if(e.target===lb||e.target===stage.parentNode)close()});
lb.querySelector('.prev').addEventListener('click',function(){show(cur-1)});
lb.querySelector('.next').addEventListener('click',function(){show(cur+1)});
lb.querySelector('.close').addEventListener('click',close);
sb.addEventListener('click',toggleInk);
document.addEventListener('keydown',function(e){if(e.metaKey||e.ctrlKey||e.altKey)return;
if(e.key==='a'||e.key==='A'){toggleInk();e.preventDefault();return}
if(cur<0)return;if(e.key==='Escape')close();else if(e.key==='ArrowLeft')show(cur-1);else if(e.key==='ArrowRight')show(cur+1);else return;e.preventDefault()});
[].forEach.call(document.querySelectorAll('.filters button'),function(b){b.addEventListener('click',function(){
[].forEach.call(document.querySelectorAll('.filters button'),function(x){x.classList.toggle('on',x===b)});var f=b.dataset.f;
[].forEach.call(document.querySelectorAll('tbody tr'),function(r){r.classList.toggle('hide',f!=='all'&&!r.classList.contains(f))})})});
})();
</script>
"#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn base64_matches_the_rfc_vectors() {
        for (i, o) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] { assert_eq!(b64(i.as_bytes()), o); }
    }
    #[test]
    fn the_html_escapes_and_counts() {
        let n = |tc: &str, t: &str, a: bool| Item { note: Note { timecode: tc.into(), frame: 12, text: t.into(), addressed: a, ..Default::default() }, frame: Some((vec![1, 2, 3], "image/jpeg")), ink: None, flat: None };
        let d = Doc { media_name: "a<b>".into(), media_path: "/x/y.mov".into(), exported: "2026-11-08T21:00:00-05:00".into(), items: vec![n("00:00:00:12", "fix <this>", false), n("00:00:01:00", "ok", true)], brand_png: b"png", fonts: vec![] };
        let h = html(&d);
        assert!(h.contains("<title>a&lt;b&gt;</title>") && h.contains("fix &lt;this&gt;") && h.contains("Open <b>1</b>") && h.contains("Addressed <b>1</b>"));
        assert!(h.contains("data:image/jpeg;base64,AQID") && !h.contains("class=\"ink\""));
        assert_eq!(heading(&d.items[0].note), ("00:00:00:12".to_string(), "frame 12".to_string()));
    }
}

#[cfg(test)]
mod sample {
    /// `QCV_EXPORT_SAMPLE=<out.html> cargo test -p qcv-notes sample_html -- --ignored`: a synthetic export (SVG frames, no
    /// client footage) for a look in a browser.
    #[test]
    #[ignore]
    fn sample_html() {
        use super::*;
        let Some(out) = std::env::var_os("QCV_EXPORT_SAMPLE") else { return };
        let svg = |h: u32| format!("<svg xmlns='http://www.w3.org/2000/svg' width='1920' height='1080'><defs><linearGradient id='g' x2='1' y2='1'><stop offset='0' stop-color='hsl({h},45%,30%)'/><stop offset='1' stop-color='hsl({},40%,12%)'/></linearGradient></defs><rect width='1920' height='1080' fill='url(#g)'/><circle cx='1300' cy='420' r='220' fill='hsl({h},30%,55%)'/></svg>", h + 60).into_bytes();
        let texts = ["Flicker on the left edge — check the comp's matte, it pops for two frames.", "", "Logo too close to title safe.\nMove 40 px in and re-render.", "Colour shift after the cut: the grade on shot 12 doesn't match 11."];
        let items = (0..8).map(|i| Item { note: Note { timecode: format!("00:00:{:02}:{:02}", i * 3, (i * 7) % 24), frame: (i * 79) as i64, text: texts[i % 4].into(), addressed: i % 3 == 1, viewer: if i == 2 { "Viewer: +1.0 stops · luma".into() } else { String::new() }, ocio: if i % 4 == 3 { "OCIO: ACEScg → ACES 2.0 - SDR 100 nits (Rec.709)".into() } else { String::new() }, ..Default::default() }, frame: Some((svg(i as u32 * 40), "image/svg+xml")), ink: (i == 0).then(|| include_bytes!("../../../apps/qcv/assets/icons/qcview_export.png").to_vec()), flat: None }).collect();
        let fonts = vec![("Geist", 400, "normal", &include_bytes!("../../min-theme/assets/fonts/Geist-Regular.ttf")[..]), ("Geist", 700, "normal", &include_bytes!("../../min-theme/assets/fonts/Geist-Bold.ttf")[..]), ("Geist Mono", 400, "normal", &include_bytes!("../../min-theme/assets/fonts/GeistMono-Regular.ttf")[..])];
        let d = Doc { media_name: "sample_shot_v003".into(), media_path: "/Volumes/projects/sample/renders/sample_shot_v003.mov".into(), exported: "2026-11-08 21:30".into(), items, brand_png: include_bytes!("../../../apps/qcv/assets/icons/qcview_export.png"), fonts };
        std::fs::write(out, html(&d)).unwrap();
    }
}
