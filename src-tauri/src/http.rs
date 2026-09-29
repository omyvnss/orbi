//! A deliberately tiny HTTP/1.1 server for Orbi's four local routes.
//!
//! Hand-rolled rather than a crate for one reason: `/ask` long-polls, and when
//! the agent stops waiting (the user answered in the terminal, the agent was
//! interrupted) its hook process dies and the socket closes. Orbi has to see
//! that, or a request that's already been answered sits on the face and the
//! next ⌃⌥A lands on it. Owning the socket makes `peer_closed` possible.
//!
//! One request per connection (`Connection: close`), strict limits on header
//! and body size, and read timeouts so a slow client can't hold a thread.

use std::{
    io::{ErrorKind, Read, Write},
    net::TcpStream,
    time::{Duration, Instant},
};

const MAX_HEAD: usize = 16 * 1024;
/// The whole request (head and body) must arrive within this, so a client
/// dribbling bytes can't hold a thread.
const READ_DEADLINE: Duration = Duration::from_secs(2);

pub struct Request {
    pub method: String,
    pub path: String,
    headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    stream: TcpStream,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// True once the client has gone away. Non-blocking.
    pub fn peer_closed(&self) -> bool {
        if self.stream.set_nonblocking(true).is_err() {
            return false;
        }
        let mut probe = [0u8; 1];
        let closed = match self.stream.peek(&mut probe) {
            Ok(0) => true,
            Ok(_) => false,
            Err(e) if e.kind() == ErrorKind::WouldBlock => false,
            Err(_) => true,
        };
        let _ = self.stream.set_nonblocking(false);
        closed
    }

    pub fn respond(mut self, status: u16, headers: &[(&str, &str)], body: &str) {
        let reason = match status {
            200 => "OK",
            204 => "No Content",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            408 => "Request Timeout",
            413 => "Payload Too Large",
            431 => "Request Header Fields Too Large",
            503 => "Service Unavailable",
            _ => "Error",
        };
        let mut out = format!("HTTP/1.1 {status} {reason}\r\n");
        for (k, v) in headers {
            out.push_str(&format!("{k}: {v}\r\n"));
        }
        if !body.is_empty() {
            out.push_str("Content-Type: application/json\r\n");
        }
        out.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()));
        out.push_str(body);
        let _ = self.stream.write_all(out.as_bytes());
        let _ = self.stream.flush();
    }
}

/// Reads one request. On a malformed or oversized request, returns the
/// status to answer with (the caller still owns the stream via `reject`).
pub fn read_request(mut stream: TcpStream, max_body: usize) -> Result<Request, (TcpStream, u16)> {
    let deadline = Instant::now() + READ_DEADLINE;
    let _ = stream.set_write_timeout(Some(READ_DEADLINE));
    // Before each read: the time left, or None once the deadline passed.
    let arm = |s: &TcpStream| -> bool {
        match deadline.checked_duration_since(Instant::now()) {
            Some(left) if !left.is_zero() => s.set_read_timeout(Some(left)).is_ok(),
            _ => false,
        }
    };

    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            return Err((stream, 431));
        }
        if !arm(&stream) {
            return Err((stream, 408));
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return Err((stream, 400)),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };

    let Ok(head) = std::str::from_utf8(&buf[..head_end]) else {
        return Err((stream, 400));
    };
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (Some(method), Some(target)) = (first.next(), first.next()) else {
        return Err((stream, 400));
    };
    let path = target.split('?').next().unwrap_or("").to_string();
    let method = method.to_string();
    let mut headers = Vec::new();
    for line in lines {
        let Some((k, v)) = line.split_once(':') else {
            return Err((stream, 400));
        };
        headers.push((k.trim().to_string(), v.trim().to_string()));
    }
    if headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("Transfer-Encoding")) {
        // Hooks always send Content-Length; chunked uploads aren't supported.
        return Err((stream, 400));
    }
    let len = match headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("Content-Length")) {
        None => 0,
        Some((_, v)) => match v.parse::<usize>() {
            Ok(n) => n,
            Err(_) => return Err((stream, 400)),
        },
    };
    if len > max_body {
        return Err((stream, 413));
    }

    let mut body = buf[head_end + 4..].to_vec();
    if body.len() > len {
        body.truncate(len);
    }
    while body.len() < len {
        if !arm(&stream) {
            return Err((stream, 408));
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return Err((stream, 400)),
            Ok(n) => body.extend_from_slice(&chunk[..n.min(len - body.len())]),
        }
    }

    Ok(Request { method, path, headers, body, stream })
}

/// Answers a request that failed to parse.
pub fn reject(mut stream: TcpStream, status: u16) {
    let body = format!("{{\"error\":\"http {status}\"}}");
    let _ = stream.write_all(
        format!(
            "HTTP/1.1 {status} Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    );
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn roundtrip(raw: &[u8]) -> Result<(String, String, Vec<u8>), u16> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let raw = raw.to_vec();
        let client = std::thread::spawn(move || {
            let mut s = TcpStream::connect(addr).unwrap();
            s.write_all(&raw).unwrap();
            s
        });
        let (stream, _) = listener.accept().unwrap();
        let _keep = client.join().unwrap();
        read_request(stream, 64)
            .map(|r| (r.method.clone(), r.path.clone(), r.body.clone()))
            .map_err(|(_, s)| s)
    }

    #[test]
    fn parses_a_post() {
        let r = roundtrip(b"POST /ask?x=1 HTTP/1.1\r\nHost: a\r\nContent-Length: 2\r\n\r\n{}").unwrap();
        assert_eq!(r, ("POST".into(), "/ask".into(), b"{}".to_vec()));
    }

    #[test]
    fn rejects_oversized_and_chunked() {
        assert_eq!(roundtrip(b"POST / HTTP/1.1\r\nContent-Length: 999\r\n\r\n").unwrap_err(), 413);
        assert_eq!(
            roundtrip(b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n").unwrap_err(),
            400
        );
        assert_eq!(roundtrip(b"POST / HTTP/1.1\r\nContent-Length: x\r\n\r\n").unwrap_err(), 400);
    }

    #[test]
    fn notices_a_closed_peer() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = std::thread::spawn(move || {
            let mut s = TcpStream::connect(addr).unwrap();
            s.write_all(b"GET /health HTTP/1.1\r\n\r\n").unwrap();
            s
        });
        let (stream, _) = listener.accept().unwrap();
        let s = client.join().unwrap();
        let req = read_request(stream, 64).map_err(|_| ()).unwrap();
        assert!(!req.peer_closed());
        drop(s);
        std::thread::sleep(Duration::from_millis(50));
        assert!(req.peer_closed());
    }
}
