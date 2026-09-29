//! Minimal HTTP/1.1 client over `std::net::TcpStream`, localhost only.
//! Just enough for Orbi's `GET /hello` and `POST /ask|/event`.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

pub struct Response {
    pub status: u16,
    /// Header names lower-cased.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        let n = name.to_ascii_lowercase();
        self.headers.iter().find(|(k, _)| *k == n).map(|(_, v)| v.as_str())
    }
}

/// Send one request to `127.0.0.1:<port><path>`. `connect` bounds the TCP
/// connect, `total` bounds everything after it (write + wait + read).
/// `headers` are extra `(name, value)` pairs; a body implies JSON.
pub fn request(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
    connect: Duration,
    total: Duration,
) -> Result<Response, String> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut s = TcpStream::connect_timeout(&addr, connect).map_err(|e| e.to_string())?;
    let _ = s.set_nodelay(true);
    let deadline = Instant::now() + total;
    s.set_write_timeout(Some(total)).map_err(|e| e.to_string())?;

    let head = build_request_head(port, method, path, headers, body.map(<[u8]>::len));
    s.write_all(head.as_bytes()).map_err(|e| e.to_string())?;
    if let Some(b) = body {
        s.write_all(b).map_err(|e| e.to_string())?;
    }
    s.flush().map_err(|e| e.to_string())?;

    let mut buf = Vec::with_capacity(512);
    let mut chunk = [0u8; 4096];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err("timeout".into());
        }
        s.set_read_timeout(Some(left)).map_err(|e| e.to_string())?;
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > 1 << 20 {
                    return Err("response too large".into());
                }
                // Stop as soon as a complete response is in (don't rely on close).
                if let Some(r) = parse_response(&buf) {
                    return Ok(r);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    parse_response_eof(&buf).ok_or_else(|| "bad response".into())
}

pub fn build_request_head(port: u16, method: &str, path: &str, headers: &[(&str, &str)], len: Option<usize>) -> String {
    let mut h = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n");
    for (k, v) in headers {
        // Never let a value smuggle in extra header lines.
        if !k.contains(['\r', '\n', ':']) && !v.contains(['\r', '\n']) {
            h.push_str(&format!("{k}: {v}\r\n"));
        }
    }
    if let Some(len) = len {
        h.push_str(&format!("Content-Type: application/json\r\nContent-Length: {len}\r\n"));
    }
    h.push_str(&format!("User-Agent: orbi-hook/{}\r\nConnection: close\r\n\r\n", env!("CARGO_PKG_VERSION")));
    h
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

struct Head {
    status: u16,
    headers: Vec<(String, String)>,
    body_start: usize,
    content_length: Option<usize>,
    chunked: bool,
}

fn parse_head(buf: &[u8]) -> Option<Head> {
    let end = find(buf, b"\r\n\r\n")?;
    let head = std::str::from_utf8(&buf[..end]).ok()?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next()?;
    let mut parts = status_line.splitn(3, ' ');
    if !parts.next()?.starts_with("HTTP/1.") {
        return None;
    }
    let status: u16 = parts.next()?.parse().ok()?;
    let mut content_length = None;
    let mut chunked = false;
    let mut headers = Vec::new();
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            let k = k.trim().to_ascii_lowercase();
            let v = v.trim();
            headers.push((k.clone(), v.to_string()));
            if k == "content-length" {
                content_length = v.parse().ok();
            } else if k == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked") {
                chunked = true;
            }
        }
    }
    Some(Head { status, headers, body_start: end + 4, content_length, chunked })
}

/// A complete response, or None if more bytes are needed.
pub fn parse_response(buf: &[u8]) -> Option<Response> {
    let h = parse_head(buf)?;
    let rest = &buf[h.body_start..];
    if h.chunked {
        return decode_chunked(rest).map(|body| Response { status: h.status, headers: h.headers, body });
    }
    let len = match h.content_length {
        Some(l) => l,
        // 1xx/204/304 have no body; otherwise wait for EOF.
        None if h.status == 204 || h.status == 304 || h.status < 200 => 0,
        None => return None,
    };
    if rest.len() >= len {
        Some(Response { status: h.status, headers: h.headers, body: rest[..len].to_vec() })
    } else {
        None
    }
}

/// After EOF: accept a body delimited by connection close.
pub fn parse_response_eof(buf: &[u8]) -> Option<Response> {
    if let Some(r) = parse_response(buf) {
        return Some(r);
    }
    let h = parse_head(buf)?;
    if h.chunked || h.content_length.is_some() {
        return None; // truncated
    }
    let body = buf[h.body_start..].to_vec();
    Some(Response { status: h.status, headers: h.headers, body })
}

fn decode_chunked(mut rest: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let eol = find(rest, b"\r\n")?;
        let size_str = std::str::from_utf8(&rest[..eol]).ok()?;
        let size = usize::from_str_radix(size_str.split(';').next()?.trim(), 16).ok()?;
        rest = &rest[eol + 2..];
        if size == 0 {
            return Some(out);
        }
        if rest.len() < size + 2 {
            return None;
        }
        out.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn head_format() {
        let h = build_request_head(47821, "POST", "/ask", &[("Authorization", "Bearer abc"), ("X-Orbi-Nonce", "00ff")], Some(12));
        assert!(h.starts_with("POST /ask HTTP/1.1\r\n"));
        assert!(h.contains("Host: 127.0.0.1:47821\r\n"));
        assert!(h.contains("Authorization: Bearer abc\r\n"));
        assert!(h.contains("X-Orbi-Nonce: 00ff\r\n"));
        assert!(h.contains("Content-Length: 12\r\n"));
        let g = build_request_head(1, "GET", "/hello", &[("X-Evil", "a\r\nInjected: 1")], None);
        assert!(g.starts_with("GET /hello HTTP/1.1\r\n"));
        assert!(!g.contains("Content-Length") && !g.contains("Injected"));
        assert!(!h.contains("Origin"));
        assert!(h.ends_with("\r\n\r\n"));
    }

    #[test]
    fn parses_content_length() {
        let r = parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nX-Orbi-Proof: AB\r\n\r\nhi").unwrap();
        assert_eq!((r.status, r.body.as_slice()), (200, &b"hi"[..]));
        assert_eq!(r.header("x-orbi-proof"), Some("AB"));
        assert_eq!(r.header("X-Orbi-Proof"), Some("AB"));
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhi").is_none());
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nContent-Le").is_none());
    }

    #[test]
    fn parses_chunked_and_eof_and_204() {
        let r = parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nhi\r\n3\r\n!!!\r\n0\r\n\r\n").unwrap();
        assert_eq!(r.body, b"hi!!!");
        let r = parse_response_eof(b"HTTP/1.0 200 OK\r\n\r\nbody").unwrap();
        assert_eq!(r.body, b"body");
        let r = parse_response(b"HTTP/1.1 204 No Content\r\n\r\n").unwrap();
        assert_eq!(r.status, 204);
        assert!(parse_response_eof(b"garbage").is_none());
    }

    #[test]
    fn roundtrip_against_local_server() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let t = thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = vec![0u8; 4096];
            let mut got = Vec::new();
            loop {
                let n = s.read(&mut buf).unwrap();
                got.extend_from_slice(&buf[..n]);
                if let Some(i) = find(&got, b"\r\n\r\n") {
                    if got.len() >= i + 4 + 7 {
                        break;
                    }
                }
            }
            s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\n\r\n{\"decision\":\"allow\"}").unwrap();
            String::from_utf8(got).unwrap()
        });
        let r = request(port, "POST", "/ask", &[("Authorization", "Bearer tok")], Some(b"{\"a\":1}"), Duration::from_millis(300), Duration::from_secs(2)).unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.body, b"{\"decision\":\"allow\"}");
        let req = t.join().unwrap();
        assert!(req.contains("Authorization: Bearer tok"));
        assert!(req.ends_with("{\"a\":1}"));
    }

    #[test]
    fn refused_is_fast_error() {
        // Bind then drop to get a (very likely) closed port.
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let t = Instant::now();
        assert!(request(port, "POST", "/ask", &[], Some(b"{}"), Duration::from_millis(300), Duration::from_secs(5)).is_err());
        assert!(t.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn read_timeout_is_enforced() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let _t = thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            thread::sleep(Duration::from_secs(3));
            drop(s);
        });
        let t = Instant::now();
        assert!(request(port, "POST", "/ask", &[], Some(b"{}"), Duration::from_millis(300), Duration::from_millis(300)).is_err());
        assert!(t.elapsed() < Duration::from_secs(2));
    }
}
