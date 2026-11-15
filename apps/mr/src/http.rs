//! The node's HTTP, both ways, as a 0.5.x node speaks it (contract 2.1-2.3): HTTP/1.1 without TLS, a new connection
//! for every request, JSON bodies. The server answers one request per connection.
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::time::Duration;

pub struct Request { pub method: String, pub path: String, pub authorization: Option<String>, pub body: Vec<u8> }
pub struct Response { pub status: u16, pub content_type: Option<&'static str>, pub body: Vec<u8> }
impl Response {
    pub fn json(status: u16, v: serde_json::Value) -> Response { Response { status, content_type: Some("application/json"), body: v.to_string().into_bytes() } }
    pub fn ok() -> Response { Response::json(200, serde_json::json!({ "status": "ok" })) }
    pub fn error(status: u16, e: &str) -> Response { Response::json(status, serde_json::json!({ "error": e })) }
    /// A status and nothing else (what an unmatched path gets).
    pub fn empty(status: u16) -> Response { Response { status, content_type: None, body: Vec::new() } }
}
/// The largest body a request may carry (contract 2.1): beyond it, 413.
const BODY_LIMIT: usize = 16 << 20;
fn reason(s: u16) -> &'static str { match s { 200 => "OK", 400 => "Bad Request", 401 => "Unauthorized", 404 => "Not Found", 409 => "Conflict", 413 => "Payload Too Large", 500 => "Internal Server Error", 503 => "Service Unavailable", _ => "" } }

fn read_request(s: &mut TcpStream) -> Result<Request, u16> {
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let (mut buf, mut tmp) = (Vec::new(), [0u8; 8192]);
    let cut = loop { if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") { break i } if buf.len() > 64 << 10 { return Err(400) }
        match s.read(&mut tmp) { Ok(0) | Err(_) => return Err(400), Ok(n) => buf.extend_from_slice(&tmp[..n]) } };
    let head = String::from_utf8_lossy(&buf[..cut]).into_owned(); let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' '); let (method, target) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("").to_string());
    let (mut len, mut authorization) = (0usize, None);
    for l in lines { if let Some((k, v)) = l.split_once(':') { let k = k.trim().to_ascii_lowercase(); if k == "content-length" { len = v.trim().parse().unwrap_or(0); } else if k == "authorization" { authorization = Some(v.trim_start_matches(' ').to_string()); } } }
    if len > BODY_LIMIT { return Err(413) }
    let mut body = buf[cut + 4..].to_vec();
    while body.len() < len { match s.read(&mut tmp) { Ok(0) | Err(_) => return Err(400), Ok(n) => body.extend_from_slice(&tmp[..n]) } }
    body.truncate(len);
    // the path is decoded before it is matched; a query is cut off and kept apart by the caller
    Ok(Request { method, path: target, authorization, body })
}
/// `%XX` decoded (a path is matched in its decoded form).
pub fn decode(p: &str) -> String {
    let b = p.as_bytes(); let (mut out, mut i) = (Vec::with_capacity(b.len()), 0);
    while i < b.len() {
        if b[i] == b'%' && i + 3 <= b.len() { if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) { out.push(v); i += 3; continue } }
        out.push(b[i]); i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
/// Serves until `stop` says so. `handle` is called on a thread of its own for each request.
pub fn serve(listener: TcpListener, stop: std::sync::Arc<std::sync::atomic::AtomicBool>, handle: impl Fn(Request) -> Response + Send + Sync + 'static) {
    // (the wait for a connection blocks, so a request is taken up at once; `wake` ends the wait when the node stops)
    let handle = std::sync::Arc::new(handle);
    loop {
        let got = listener.accept(); if stop.load(std::sync::atomic::Ordering::Relaxed) { break }
        match got {
            Ok((mut s, _)) => { let handle = handle.clone();
                std::thread::spawn(move || {
                    let r = match read_request(&mut s) { Ok(q) => handle(q), Err(code) => Response::empty(code) };
                    let mut head = format!("HTTP/1.1 {} {}\r\nConnection: close\r\n", r.status, reason(r.status));
                    if let Some(t) = r.content_type { head.push_str(&format!("Content-Type: {t}\r\n")); }
                    head.push_str(&format!("Content-Length: {}\r\n\r\n", r.body.len()));
                    let _ = s.write_all(head.as_bytes()); let _ = s.write_all(&r.body); let _ = s.flush(); let _ = s.shutdown(std::net::Shutdown::Write);
                    let mut sink = [0u8; 256]; let _ = s.set_read_timeout(Some(Duration::from_millis(200))); let _ = s.read(&mut sink); }); }
            Err(_) => std::thread::sleep(Duration::from_millis(15)),
        }
    }
}
/// A connection to this machine's own port, made so that a server waiting for one looks at its stop flag.
pub fn wake(port: u16) { let _ = TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(300)); }
/// One request to another node. `None`: no answer at all (the callers' "status 0").
pub fn call(endpoint: &str, method: &str, path: &str, bearer: Option<&str>, body: Option<&str>, connect: Duration, read: Duration) -> Option<(u16, Vec<u8>)> {
    let addr = endpoint.to_socket_addrs().ok()?.next()?;
    let mut s = TcpStream::connect_timeout(&addr, connect).ok()?; let _ = s.set_read_timeout(Some(read)); let _ = s.set_write_timeout(Some(read));
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {endpoint}\r\nAccept: */*\r\nConnection: close\r\n");
    if let Some(b) = bearer { req.push_str(&format!("Authorization: Bearer {b}\r\n")); }
    if method == "POST" { let b = body.unwrap_or(""); req.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}", b.len())); } else { req.push_str("\r\n"); }
    s.write_all(req.as_bytes()).ok()?;
    let mut raw = Vec::new(); s.read_to_end(&mut raw).ok()?;
    let cut = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let status: u16 = std::str::from_utf8(&raw[..cut]).ok()?.split("\r\n").next()?.split(' ').nth(1)?.parse().ok()?;
    Some((status, raw[cut + 4..].to_vec()))
}
