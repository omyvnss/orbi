//! Server authenticity (docs/APP.md, "anti port-squatting"): never trust a
//! process on Orbi's port until it proves it knows the token.
//!
//! 1. `GET /hello` + `X-Orbi-Nonce: n1` (no token) → must answer 200 with
//!    `X-Orbi-Proof: hex(HMAC-SHA256(token, "hello\n" + port + "\n" + n1))`,
//!    where `port` is the port this hook dialled — so a squatter can't relay
//!    the nonce to the real Orbi listening somewhere else.
//! 2. The real request with the bearer token and a fresh nonce `n2` → its
//!    response must carry `X-Orbi-Proof: hex(HMAC(token, "resp\n" + n2 + "\n" + body))`.
//!
//! Any mismatch, missing header or I/O error → None ("Orbi isn't there").

use crate::http;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::io::Read;
use std::time::Duration;

type HmacSha256 = Hmac<Sha256>;

/// 16 random bytes from the kernel as 32 lowercase hex chars.
pub fn nonce() -> Option<String> {
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom").ok()?.read_exact(&mut b).ok()?;
    Some(hex(&b))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 || !s.is_ascii() {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

fn mac(token: &str, parts: &[&[u8]]) -> HmacSha256 {
    let mut m = HmacSha256::new_from_slice(token.as_bytes()).expect("HMAC takes any key length");
    for p in parts {
        m.update(p);
    }
    m
}

/// hex(HMAC-SHA256(token, concat(parts))) — what a real Orbi sends.
#[cfg(test)]
pub fn proof(token: &str, parts: &[&[u8]]) -> String {
    hex(&mac(token, parts).finalize().into_bytes())
}

/// Constant-time check of a presented hex proof.
pub fn verify(token: &str, parts: &[&[u8]], presented: Option<&str>) -> bool {
    let Some(p) = presented.and_then(unhex) else { return false };
    mac(token, parts).verify_slice(&p).is_ok()
}

pub struct Conn {
    pub port: u16,
    pub token: String,
}

/// Real Orbi answers `/hello` in well under a millisecond; anything slower
/// is not worth delaying the agent for.
const HELLO_TIMEOUT: Duration = Duration::from_millis(250);

/// Step 1: prove the server on `port` knows our token.
fn hello(conn: &Conn, connect: Duration, total: Duration) -> Option<()> {
    let n = nonce()?;
    let wait = HELLO_TIMEOUT.min(total);
    let r = http::request(conn.port, "GET", "/hello", &[("X-Orbi-Nonce", &n)], None, connect, wait).ok()?;
    if r.status != 200 {
        return None;
    }
    let port = conn.port.to_string();
    verify(&conn.token, &[b"hello\n", port.as_bytes(), b"\n", n.as_bytes()], r.header("x-orbi-proof")).then_some(())
}

/// A verified request: `(status, body)` only if both handshake steps check out.
pub fn call(conn: &Conn, method: &str, path: &str, body: Option<&[u8]>, connect: Duration, total: Duration) -> Option<(u16, Vec<u8>)> {
    hello(conn, connect, total)?;
    let n = nonce()?;
    let auth = format!("Bearer {}", conn.token);
    let r = http::request(conn.port, method, path, &[("Authorization", &auth), ("X-Orbi-Nonce", &n)], body, connect, total).ok()?;
    let ok = verify(&conn.token, &[b"resp\n", n.as_bytes(), b"\n", &r.body], r.header("x-orbi-proof"));
    ok.then_some((r.status, r.body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn nonce_and_hex() {
        let a = nonce().unwrap();
        let b = nonce().unwrap();
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
        assert_eq!(unhex("00ff10"), Some(vec![0, 255, 16]));
        assert_eq!(unhex("0"), None);
        assert_eq!(unhex("zz"), None);
    }

    #[test]
    fn proof_matches_known_vector() {
        // RFC 4231 test case 2: key "Jefe", data "what do ya want for nothing?"
        assert_eq!(
            proof("Jefe", &[b"what do ya want ", b"for nothing?"]),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        let p = proof("tok", &[b"hello\n", b"abc"]);
        assert!(verify("tok", &[b"hello\nabc"], Some(&p)));
        assert!(verify("tok", &[b"hello\nabc"], Some(&p.to_uppercase())));
        assert!(!verify("other", &[b"hello\nabc"], Some(&p)));
        assert!(!verify("tok", &[b"hello\nabd"], Some(&p)));
        assert!(!verify("tok", &[b"hello\nabc"], None));
        assert!(!verify("tok", &[b"hello\nabc"], Some("")));
    }

    fn header_of(req: &str, name: &str) -> String {
        req.lines()
            .find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.trim().to_string()))
            .unwrap_or_default()
    }

    /// A tiny fake Orbi. `key` is what it signs with (the real token, or a
    /// squatter's guess); `tamper` flips the body after signing.
    fn fake_orbi(key: &'static str, sign_hello: bool, tamper: bool) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        thread::spawn(move || {
            for stream in l.incoming().take(2) {
                let mut s = stream.unwrap();
                let mut buf = vec![0u8; 8192];
                let n = s.read(&mut buf).unwrap();
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let nonce = header_of(&req, "X-Orbi-Nonce");
                let resp = if req.starts_with("GET /hello ") {
                    let port_s = port.to_string();
                    let p = if sign_hello {
                        proof(key, &[b"hello\n", port_s.as_bytes(), b"\n", nonce.as_bytes()])
                    } else {
                        String::new()
                    };
                    format!("HTTP/1.1 200 OK\r\nX-Orbi-Proof: {p}\r\nContent-Length: 11\r\n\r\n{{\"ok\":true}}")
                } else {
                    assert_eq!(header_of(&req, "Authorization"), "Bearer tok");
                    let body = r#"{"decision":"allow","reason":"x"}"#;
                    let p = proof(key, &[b"resp\n", nonce.as_bytes(), b"\n", body.as_bytes()]);
                    let sent = if tamper { body.replace("allow", "ALLOW") } else { body.to_string() };
                    format!("HTTP/1.1 200 OK\r\nX-Orbi-Proof: {p}\r\nContent-Length: {}\r\n\r\n{sent}", sent.len())
                };
                s.write_all(resp.as_bytes()).unwrap();
            }
        });
        port
    }

    fn ask(port: u16) -> Option<(u16, Vec<u8>)> {
        let conn = Conn { port, token: "tok".into() };
        call(&conn, "POST", "/ask", Some(b"{}"), Duration::from_millis(300), Duration::from_secs(2))
    }

    #[test]
    fn verified_server_is_trusted() {
        let (st, body) = ask(fake_orbi("tok", true, false)).unwrap();
        assert_eq!(st, 200);
        assert_eq!(body, br#"{"decision":"allow","reason":"x"}"#);
    }

    #[test]
    fn squatter_is_not_trusted() {
        // doesn't know the token
        assert!(ask(fake_orbi("guess", true, false)).is_none());
        // no hello proof at all
        assert!(ask(fake_orbi("tok", false, false)).is_none());
        // body altered after signing
        assert!(ask(fake_orbi("tok", true, true)).is_none());
    }

    #[test]
    fn nothing_listening_is_none_fast() {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let t = std::time::Instant::now();
        assert!(ask(port).is_none());
        assert!(t.elapsed() < Duration::from_millis(500));
    }
}
