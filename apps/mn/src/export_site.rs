//! Notes written out as a WEB FOLDER (canon row 339): pages a browser opens from a shared drive or a web server - one
//! per note and per canvas, one stylesheet and one script for all of them, the pictures as files. It is what a whole
//! collection, or some of its notes, is exported as; one note alone is the same folder with one page.
//!
//! ```text
//! index.html            the one note; or, for several, the contents
//! notes/<name>.html     a page per note and per canvas (several only)
//! assets/site.css       the look (fonts in it), canvas.css, site.js (the list of notes, the search), data.js (what they show)
//! media/<hash>.<ext>    every picture, once however often it is shown
//! media/files/<name>    PDFs and attached files, when they are taken along
//! ```
//!
//! Rules it keeps:
//! - It opens from disk by a double click: no page loads data at run time; the list and the search's text are a
//!   script (`data.js`) every page includes.
//! - A page's content goes through ONE step ([`content`]) on its way into its shell, so that a later step (a
//!   password: the content encrypted, row 339's ruling 11) changes nothing of the layout.
//! - A page names its media by a path relative to itself; nothing else knows where media lives.
//! - Only a folder that is empty, or carries our marker, is written into; of an earlier export only what we wrote is
//!   removed.
//!
//! The pages' bodies are the HTML export's own (`html_blocks`, `canvas_body`): their pictures come embedded and are
//! moved out to files here ([`externalise`]).
use super::{canvas::{canvas_body, CANVAS_CSS}, esc, html_blocks, Col, Item, Mode, Opts, Source, Walk, What, COPY_SCRIPT, CSS, LIGHTBOX, PRINT};
use mn_doc::{BlockId, Doc};
use std::{collections::{HashMap, HashSet}, path::{Path, PathBuf}, sync::{atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering::Relaxed}, Mutex}};

pub const MARKER: &str = ".minnotes-export";
pub const QUEUED: u8 = 0; pub const WORKING: u8 = 1; pub const DONE: u8 = 2;

pub enum Kind { /// a folder, or an entry that is only on the way to an exported one
    Folder, Note(Doc), /// its elements from the back to the front, each frame's members after it
    Canvas(Vec<mn_store::Element>) }
pub struct Page { pub id: BlockId, pub parent: Option<BlockId>, pub title: String, pub kind: Kind }
pub struct Job {
    /// in the collection's order; every exported page's ancestors are among them
    pub pages: Vec<Page>, pub dest: PathBuf, pub opts: Opts,
    /// PDFs and attached files are copied in and linked
    pub files: bool,
    /// the collection's name (the list's head, the contents page) and when this was made
    pub name: String, pub when: String,
    /// the notes canvases show as cards: their names and blocks
    pub cards: HashMap<BlockId, (String, Doc)>,
}
impl Job { pub fn exported(&self) -> Vec<usize> { (0..self.pages.len()).filter(|i| !matches!(self.pages[*i].kind, Kind::Folder)).collect() } }

/// What the window watches while a folder is written (one state per page of [`Job::pages`]).
pub struct Progress { pub states: Vec<AtomicU8>, pub stop: AtomicBool, pub over: AtomicBool, pub error: Mutex<Option<String>>, pub media: AtomicU64, pub bytes: AtomicU64 }
impl Progress {
    pub fn new(pages: usize) -> Progress { Progress { states: (0..pages).map(|_| AtomicU8::new(QUEUED)).collect(), stop: AtomicBool::new(false), over: AtomicBool::new(false), error: Mutex::new(None), media: AtomicU64::new(0), bytes: AtomicU64::new(0) } }
    #[cfg(test)] pub fn count(&self, state: u8) -> usize { self.states.iter().filter(|s| s.load(Relaxed) == state).count() }
}

/// What a folder chosen as the destination is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dest { /// not there yet, or empty
    Free, /// an earlier web folder of ours: it is replaced
    Ours, /// something else: left alone
    Other }
pub fn dest_state(dir: &Path) -> Dest {
    if dir.is_file() { return Dest::Other }
    let Ok(list) = std::fs::read_dir(dir) else { return Dest::Free };
    let names: Vec<String> = list.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n != ".DS_Store").collect();
    if names.is_empty() { Dest::Free } else if names.iter().any(|n| n == MARKER) { Dest::Ours } else { Dest::Other }
}
/// The destination made ready: what an earlier export of ours left is removed (its pages, its folders - nothing else),
/// the marker is written.
fn prepare(dir: &Path) -> Result<(), String> {
    match dest_state(dir) {
        Dest::Other => return Err("That folder holds other things: choose an empty folder, or an earlier web folder to replace".into()),
        Dest::Ours => { for d in ["assets", "notes", "media"] { let p = dir.join(d); if p.is_dir() { std::fs::remove_dir_all(&p).map_err(|e| format!("The earlier export could not be replaced ({e})"))?; } } let _ = std::fs::remove_file(dir.join("index.html")); }
        Dest::Free => {}
    }
    std::fs::create_dir_all(dir.join("assets")).map_err(|e| format!("The folder could not be made ({e})"))?;
    std::fs::write(dir.join(MARKER), "A web folder written by minNotes. Exporting to this folder again replaces index.html, assets, notes and media.\n").map_err(|e| format!("The folder could not be written to ({e})"))
}

/// A page's file name from its title: letters, digits and dashes, one of a kind.
fn slug(title: &str, taken: &mut HashSet<String>) -> String {
    let mut s = String::new(); for c in title.trim().chars() { if c.is_ascii_alphanumeric() { s.push(c.to_ascii_lowercase()); } else if !s.ends_with('-') && !s.is_empty() { s.push('-'); } if s.len() >= 60 { break } }
    let s = s.trim_end_matches('-'); let s = if s.is_empty() { "page" } else { s };
    let mut name = s.to_string(); let mut n = 2; while !taken.insert(name.clone()) { name = format!("{s}-{n}"); n += 1; }
    name
}
fn url(name: &str) -> String { name.bytes().map(|b| if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') { (b as char).to_string() } else { format!("%{b:02X}") }).collect() }
/// A note's words, for the search.
fn words(doc: &Doc) -> String { let mut s = String::new(); for b in doc.walk() { if b.kind.is_text() && !b.text.trim().is_empty() { if !s.is_empty() { s.push('\n'); } s.push_str(b.text.trim()); } } s }

/// The step every page's content passes on its way into its shell.
fn content(html: String) -> String { html }

/// The pictures a body carries in itself (`src="data:…;base64,…"`) written to `media/` - each once - and named there.
fn externalise(html: &str, dest: &Path, pre: &str, seen: &mut HashSet<String>, p: &Progress) -> Result<String, String> {
    use base64::Engine; use std::hash::{Hash, Hasher};
    let mut out = String::with_capacity(html.len() / 4 + 1024); let mut rest = html;
    while let Some(i) = rest.find("src=\"data:") {
        out.push_str(&rest[..i + 5]); let after = &rest[i + 5..]; let end = after.find('"').unwrap_or(after.len()); let uri = &after[..end];
        match uri.split_once(";base64,") {
            Some((head, data)) => {
                let ext = match &head[5..] { "image/png" => "png", "image/jpeg" => "jpg", "image/gif" => "gif", "image/webp" => "webp", "image/svg+xml" => "svg", _ => "bin" };
                let mut h = std::collections::hash_map::DefaultHasher::new(); data.hash(&mut h); let name = format!("{:016x}-{:x}.{ext}", h.finish(), data.len());
                if seen.insert(name.clone()) {
                    let bytes = base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| format!("A picture could not be written ({e})"))?;
                    std::fs::create_dir_all(dest.join("media")).and_then(|_| std::fs::write(dest.join("media").join(&name), &bytes)).map_err(|e| format!("A picture could not be written ({e})"))?;
                    p.media.fetch_add(1, Relaxed); p.bytes.fetch_add(bytes.len() as u64, Relaxed);
                }
                out.push_str(pre); out.push_str("media/"); out.push_str(&name);
            }
            None => out.push_str(uri),
        }
        rest = &after[end..];
    }
    out.push_str(rest); Ok(out)
}

/// The application's source, with PDFs and attached files taken along: each copied to `media/files/` once and linked.
struct Linked<'a> { inner: &'a mut dyn Source, dest: &'a Path, pre: &'a str, on: bool, done: &'a mut HashMap<String, Option<String>>, taken: &'a mut HashSet<String>, p: &'a Progress }
impl Source for Linked<'_> {
    fn item(&mut self, descriptor: &str, mode: Mode, opts: Opts) -> Item {
        let mut it = self.inner.item(descriptor, mode, opts);
        if !self.on || !matches!(it.what, What::Pdf | What::File) || it.path.is_empty() { return it }
        let from = PathBuf::from(&it.path);
        let rel = match self.done.get(&it.path) { Some(r) => r.clone(), None => {
            let r = from.is_file().then(|| { let name = crate::gather::unique(&it.name, self.taken); let dir = self.dest.join("media").join("files");
                std::fs::create_dir_all(&dir).and_then(|_| std::fs::copy(&from, dir.join(&name))).ok().map(|n| { self.p.media.fetch_add(1, Relaxed); self.p.bytes.fetch_add(n, Relaxed); name }) }).flatten();
            self.done.insert(it.path.clone(), r.clone()); r } };
        if let Some(name) = rel { it.href = Some(format!("{}media/files/{}", self.pre, url(&name))); }
        it
    }
}

fn head(title: &str, pre: &str, canvas: bool, nav: bool) -> String {
    format!("<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta name=\"generator\" content=\"minNotes\">\n<title>{}</title>\n<link rel=\"stylesheet\" href=\"{pre}assets/site.css\">{}\n{}</head>",
        esc(title), if canvas { format!("<link rel=\"stylesheet\" href=\"{pre}assets/canvas.css\">") } else { String::new() },
        if nav { "<script>(function(){var w=innerWidth>900,v=null;try{v=localStorage.getItem('mn.nav')}catch(e){}if(v==='1'||(v!=='0'&&w))document.documentElement.classList.add('nav')})()</script>\n" } else { "" })
}
fn foot(pre: &str, nav: bool) -> String { if nav { format!("<script src=\"{pre}assets/data.js\"></script><script src=\"{pre}assets/site.js\"></script>\n</body></html>\n") } else { "</body></html>\n".into() } }

/// Writes the folder. `src`: what media looks like (the application's). Stopped between pages by `p.stop`; what was
/// written by then stays, under the marker, to be replaced by the next export there.
pub fn write(job: &Job, src: &mut dyn Source, fonts: &super::Fonts, p: &Progress, wake: &dyn Fn()) -> Result<(), String> {
    let io = |what: &str, e: std::io::Error| format!("{what} could not be written ({e})");
    let dest = job.dest.as_path(); prepare(dest)?;
    let pages = job.exported(); let several = pages.len() != 1;
    // the look, once
    let mut css = qcv_notes::export::font_faces(fonts); css += qcv_notes::export::TOKENS; css += CSS; css += &super::syntax_css(&mn_layout::syntax::DARK); css += SITE_CSS;
    css += &format!("@media print{{\n{}\n{PRINT}{}{SITE_PRINT}}}\n", qcv_notes::export::PRINT_TOKENS, super::syntax_css(&mn_layout::syntax::LIGHT));
    std::fs::write(dest.join("assets/site.css"), css).map_err(|e| io("The stylesheet", e))?;
    if job.pages.iter().any(|x| matches!(x.kind, Kind::Canvas(_))) { std::fs::write(dest.join("assets/canvas.css"), format!("{CANVAS_CSS}{CANVAS_SITE_CSS}")).map_err(|e| io("The stylesheet", e))?; }
    // where each page goes
    let mut taken = HashSet::new(); let mut href: HashMap<BlockId, String> = HashMap::new();
    for i in &pages { let pg = &job.pages[*i]; href.insert(pg.id, if several { format!("notes/{}.html", slug(&pg.title, &mut taken)) } else { "index.html".into() }); }
    if several { std::fs::create_dir_all(dest.join("notes")).map_err(|e| io("The folder", e))?; }
    let pre = if several { "../" } else { "" };
    let (mut seen, mut copied, mut names) = (HashSet::new(), HashMap::new(), HashSet::new()); let mut text: HashMap<String, String> = HashMap::new();
    for i in &pages {
        if p.stop.load(Relaxed) { return Err("Stopped".into()) }
        p.states[*i].store(WORKING, Relaxed); wake();
        let pg = &job.pages[*i]; let title = if pg.title.trim().is_empty() { "Untitled" } else { pg.title.as_str() };
        let mut linked = Linked { inner: &mut *src, dest, pre, on: job.files, done: &mut copied, taken: &mut names, p };
        let (canvas, body) = match &pg.kind {
            Kind::Note(doc) => { let w = Walk::new(doc.walk(), job.opts); let mut b = String::new(); html_blocks(&w, doc.children(None), &Col::default(), &mut linked, &mut b);
                text.insert(pg.id.to_string(), words(doc));
                let copies = b.contains("<button class=\"copy\"");
                (false, format!("<main>\n<header class=\"doc\"><div class=\"titles\"><div class=\"name\">{}</div><div class=\"meta\"><span>Exported {}</span></div></div></header>\n<article>\n{b}</article>\n</main>\n{LIGHTBOX}{}", esc(title), esc(&job.when), if copies { COPY_SCRIPT } else { "" })) }
            Kind::Canvas(els) => { let refs: Vec<&mn_store::Element> = els.iter().collect(); let mut note = |id: BlockId| job.cards.get(&id).cloned();
                text.insert(pg.id.to_string(), els.iter().filter(|e| e.kind == "text").map(|e| e.record.trim()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n"));
                (true, canvas_body(&refs, title, &job.when, job.opts, &mut linked, &mut note)) }
            Kind::Folder => continue,
        };
        let body = content(externalise(&body, dest, pre, &mut seen, p)?);
        let page = format!("{}<body class=\"site{}\" data-id=\"{}\" data-root=\"{pre}\">\n{body}{}", head(title, pre, canvas, several), if canvas { " cv" } else { "" }, pg.id, foot(pre, several));
        std::fs::write(dest.join(&href[&pg.id]), page).map_err(|e| io(&format!("\u{201c}{title}\u{201d}"), e))?;
        p.states[*i].store(DONE, Relaxed); wake();
    }
    if several {
        // what every page's list and search show; then the contents
        let list: Vec<serde_json::Value> = job.pages.iter().map(|x| serde_json::json!({ "id": x.id.to_string(), "parent": x.parent.map(|p| p.to_string()), "title": if x.title.trim().is_empty() { "Untitled" } else { x.title.as_str() },
            "kind": match x.kind { Kind::Folder => "folder", Kind::Note(_) => "note", Kind::Canvas(_) => "canvas" }, "href": href.get(&x.id) })).collect();
        let data = serde_json::json!({ "title": job.name, "pages": list, "text": text });
        std::fs::write(dest.join("assets/data.js"), format!("window.MN={data};\n")).and_then(|_| std::fs::write(dest.join("assets/site.js"), SITE_JS)).map_err(|e| io("The list of notes", e))?;
        fn toc(job: &Job, href: &HashMap<BlockId, String>, parent: Option<BlockId>, out: &mut String) {
            let kids: Vec<&Page> = job.pages.iter().filter(|x| x.parent == parent).collect(); if kids.is_empty() { return }
            out.push_str("<ul>");
            for k in kids { let t = if k.title.trim().is_empty() { "Untitled" } else { k.title.as_str() };
                match href.get(&k.id) { Some(h) => out.push_str(&format!("<li><a href=\"{h}\">{}</a>", esc(t))), None => out.push_str(&format!("<li><span class=\"f\">{}</span>", esc(t))) }
                toc(job, href, Some(k.id), out); out.push_str("</li>"); }
            out.push_str("</ul>");
        }
        let mut list = String::new(); let ids: HashSet<BlockId> = job.pages.iter().map(|x| x.id).collect();
        // (an entry whose parent is not among the pages stands at the top)
        let tops: Vec<Option<BlockId>> = { let mut v = vec![None]; for x in &job.pages { if let Some(p) = x.parent { if !ids.contains(&p) && !v.contains(&Some(p)) { v.push(Some(p)); } } } v };
        for t in tops { toc(job, &href, t, &mut list); }
        let n = pages.len();
        let body = content(format!("<main>\n<header class=\"doc\"><div class=\"titles\"><div class=\"name\">{}</div><div class=\"meta\"><span>{n} page{} \u{b7} Exported {}</span></div></div></header>\n<article class=\"toc\">\n<h1>{}</h1>\n{list}\n</article>\n</main>\n", esc(&job.name), if n == 1 { "" } else { "s" }, esc(&job.when), esc(&job.name)));
        std::fs::write(dest.join("index.html"), format!("{}<body class=\"site\" data-id=\"\" data-root=\"\">\n{body}{}", head(&job.name, "", false, true), foot("", true))).map_err(|e| io("The contents", e))?;
    }
    Ok(())
}

const SITE_CSS: &str = r#"
#nav{position:fixed;inset:0 auto 0 0;width:280px;display:none;flex-direction:column;background:var(--l1);border-right:1px solid var(--g300);z-index:40;font:13px/20px var(--sans)}
html.nav #nav{display:flex}
html.nav body.site{padding-left:280px}
#navb{position:fixed;left:8px;top:8px;z-index:41;width:30px;height:30px;display:flex;align-items:center;justify-content:center;padding:0;border:1px solid var(--g300);border-radius:var(--rs);background:var(--l1);color:var(--g600);cursor:pointer}
#navb:hover{color:var(--g900);background:var(--l2)}
html.nav #navb{left:242px;border-color:transparent;background:none}
#nav .top{padding:12px 46px 8px 16px;font:500 13px/22px var(--sans);color:var(--g900);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
#nav .top a{color:inherit;text-decoration:none}
#nav input{margin:0 12px 8px;padding:6px 10px;border:1px solid var(--g300);border-radius:var(--rs);background:var(--base);color:var(--g900);font:13px/18px var(--sans);outline:none;-webkit-appearance:none}
#nav input:focus{border-color:var(--accent-hi)}
#nav .list{flex:1;overflow:auto;padding:0 8px 16px}
#nav .list a,#nav .fold{display:block;padding:4px 8px;border-radius:var(--rs);color:var(--g700);text-decoration:none;cursor:pointer;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
#nav .list a:hover,#nav .fold:hover{background:var(--l2);color:var(--g900)}
#nav .list a.cur{background:var(--l2);color:var(--g900)}
#nav .fold{color:var(--g600);font-weight:500;user-select:none}
#nav .fold::before{content:"\25BE";display:inline-block;width:14px;font-size:10px}
#nav .shut>.fold::before{content:"\25B8"}
#nav .kids{margin-left:14px}
#nav .shut>.kids{display:none}
#nav .list a.hit{white-space:normal}
#nav .hit small{display:block;color:var(--g500);font-size:12px;line-height:17px;max-height:34px;overflow:hidden}
#nav .none{padding:8px;color:var(--g500)}
.toc ul{list-style:none;margin:0;padding:0 0 0 20px}.toc>ul{padding:0}
.toc li{margin:0;padding:3px 0}.toc .f{color:var(--g600);font-weight:500}
@media (max-width:900px){html.nav body.site{padding-left:0}html.nav #nav{box-shadow:0 0 48px rgba(0,0,0,.6)}}
"#;
const SITE_PRINT: &str = "#nav,#navb{display:none!important}html.nav body.site{padding-left:0}\n";
/// A canvas's page beside the list: its bar and its view start where the list ends.
const CANVAS_SITE_CSS: &str = "body.site.cv{padding-left:0!important}body.site.cv #bar{padding-left:48px}html.nav body.site.cv #bar,html.nav body.site.cv #view{left:280px}html.nav body.site.cv #bar{padding-left:14px}\n@media (max-width:900px){html.nav body.site.cv #bar,html.nav body.site.cv #view{left:0}}\n";

/// The list of notes and the search, built on every page from `data.js`.
const SITE_JS: &str = r#"(function(){
var D=window.MN;if(!D)return;
var B=document.body,H=document.documentElement,root=B.getAttribute('data-root')||'',cur=B.getAttribute('data-id')||'';
function el(t,c,x){var e=document.createElement(t);if(c)e.className=c;if(x!=null)e.textContent=x;return e}
function mem(k,v){try{if(v===undefined)return localStorage.getItem(k);localStorage.setItem(k,v)}catch(e){}return null}
var nav=el('nav');nav.id='nav';nav.setAttribute('aria-label','Notes');
var top=el('div','top'),home=el('a','',D.title);home.href=root+'index.html';home.title=D.title;top.appendChild(home);
var q=el('input');q.type='search';q.placeholder='Search';q.setAttribute('aria-label','Search the notes');
var list=el('div','list');nav.appendChild(top);nav.appendChild(q);nav.appendChild(list);
var b=el('button');b.id='navb';b.type='button';b.title='Show or hide the list of notes';b.setAttribute('aria-label',b.title);
b.innerHTML='<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><rect x="3" y="4.5" width="18" height="15" rx="2.5"/><path d="M9 4.5v15"/></svg>';
B.appendChild(nav);B.appendChild(b);
var by={},ids={};D.pages.forEach(function(p){ids[p.id]=1});
D.pages.forEach(function(p){var k=p.parent&&ids[p.parent]?p.parent:'';(by[k]=by[k]||[]).push(p)});
function walk(pid,into){(by[pid]||[]).forEach(function(p){
  var kids=by[p.id]&&by[p.id].length;
  if(!p.href){var w=el('div',mem('mn.f.'+p.id)==='1'?'shut':''),f=el('div','fold',p.title),k=el('div','kids');
    f.onclick=function(){w.classList.toggle('shut');mem('mn.f.'+p.id,w.classList.contains('shut')?'1':'0')};
    w.appendChild(f);w.appendChild(k);into.appendChild(w);walk(p.id,k);return}
  var a=el('a',p.id===cur?'cur':'',p.title);a.href=root+p.href;a.title=p.title;into.appendChild(a);
  if(kids){var k2=el('div','kids');into.appendChild(k2);walk(p.id,k2)}
})}
function tree(){list.textContent='';walk('',list)}
function search(s){s=s.trim().toLowerCase();if(!s){tree();return}
  list.textContent='';var n=0;
  D.pages.forEach(function(p){if(!p.href)return;var t=D.text[p.id]||'',i=t.toLowerCase().indexOf(s),inTitle=p.title.toLowerCase().indexOf(s)>=0;if(i<0&&!inTitle)return;n++;
    var a=el('a','hit',p.title);a.href=root+p.href;
    if(i>=0){var from=Math.max(0,i-30);a.appendChild(el('small','',(from?'…':'')+t.substr(from,120).replace(/\s+/g,' ')))}
    list.appendChild(a)});
  if(!n)list.appendChild(el('div','none','Nothing found'))}
q.addEventListener('input',function(){search(q.value)});
q.addEventListener('keydown',function(e){e.stopPropagation();if(e.key==='Escape'){q.value='';search('');q.blur()}if(e.key==='Enter'){var a=list.querySelector('a');if(a)location.href=a.href}});
b.addEventListener('click',function(){var on=!H.classList.contains('nav');H.classList.toggle('nav',on);mem('mn.nav',on?'1':'0');window.dispatchEvent(new Event('resize'))});
tree();var c=list.querySelector('.cur');if(c&&c.scrollIntoView)c.scrollIntoView({block:'nearest'});
document.addEventListener('keydown',function(e){if(e.key==='/'&&!e.metaKey&&!e.ctrlKey&&!/^(input|textarea)$/i.test(e.target.tagName||'')&&H.classList.contains('nav')){e.preventDefault();q.focus()}});
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use mn_doc::Kind as K;
    struct Fake;
    impl Source for Fake { fn item(&mut self, d: &str, _: Mode, _: Opts) -> Item {
        let file = d.contains("\"pdf\"");
        Item { what: if file { What::Pdf } else { What::Picture }, name: if file { "plan one.pdf".into() } else { "shot.png".into() }, path: if file { std::env::temp_dir().join("mn-site-test-plan.pdf").to_string_lossy().into_owned() } else { String::new() }, meta: String::new(), dw: None, problem: None,
            shots: if file { vec![] } else { vec![super::super::Shot { pic: Some(super::super::Pic { bytes: vec![1, 2, 3], mime: "image/png", ext: "png", w: 4, h: 2 }), head: "shot.png".into(), ..Default::default() }] }, notes: vec![], href: None } } }
    fn note(blocks: &[(K, &str)]) -> Doc { let mut d = Doc::new(); for (i, (k, t)) in blocks.iter().enumerate() { d.insert(None, i, BlockId::new(), k.clone(), *t).unwrap(); } d }
    fn dir(name: &str) -> PathBuf { let d = std::env::temp_dir().join(format!("mn-site-{name}-{}", std::process::id())); let _ = std::fs::remove_dir_all(&d); d }

    #[test]
    fn several_notes_become_pages_with_one_look_a_list_and_each_picture_once() {
        std::fs::write(std::env::temp_dir().join("mn-site-test-plan.pdf"), b"%PDF").unwrap();
        let (f, a, b) = (BlockId::new(), BlockId::new(), BlockId::new()); let pic = K::Media { descriptor: "{\"src\":\"a.png\"}".into() };
        let job = Job { dest: dir("several"), opts: Opts::default(), files: true, name: "Show & Tell".into(), when: "now".into(), cards: HashMap::new(), pages: vec![
            Page { id: f, parent: None, title: "Shots".into(), kind: Kind::Folder },
            Page { id: a, parent: Some(f), title: "Shot 010 / notes".into(), kind: Kind::Note(note(&[(K::Paragraph, "the quick brown fox"), (pic.clone(), ""), (K::Media { descriptor: "{\"kind\":\"pdf\"}".into() }, "")])) },
            Page { id: b, parent: Some(f), title: "Shot 010 / notes".into(), kind: Kind::Note(note(&[(K::Code { lang: "rs".into() }, "let a = 1;"), (pic, "")])) }] };
        let p = Progress::new(job.pages.len()); write(&job, &mut Fake, &vec![], &p, &|| {}).unwrap();
        let read = |n: &str| std::fs::read_to_string(job.dest.join(n)).unwrap();
        let (one, two, index, data) = (read("notes/shot-010-notes.html"), read("notes/shot-010-notes-2.html"), read("index.html"), read("assets/data.js"));
        assert!(one.contains("href=\"../assets/site.css\"") && one.contains("the quick brown fox") && !one.contains("base64") && one.contains("src=\"../media/") && one.contains("<a class=\"name\" href=\"../media/files/plan%20one.pdf\">"));
        assert!(two.contains("class=\"copy\"") && two.contains("navigator.clipboard") && two.contains("data-root=\"../\""));
        assert!(index.contains("Show &amp; Tell") && index.contains("<span class=\"f\">Shots</span><ul><li><a href=\"notes/shot-010-notes.html\">") && index.contains("src=\"assets/site.js\""));
        assert!(data.starts_with("window.MN={") && data.contains("the quick brown fox") && data.contains("\"href\":\"notes/shot-010-notes-2.html\"") && data.contains("\"kind\":\"folder\""));
        assert_eq!(std::fs::read_dir(job.dest.join("media")).unwrap().flatten().filter(|e| e.path().is_file()).count(), 1, "one picture, shown twice");
        assert_eq!((std::fs::read(job.dest.join("media/files/plan one.pdf")).unwrap(), p.count(DONE), p.media.load(Relaxed)), (b"%PDF".to_vec(), 2, 2));
        let _ = std::fs::remove_dir_all(&job.dest);
    }
    #[test]
    fn one_note_is_the_index_and_only_our_own_folder_is_replaced() {
        let a = BlockId::new(); let d = dir("one");
        let job = Job { dest: d.clone(), opts: Opts::default(), files: false, name: "C".into(), when: "now".into(), cards: HashMap::new(), pages: vec![Page { id: a, parent: None, title: "Only".into(), kind: Kind::Note(note(&[(K::Paragraph, "alone")])) }] };
        write(&job, &mut Fake, &vec![], &Progress::new(1), &|| {}).unwrap();
        let index = std::fs::read_to_string(d.join("index.html")).unwrap();
        assert!(index.contains("alone") && index.contains("href=\"assets/site.css\"") && !index.contains("site.js") && !d.join("notes").exists() && !d.join("assets/data.js").exists());
        // again over itself: replaced; a file of the user's beside our own stays
        std::fs::write(d.join("mine.txt"), "keep").unwrap(); std::fs::create_dir_all(d.join("notes")).unwrap(); std::fs::write(d.join("notes/old.html"), "old").unwrap();
        assert_eq!(dest_state(&d), Dest::Ours); write(&job, &mut Fake, &vec![], &Progress::new(1), &|| {}).unwrap();
        assert!(d.join("mine.txt").exists() && !d.join("notes").exists());
        // a folder of other things is refused, and left as it was
        std::fs::remove_file(d.join(MARKER)).unwrap(); assert_eq!(dest_state(&d), Dest::Other);
        assert!(write(&job, &mut Fake, &vec![], &Progress::new(1), &|| {}).is_err() && d.join("mine.txt").exists() && d.join("index.html").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
    #[test]
    fn a_stop_ends_the_export_between_pages() {
        let d = dir("stop"); let pages = (0..3).map(|i| Page { id: BlockId::new(), parent: None, title: format!("n{i}"), kind: Kind::Note(note(&[(K::Paragraph, "x")])) }).collect();
        let job = Job { dest: d.clone(), opts: Opts::default(), files: false, name: "C".into(), when: "now".into(), cards: HashMap::new(), pages };
        let p = Progress::new(3); p.stop.store(true, Relaxed);
        assert_eq!(write(&job, &mut Fake, &vec![], &p, &|| {}), Err("Stopped".into())); assert_eq!((p.count(DONE), dest_state(&d)), (0, Dest::Ours));
        let _ = std::fs::remove_dir_all(&d);
    }
}
