//! A canvas written out (canon row 331; the Qt app's is in `docs/phase3/step-export-import-spec.md` §5). Two forms:
//! - HTML: one file that opens ON the canvas - every card where it is, the sheet dragged about and zoomed (the wheel,
//!   Cmd / Ctrl + the wheel, `+` `-` `0` `1`), fitted when it opens. A note card holds the note as its export writes it,
//!   cut at the card's height; a click opens the whole note over the canvas, Esc goes back. Pictures, pages and frames
//!   open in the lightbox with their drawings as a layer. Strokes are SVG; frames are drawn under their members and one
//!   with a size of its own clips them.
//! - a poster: the whole canvas as one PNG, 1600 px wide - notes as sheets with their title and a bar per block,
//!   pictures as they are, strokes, frames, text. Drawn as an SVG and made into pixels by resvg (no window needed).
use super::{html_blocks, html_media, thumb, Col, Fonts, Item, Mode, Opts, Source, Walk, What, CSS, LIGHTBOX, PRINT};
use kurbo::{Rect, Vec2};
use mn_doc::{BlockId, Doc, Kind};
use mn_store::Element;
use qcv_notes::export::{b64, esc};

/// The room left round everything.
const MARGIN: f64 = 24.0;
/// A frame's title strip (the canvas's own measure).
const FRAME_TITLE: f64 = 28.0;

fn rect(e: &Element) -> Rect { Rect::new(e.x, e.y, e.x + e.w, e.y + e.h) }
fn bounds(els: &[&Element]) -> Rect { els.iter().map(|e| rect(e)).reduce(|a, b| a.union(b)).unwrap_or(Rect::new(0.0, 0.0, 800.0, 600.0)).inflate(MARGIN, MARGIN) }
fn frame_of(e: &Element) -> (String, Option<String>, bool) { let v: serde_json::Value = serde_json::from_str(&e.record).unwrap_or_default();
    (v.get("title").and_then(|x| x.as_str()).unwrap_or("").to_string(), v.get("tint").and_then(|x| x.as_str()).and_then(|c| mn_layout::colour(c)).map(|c| { let c = c.to_rgba8(); format!("{},{},{}", c.r, c.g, c.b) }), v.get("fixed").and_then(|x| x.as_bool()).unwrap_or(false)) }
fn host(url: &str) -> &str { url.split("://").nth(1).unwrap_or(url).split(['/', '?', '#']).next().unwrap_or(url) }

/// The canvas as one HTML file. `els`: from the back to the front, each frame with its members straight after it.
/// `note`: a note card's note - its name and its blocks.
#[allow(clippy::too_many_arguments)]
pub fn canvas_html(els: &[&Element], title: &str, exported: &str, opts: Opts, fonts: &Fonts, src: &mut dyn Source, note: &mut dyn FnMut(BlockId) -> Option<(String, Doc)>) -> String {
    let b = bounds(els); let mut world = String::new();
    let place = |e: &Element, o: Vec2| format!("left:{:.1}px;top:{:.1}px;width:{:.1}px;height:{:.1}px", e.x - o.x, e.y - o.y, e.w, e.h);
    // inside a frame with a size of its own, things are placed from the frame's corner (it clips them)
    let (mut origin, mut inside): (Vec2, Option<BlockId>) = (Vec2::new(b.x0, b.y0), None); let mut i = 0;
    while i < els.len() { let e = els[i];
        if inside.is_some() && e.parent != inside { world += "</div></div>\n"; inside = None; origin = Vec2::new(b.x0, b.y0); }
        match e.kind.as_str() {
            "frame" => { let (name, tint, fixed) = frame_of(e);
                world += &format!("<div class=\"frame\" style=\"{}{}\"><div class=\"ft\">{}</div>", place(e, origin), tint.map(|t| format!(";--t:{t}")).unwrap_or_default(), esc(&name));
                if fixed { world += "<div class=\"fc\">"; inside = Some(e.id); origin = Vec2::new(e.x, e.y); } else { world += "</div>\n"; } }
            // strokes that follow one another are one drawing
            "ink" => { let mut run = vec![]; while i < els.len() && els[i].kind == "ink" && els[i].parent == e.parent { if let Some(l) = crate::ink::read(els[i]) { run.push(crate::ink::placed(&l, 1.0, -origin)); } i += 1; }
                let (w, h) = if inside.is_some() { let f = els.iter().find(|x| Some(x.id) == inside).map(|f| (f.w, f.h)).unwrap_or((b.width(), b.height())); f } else { (b.width(), b.height()) };
                world += &format!("<svg class=\"strokes\" viewBox=\"0 0 {w:.1} {h:.1}\" style=\"width:{w:.1}px;height:{h:.1}px\">{}</svg>\n", crate::ink::svg(&run)); continue }
            "link" => world += &format!("<a class=\"card link\" style=\"{}\" href=\"{}\" target=\"_blank\" rel=\"noopener\"><span class=\"host\">{}</span><span class=\"url\">{}</span></a>\n", place(e, origin), esc(&e.record), esc(host(&e.record)), esc(&e.record)),
            "text" => world += &format!("<div class=\"card text\" style=\"{}\">{}</div>\n", place(e, origin), esc(&e.record)),
            "note" => { let got = BlockId::parse(&e.record).and_then(|id| note(id));
                match got { Some((name, doc)) => { let w = Walk::new(doc.walk(), opts); let mut body = String::new(); html_blocks(&w, doc.children(None), &Col::default(), src, &mut body);
                        world += &format!("<div class=\"card note\" style=\"{}\" tabindex=\"0\" data-name=\"{}\"><div class=\"nt\">{}</div><article class=\"nb\">{body}</article><div class=\"fade\"></div></div>\n", place(e, origin), esc(&name), esc(&name)); }
                    None => world += &format!("<div class=\"card text\" style=\"{}\">(a note that is no longer in the collection)</div>\n", place(e, origin)) } }
            _ => { let it = src.item(&e.record, Mode::Layers, opts);
                match (it.what, it.shots.first().filter(|s| s.pic.is_some())) {
                    (What::Picture, Some(s)) => world += &format!("<div class=\"card pic\" style=\"{}\">{}</div>\n", place(e, origin), thumb(s, "fill")),
                    _ => { let mut inner = String::new(); html_media(&it, opts, &mut inner); world += &format!("<div class=\"card ref\" style=\"{}\">{inner}</div>\n", place(e, origin)); } } }
        }
        i += 1;
    }
    if inside.is_some() { world += "</div></div>\n"; }
    let mut o = String::with_capacity(world.len() + 256 * 1024);
    o += "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta name=\"generator\" content=\"minNotes\">\n";
    o += &format!("<title>{}</title>\n<style>\n", esc(title));
    o += &qcv_notes::export::font_faces(fonts); o += qcv_notes::export::TOKENS; o += CSS; o += CANVAS_CSS;
    o += &super::syntax_css(&mn_layout::syntax::DARK); o += &format!("@media print{{\n{}\n{PRINT}{}}}\n", qcv_notes::export::PRINT_TOKENS, super::syntax_css(&mn_layout::syntax::LIGHT));
    o += "</style></head><body class=\"cv\">\n";
    o += &format!("<header id=\"bar\"><span class=\"name\">{}</span><span class=\"meta\">Exported {}</span><span class=\"grow\"></span><button id=\"fit\" title=\"Fit everything (0)\">Fit</button><span id=\"zoom\">100%</span></header>\n", esc(title), esc(exported));
    o += &format!("<div id=\"view\"><div id=\"world\" style=\"width:{:.1}px;height:{:.1}px\">\n{world}</div></div>\n", b.width(), b.height());
    o += "<div id=\"reader\" aria-hidden=\"true\"><header><button id=\"back\" title=\"Back to the canvas (Esc)\">\u{2190} Canvas</button><span class=\"name\"></span></header><main><article></article></main></div>\n";
    o += LIGHTBOX; o += CANVAS_SCRIPT; o += "</body></html>\n"; o
}

const CANVAS_CSS: &str = r#"
body.cv{overflow:hidden;height:100vh}
#bar{position:fixed;inset:0 0 auto 0;height:36px;display:flex;align-items:center;gap:14px;padding:0 14px;background:var(--l1);border-bottom:1px solid var(--g300);z-index:5;font:12px/18px var(--mono);color:var(--g600)}
#bar .name{font-weight:500;color:var(--g800)}#bar .grow{flex:1}
#bar button,#reader button{font:500 12px/18px var(--sans);color:var(--g800);background:var(--l2);border:1px solid var(--g300);border-radius:6px;padding:3px 10px;cursor:pointer}
#bar button:hover,#reader button:hover{background:var(--g300);color:var(--g900)}
#zoom{min-width:44px;text-align:right}
#view{position:fixed;inset:36px 0 0 0;overflow:hidden;cursor:grab;touch-action:none}
#view.drag{cursor:grabbing}
#world{position:absolute;left:0;top:0;transform-origin:0 0}
#world>*,.fc>*{position:absolute}
.card{background:var(--l2);border:1px solid var(--g300);border-radius:8px;overflow:hidden;color:var(--g800)}
.card.text{padding:8px 10px;white-space:pre-wrap;overflow-wrap:anywhere;font-size:14px;line-height:20px}
.card.link{display:flex;flex-direction:column;justify-content:center;padding:6px 12px;text-decoration:none}
.card.link .host{color:var(--accent-hi);font-weight:500;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.card.link .url{font:11px/16px var(--mono);color:var(--g600);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.card.pic{background:#000;line-height:0}.card.pic .thumb.fill{width:100%;height:100%;border-radius:0}.card.pic .thumb.fill img{width:100%;height:100%;object-fit:contain}
.card.ref{display:flex;flex-direction:column}.card.ref figure{margin:0;border:0;border-radius:0;flex:1;min-height:0;display:flex;flex-direction:column}.card.ref figure.ref .thumb{flex:1;min-height:0}.card.ref figure.ref .thumb img{width:100%;height:100%;object-fit:contain}
.card.ref figure+figure{display:none}
/* a video's notes on its card: their stills in a row under the picture (each opens with its drawing and its words) */
.card.ref table.vnotes{display:block;flex:none;width:auto;margin:0;border:0;border-top:1px solid var(--g300);border-radius:0;padding:6px;overflow:hidden;background:var(--l1)}
.card.ref table.vnotes tbody{display:flex;gap:6px}.card.ref table.vnotes tr{display:block;flex:none}.card.ref table.vnotes td{display:none}
.card.ref table.vnotes td.fr{display:block;padding:0;width:auto;border:0}.card.ref table.vnotes .thumb,.card.ref table.vnotes .nothumb{width:auto;height:44px;border-radius:3px}.card.ref table.vnotes .thumb img{width:auto;height:44px}
.card.ref table.vnotes tr.done .thumb{opacity:.55}
.card.note{cursor:pointer;background:var(--l1)}
.card.note:hover,.card.note:focus-visible{border-color:var(--accent);outline:none}
.card.note .nt{font:500 12px/18px var(--mono);color:var(--g700);padding:8px 16px 6px;border-bottom:1px solid var(--g200)}
.card.note .nb{padding:12px 16px;pointer-events:none}
.card.note .fade{position:absolute;inset:auto 0 0 0;height:36px;background:linear-gradient(transparent,var(--l1))}
.frame{background:rgba(var(--t,138,138,138),.10);border:1px solid rgba(var(--t,138,138,138),.5);border-radius:8px}
.frame .ft{height:28px;line-height:28px;padding:0 10px;font-weight:700;font-size:13px;color:rgb(var(--t,219,219,219));background:rgba(var(--t,138,138,138),.12);border-radius:8px 8px 0 0;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.frame .fc{position:absolute;inset:0;overflow:hidden;border-radius:8px}
svg.strokes{left:0;top:0;pointer-events:none;overflow:visible}
#reader{position:fixed;inset:0;background:var(--base);z-index:20;display:none;flex-direction:column}
#reader.open{display:flex}
#reader header{height:44px;flex:none;display:flex;align-items:center;gap:14px;padding:0 14px;border-bottom:1px solid var(--g300);font:500 13px/20px var(--mono);color:var(--g700)}
#reader main{flex:1;overflow:auto;max-width:none;padding:32px 48px 96px}#reader article{max-width:864px;margin:0 auto}
"#;

/// The sheet's moves, and a note card opened over it.
const CANVAS_SCRIPT: &str = r#"<script>
(function(){
var view=document.getElementById('view'),world=document.getElementById('world'),zl=document.getElementById('zoom'),reader=document.getElementById('reader');
var W=parseFloat(world.style.width),H=parseFloat(world.style.height),z=1,tx=0,ty=0;
function put(){world.style.transform='translate('+tx+'px,'+ty+'px) scale('+z+')';zl.textContent=Math.round(z*100)+'%'}
function fit(){var r=view.getBoundingClientRect();z=Math.min(r.width/W,r.height/H,1);tx=(r.width-W*z)/2;ty=(r.height-H*z)/2;put()}
function zoomTo(n,cx,cy){n=Math.max(.05,Math.min(8,n));tx=cx-(cx-tx)*n/z;ty=cy-(cy-ty)*n/z;z=n;put()}
function mid(){var r=view.getBoundingClientRect();return [r.width/2,r.height/2]}
view.addEventListener('wheel',function(e){e.preventDefault();var r=view.getBoundingClientRect();
if(e.ctrlKey||e.metaKey)zoomTo(z*Math.exp(-e.deltaY*.0025),e.clientX-r.left,e.clientY-r.top);else{tx-=e.deltaX;ty-=e.deltaY;put()}},{passive:false});
var down=null,moved=false;
view.addEventListener('pointerdown',function(e){if(e.button!==0)return;down=[e.clientX,e.clientY,tx,ty];moved=false});
window.addEventListener('pointermove',function(e){if(!down)return;var dx=e.clientX-down[0],dy=e.clientY-down[1];if(!moved&&Math.abs(dx)+Math.abs(dy)<5)return;moved=true;view.classList.add('drag');tx=down[2]+dx;ty=down[3]+dy;put()});
window.addEventListener('pointerup',function(){down=null;view.classList.remove('drag')});
// a drag is never a click: the sheet was moved, nothing is opened
view.addEventListener('click',function(e){if(moved){e.preventDefault();e.stopPropagation();moved=false;return}
var n=e.target.closest('.card.note');if(n){e.stopPropagation();open(n)}},true);
function open(n){reader.querySelector('article').innerHTML=n.querySelector('.nb').innerHTML;reader.querySelector('.name').textContent=n.dataset.name||'';reader.classList.add('open');reader.setAttribute('aria-hidden','false');reader.querySelector('main').scrollTop=0}
function shut(){reader.classList.remove('open');reader.setAttribute('aria-hidden','true');reader.querySelector('article').innerHTML=''}
document.getElementById('back').addEventListener('click',shut);document.getElementById('fit').addEventListener('click',fit);
document.addEventListener('keydown',function(e){if(e.metaKey||e.ctrlKey||e.altKey)return;if(document.getElementById('lb').classList.contains('open'))return;
if(reader.classList.contains('open')){if(e.key==='Escape')shut();return}
var m=mid();if(e.key==='+'||e.key==='=')zoomTo(z*1.25,m[0],m[1]);else if(e.key==='-')zoomTo(z/1.25,m[0],m[1]);else if(e.key==='0')fit();else if(e.key==='1')zoomTo(1,m[0],m[1]);
else if(e.key==='Enter'&&document.activeElement&&document.activeElement.classList.contains('note'))open(document.activeElement)});
window.addEventListener('resize',fit);fit();
})();
</script>
"#;

// -- the poster ------------------------------------------------------------------------------------------------------

fn bar_of(k: &Kind) -> Option<(f64, f64, f64)> { // (indent, height, share of the width)
    Some(match k { Kind::Heading { level } => (0.0, (16.0 - *level as f64).max(8.0), 0.55), Kind::Bullet { depth } | Kind::Numbered { depth } | Kind::Task { depth, .. } => (14.0 * (*depth as f64 + 1.0), 6.0, 0.7), Kind::Code { .. } => (0.0, 18.0, 1.0), Kind::Divider => (0.0, 1.5, 1.0),
        Kind::Media { .. } | Kind::Table { .. } | Kind::Columns => (0.0, 48.0, 1.0), Kind::Paragraph | Kind::Quote => (0.0, 6.0, 0.92), _ => return None })
}
/// The canvas as an SVG `width` px wide (the poster before it is pixels).
pub fn canvas_svg(els: &[&Element], width: f64, src: &mut dyn Source, note: &mut dyn FnMut(BlockId) -> Option<(String, Doc)>) -> (String, u32, u32) {
    let b = bounds(els); let k = width / b.width().max(1.0); let h = (b.height() * k).ceil().max(1.0);
    let mut s = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{width:.0}\" height=\"{h:.0}\" viewBox=\"{:.2} {:.2} {:.2} {:.2}\" font-family=\"Geist\"><rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" fill=\"#111111\"/>\n", b.x0, b.y0, b.width(), b.height(), b.x0, b.y0, b.width(), b.height());
    let clip_text = |t: &str, room: f64, size: f64| -> String { let n = (room / (size * 0.56)).max(1.0) as usize; if t.chars().count() > n { format!("{}\u{2026}", t.chars().take(n.saturating_sub(1)).collect::<String>()) } else { t.to_string() } };
    let mut clips = 0; let mut inside: Option<BlockId> = None;
    for e in els { let r = rect(e);
        if inside.is_some() && e.parent != inside { s += "</g>\n"; inside = None; }
        let card = |fill: &str| format!("<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"8\" fill=\"{fill}\" stroke=\"#393939\"/>", r.x0, r.y0, r.width(), r.height());
        match e.kind.as_str() {
            "frame" => { let (name, tint, fixed) = frame_of(e); let t = tint.unwrap_or_else(|| "138,138,138".into());
                s += &format!("<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"8\" fill=\"rgba({t},.10)\" stroke=\"rgba({t},.5)\"/><text x=\"{:.1}\" y=\"{:.1}\" font-size=\"13\" font-weight=\"700\" fill=\"rgb({t})\">{}</text>\n", r.x0, r.y0, r.width(), r.height(), r.x0 + 10.0, r.y0 + FRAME_TITLE / 2.0 + 4.5, esc(&clip_text(&name, r.width() - 20.0, 13.0)));
                if fixed { clips += 1; s += &format!("<clipPath id=\"f{clips}\"><rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"8\"/></clipPath><g clip-path=\"url(#f{clips})\">", r.x0, r.y0, r.width(), r.height()); inside = Some(e.id); } }
            "ink" => if let Some(l) = crate::ink::read(e) { s += &crate::ink::svg(&[l]); s.push('\n'); },
            "link" => { s += &card("#222222"); s += &format!("<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"13\" fill=\"#6995fe\">{}</text>\n", r.x0 + 12.0, r.center().y + 4.5, esc(&clip_text(&e.record, r.width() - 24.0, 13.0))); }
            "text" => { s += &card("#222222"); let per = ((r.width() - 20.0) / (14.0 * 0.56)).max(4.0) as usize; let mut y = r.y0 + 24.0;
                'lines: for line in e.record.lines() { let chars: Vec<char> = line.chars().collect(); let mut at = 0;
                    loop { if y > r.y1 - 6.0 { break 'lines } let end = (at + per).min(chars.len()); let cut = if end < chars.len() { chars[at..end].iter().rposition(|c| *c == ' ').map(|p| at + p + 1).filter(|p| *p > at).unwrap_or(end) } else { end };
                        s += &format!("<text x=\"{:.1}\" y=\"{y:.1}\" font-size=\"14\" fill=\"#dbdbdb\">{}</text>", r.x0 + 10.0, esc(chars[at..cut].iter().collect::<String>().trim_end())); y += 20.0; at = cut; if at >= chars.len() { break } } }
                s.push('\n'); }
            "note" => { s += &card("#1b1b1b");
                if let Some((name, doc)) = BlockId::parse(&e.record).and_then(|id| note(id)) {
                    s += &format!("<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"12\" fill=\"#afafaf\">{}</text><line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"#323232\"/>", r.x0 + 16.0, r.y0 + 21.0, esc(&clip_text(&name, r.width() - 32.0, 12.0)), r.x0, r.y0 + 32.0, r.x1, r.y0 + 32.0);
                    // a bar for each block: the note's shape, not its words
                    let mut y = r.y0 + 46.0; let room = r.width() - 32.0;
                    for id in doc.children(None) { let Some(blk) = doc.get(*id) else { continue }; let Some((indent, bh, share)) = bar_of(&blk.kind) else { continue };
                        let lines = if blk.kind.is_text() && !matches!(blk.kind, Kind::Heading { .. } | Kind::Code { .. }) { ((blk.text.chars().count() as f64 * 7.4 / room.max(40.0)).ceil() as usize).clamp(1, 6) } else { 1 };
                        for n in 0..lines { if y + bh > r.y1 - 8.0 { break } let w = (room - indent) * if n + 1 == lines && lines > 1 { 0.5 } else { share };
                            s += &format!("<rect x=\"{:.1}\" y=\"{y:.1}\" width=\"{:.1}\" height=\"{bh:.1}\" rx=\"2\" fill=\"{}\"/>", r.x0 + 16.0 + indent, w.max(8.0), if matches!(blk.kind, Kind::Heading { .. }) { "#8a8a8a" } else { "#444444" }); y += bh + 5.0; }
                        y += 5.0; if y > r.y1 - 8.0 { break } }
                    s.push('\n'); } }
            _ => { let it: Item = src.item(&e.record, Mode::Paper, Opts { paths: false, video_notes: false });
                match it.shots.first().and_then(|sh| sh.pic.as_ref()) {
                    Some(p) => { let (pw, ph) = (p.w.max(1) as f64, p.h.max(1) as f64); let f = (r.width() / pw).min(r.height() / ph); let (w, hh) = (pw * f, ph * f);
                        s += &format!("<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"#000\"/><image x=\"{:.1}\" y=\"{:.1}\" width=\"{w:.1}\" height=\"{hh:.1}\" preserveAspectRatio=\"none\" xlink:href=\"data:{};base64,{}\"/>\n", r.x0, r.y0, r.width(), r.height(), r.center().x - w / 2.0, r.center().y - hh / 2.0, p.mime, b64(&p.bytes)); }
                    None => { s += &card("#222222"); s += &format!("<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"13\" fill=\"#dbdbdb\">{}</text>\n", r.x0 + 10.0, r.y0 + 22.0, esc(&clip_text(&it.name, r.width() - 20.0, 13.0))); } } }
        }
    }
    if inside.is_some() { s += "</g>\n"; }
    s += "</svg>"; (s, width as u32, h as u32)
}
/// The poster: the canvas 1600 px wide, as a PNG.
pub fn canvas_poster(els: &[&Element], src: &mut dyn Source, note: &mut dyn FnMut(BlockId) -> Option<(String, Doc)>) -> Result<Vec<u8>, String> {
    use resvg::{tiny_skia, usvg};
    let (svg, w, h) = canvas_svg(els, 1600.0, src, note); if h > 16000 { return Err("the canvas is too tall for a poster".into()) }
    let mut opt = usvg::Options::default(); { let db = opt.fontdb_mut(); use min_theme::fonts as f; for bytes in [f::GEIST_REGULAR, f::GEIST_BOLD] { db.load_font_data(bytes.to_vec()); } }
    let tree = usvg::Tree::from_data(svg.as_bytes(), &opt).map_err(|e| e.to_string())?;
    let mut pm = tiny_skia::Pixmap::new(w, h).ok_or("the poster could not be made")?;
    resvg::render(&tree, tiny_skia::Transform::identity(), &mut pm.as_mut());
    pm.encode_png().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{Pic, Shot};
    struct Fake;
    impl Source for Fake { fn item(&mut self, _: &str, _: Mode, _: Opts) -> Item { Item { what: What::Picture, name: "shot.png".into(), path: String::new(), meta: String::new(), dw: None, problem: None, shots: vec![Shot { pic: Some(Pic { bytes: vec![1, 2, 3], mime: "image/png", ext: "png", w: 4, h: 2 }), head: "shot.png".into(), ..Default::default() }], notes: vec![] } } }
    fn el(kind: &str, record: &str, x: f64, y: f64, w: f64, h: f64, parent: Option<BlockId>) -> Element { Element { id: BlockId::new(), kind: kind.into(), x, y, w, h, z: 0, parent, record: record.into(), locked: false } }
    /// A page to LOOK at (run with `--ignored`, `MN_SAMPLE=<file.html>`): a video card with three notes, one without.
    #[test] #[ignore]
    fn a_sample_canvas_with_a_videos_notes_is_written() {
        let Ok(to) = std::env::var("MN_SAMPLE") else { return };
        struct Vid;
        impl Source for Vid { fn item(&mut self, d: &str, _: Mode, _: Opts) -> Item {
            let shot = |shade: u8, head: &str, text: &str, done: Option<bool>| Shot { pic: Some(Pic { bytes: min_image::write::png_rgba8_bytes(&[shade, 90, 255 - shade, 255].repeat(160 * 90), 160, 90).unwrap(), mime: "image/png", ext: "png", w: 160, h: 90 }), head: head.into(), sub: "frame".into(), text: text.into(), done, ..Default::default() };
            Item { what: What::Video, name: "clip.mp4".into(), path: String::new(), meta: "1920 \u{d7} 1080 \u{b7} 00:00:10:00".into(), dw: None, problem: None, shots: vec![shot(40, "clip.mp4", "", None)],
                notes: if d.contains("plain") { vec![] } else { vec![shot(90, "00:00:01:00", "too dark here", Some(false)), shot(150, "00:00:03:12", "fixed", Some(true)), shot(210, "00:00:07:00", "", Some(false))] } } } }
        let a = el("media", "{\"kind\":\"video\"}", 0.0, 0.0, 420.0, 330.0, None); let b = el("media", "{\"kind\":\"video\",\"plain\":1}", 460.0, 0.0, 420.0, 330.0, None);
        std::fs::write(to, canvas_html(&[&a, &b], "Sample", "today", Opts::default(), &vec![], &mut Vid, &mut |_| None)).unwrap();
    }
    #[test]
    fn cards_stand_where_they_are_and_a_frame_with_a_size_holds_its_members() {
        let frame = el("frame", "{\"title\":\"Shots <A>\",\"tint\":\"teal\",\"fixed\":true}", 100.0, 100.0, 400.0, 300.0, None);
        let pic = el("media", "{\"src\":\"x\"}", 150.0, 160.0, 200.0, 100.0, Some(frame.id)); let text = el("text", "a <note>", 600.0, 100.0, 200.0, 60.0, None);
        let note_id = BlockId::new(); let card = el("note", &note_id.to_string(), 600.0, 200.0, 300.0, 200.0, None); let link = el("link", "https://example.org/a?b=1", 100.0, 500.0, 300.0, 52.0, None);
        let mut d = Doc::new(); d.insert(None, 0, BlockId::new(), Kind::Heading { level: 1 }, "Inside").unwrap(); d.insert(None, 1, BlockId::new(), Kind::Paragraph, "words of the note").unwrap();
        let els = [&frame, &pic, &text, &card, &link]; let mut note = |id: BlockId| (id == note_id).then(|| ("My note".to_string(), d.clone()));
        let h = canvas_html(&els, "Board", "today", Opts::default(), &vec![], &mut Fake, &mut note);
        // the bounds start at (76, 76): the frame at 24, 24; its member from the frame's own corner
        assert!(h.contains("<div class=\"frame\" style=\"left:24.0px;top:24.0px;width:400.0px;height:300.0px;--t:0,205,172\"><div class=\"ft\">Shots &lt;A&gt;</div><div class=\"fc\"><div class=\"card pic\" style=\"left:50.0px;top:60.0px;width:200.0px;height:100.0px\">"), "{h}");
        assert!(h.contains("</div></div>\n<div class=\"card text\" style=\"left:524.0px;top:24.0px") && h.contains("a &lt;note&gt;") && h.contains("data-name=\"My note\"") && h.contains("<h1>Inside</h1>") && h.contains("href=\"https://example.org/a?b=1\"") && h.contains("<span class=\"host\">example.org</span>"));
        let (svg, w, hh) = canvas_svg(&els, 1600.0, &mut Fake, &mut note); assert_eq!(w, 1600); assert!(hh > 0 && svg.contains("<clipPath id=\"f1\">") && svg.contains("My note") && svg.contains("data:image/png;base64,AQID"));
    }
}
