//! Locks down the PUT wire format, using a throwaway in-process HTTP server.
//!
//! `SendBody::from_reader` makes ureq use `Transfer-Encoding: chunked`, which
//! several WebDAV servers reject on PUT. The backend therefore sets an explicit
//! `Content-Length`. This test proves ureq honours it and does not add chunked on
//! top — the one behaviour the plan flagged as needing real verification.
//!
//! Self-contained: no external server, so it runs in CI.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;

use lazy_transfer::transfer::backend::RemoteBackend;
use lazy_transfer::transfer::types::{WebDavAuth, WebDavConfig};
use lazy_transfer::transfer::webdav_backend::WebDavBackend;

const MULTISTATUS: &str = concat!(
    r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:"><d:response>"#,
    r#"<d:href>/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/>"#,
    r#"</d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status>"#,
    r#"</d:propstat></d:response></d:multistatus>"#
);

/// One request: the method and its headers, lowercased keys.
struct Seen {
    method: String,
    headers: Vec<(String, String)>,
    body_len: usize,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

fn md5_hex(input: &str) -> String {
    use md5::{Digest, Md5};
    let mut h = Md5::new();
    h.update(input.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn handle(stream: &mut TcpStream) -> Option<Seen> {
    handle_with_challenge(stream, None)
}

/// Reads one request; when `challenge` is set, answers 401 with it instead of 207.
fn handle_with_challenge(stream: &mut TcpStream, challenge: Option<String>) -> Option<Seen> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut start = String::new();
    if reader.read_line(&mut start).ok()? == 0 {
        return None;
    }
    let method = start.split_whitespace().next()?.to_string();

    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            break;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }

    // Read exactly what was announced; a chunked body would have no length, and
    // that is precisely what this test is here to catch.
    let len: usize = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).ok()?;

    let response = if let Some(ch) = challenge {
        format!(
            "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: {ch}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
    } else if method == "PROPFIND" {
        format!(
            "HTTP/1.1 207 Multi-Status\r\nDAV: 1,2\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            MULTISTATUS.len(),
            MULTISTATUS
        )
    } else {
        "HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
    };
    stream.write_all(response.as_bytes()).ok()?;
    stream.flush().ok()?;

    Some(Seen {
        method,
        headers,
        body_len: body.len(),
    })
}

#[test]
fn put_sends_content_length_and_never_chunked() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();

    let server = thread::spawn(move || {
        // One PROPFIND for connect(), one PUT for the upload.
        for _ in 0..2 {
            let (mut stream, _) = match listener.accept() {
                Ok(v) => v,
                Err(_) => return,
            };
            if let Some(seen) = handle(&mut stream) {
                let _ = tx.send(seen);
            }
        }
    });

    let cfg = WebDavConfig {
        url: format!("http://127.0.0.1:{port}/"),
        auth: WebDavAuth::Anonymous,
        insecure_tls: false,
    };
    let backend = WebDavBackend::connect(&cfg).expect("connect");

    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("payload.bin");
    let payload = vec![9u8; 5000];
    std::fs::write(&file, &payload).unwrap();

    let handle_stream = backend
        .upload(file.to_str().unwrap(), "/payload.bin")
        .expect("upload");
    for line in handle_stream.rx.iter() {
        if line.done {
            assert!(line.err.is_none(), "upload failed: {:?}", line.err);
            break;
        }
    }
    let _ = server.join();

    let requests: Vec<Seen> = rx.try_iter().collect();
    let propfind = requests
        .iter()
        .find(|r| r.method == "PROPFIND")
        .expect("connect() must issue a PROPFIND");
    assert_eq!(propfind.header("depth"), Some("0"), "connect uses Depth: 0");

    let put = requests
        .iter()
        .find(|r| r.method == "PUT")
        .expect("upload must issue a PUT");
    assert_eq!(
        put.header("content-length"),
        Some("5000"),
        "PUT must announce an explicit length"
    );
    assert_eq!(
        put.header("transfer-encoding"),
        None,
        "ureq must not add chunked encoding on top of Content-Length"
    );
    assert_eq!(put.body_len, payload.len(), "the whole body must arrive");
}

/// The whole Digest exchange, verified server-side: challenge, then a second
/// request whose `response` hash the test recomputes itself. This is what proves
/// SabreDAV-style hosts (BigCommerce) are reachable.
#[test]
fn digest_auth_completes_the_challenge_response_exchange() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();

    const REALM: &str = "SabreDAV";
    const NONCE: &str = "6a86ecabd683c";
    const USER: &str = "alice";
    const PASSWORD: &str = "s3cret";

    let server = thread::spawn(move || {
        for i in 0..2 {
            let (mut stream, _) = match listener.accept() {
                Ok(v) => v,
                Err(_) => return,
            };
            // First exchange: refuse and advertise Digest only, like SabreDAV.
            let challenge = if i == 0 {
                Some(format!(
                    r#"Digest realm="{REALM}",qop="auth",nonce="{NONCE}",opaque="op""#
                ))
            } else {
                None
            };
            if let Some(seen) = handle_with_challenge(&mut stream, challenge) {
                let _ = tx.send(seen);
            }
        }
    });

    let cfg = WebDavConfig {
        url: format!("http://127.0.0.1:{port}/dav/"),
        auth: WebDavAuth::Digest {
            user: USER.to_string(),
            password: PASSWORD.to_string(),
        },
        insecure_tls: false,
    };
    WebDavBackend::connect(&cfg).expect("the Digest exchange must succeed");
    let _ = server.join();

    let reqs: Vec<Seen> = rx.try_iter().collect();
    assert_eq!(reqs.len(), 2, "expected a challenge then an answer");
    assert_eq!(
        reqs[0].header("authorization"),
        None,
        "nothing can be pre-computed for Digest, so the first request is bare"
    );

    let auth = reqs[1]
        .header("authorization")
        .expect("the retry must carry an Authorization header");
    assert!(auth.starts_with("Digest "), "{auth}");
    for needle in [
        format!(r#"username="{USER}""#),
        format!(r#"realm="{REALM}""#),
        format!(r#"nonce="{NONCE}""#),
        r#"uri="/dav/""#.to_string(),
        "qop=auth".to_string(),
        "nc=00000001".to_string(),
        r#"opaque="op""#.to_string(),
    ] {
        assert!(auth.contains(&needle), "{needle} missing from {auth}");
    }

    // Recompute the response the way the server would, and compare.
    let cnonce = auth
        .split("cnonce=\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .expect("cnonce");
    let ha1 = md5_hex(&format!("{USER}:{REALM}:{PASSWORD}"));
    let ha2 = md5_hex("PROPFIND:/dav/");
    let expected = md5_hex(&format!("{ha1}:{NONCE}:00000001:{cnonce}:auth:{ha2}"));
    assert!(
        auth.contains(&format!(r#"response="{expected}""#)),
        "digest response mismatch\n  got: {auth}\n  want response={expected}"
    );
}

/// A Digest-only server refuses Basic, and the message must say to pick Digest
/// rather than to re-check the password.
#[test]
fn basic_against_a_digest_only_server_says_to_choose_digest() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        // Exactly ONE request is expected: a Basic 401 must not be retried, because
        // re-sending the identical header could only fail in the same way.
        let mut count = 0usize;
        if let Ok((mut stream, _)) = listener.accept() {
            count += 1;
            let _ = handle_with_challenge(
                &mut stream,
                Some(r#"Digest realm="SabreDAV",qop="auth",nonce="n""#.to_string()),
            );
        }
        count
    });

    let cfg = WebDavConfig {
        url: format!("http://127.0.0.1:{port}/dav/"),
        auth: WebDavAuth::Basic {
            user: "alice".to_string(),
            password: "s3cret".to_string(),
        },
        insecure_tls: false,
    };
    let err = match WebDavBackend::connect(&cfg) {
        Ok(_) => panic!("Basic must not be accepted here"),
        Err(e) => e,
    };
    let requests = server.join().expect("server thread");
    assert_eq!(requests, 1, "Basic must not trigger a retry round trip");
    assert!(err.contains("Digest"), "{err}");
    assert!(
        err.contains("choisissez"),
        "should tell the user what to do: {err}"
    );
}

#[test]
fn anonymous_auth_sends_no_authorization_header() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    let server = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            if let Some(seen) = handle(&mut stream) {
                let _ = tx.send(seen);
            }
        }
    });

    let cfg = WebDavConfig {
        url: format!("http://127.0.0.1:{port}/"),
        auth: WebDavAuth::Anonymous,
        insecure_tls: false,
    };
    WebDavBackend::connect(&cfg).expect("connect");
    let _ = server.join();

    let seen = rx.recv().expect("one request");
    assert_eq!(seen.header("authorization"), None);
}

#[test]
fn basic_auth_sends_the_expected_header() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    let server = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            if let Some(seen) = handle(&mut stream) {
                let _ = tx.send(seen);
            }
        }
    });

    let cfg = WebDavConfig {
        url: format!("http://127.0.0.1:{port}/"),
        auth: WebDavAuth::Basic {
            user: "user".to_string(),
            password: "pw".to_string(),
        },
        insecure_tls: false,
    };
    WebDavBackend::connect(&cfg).expect("connect");
    let _ = server.join();

    let seen = rx.recv().expect("one request");
    // base64("user:pw")
    assert_eq!(seen.header("authorization"), Some("Basic dXNlcjpwdw=="));
}
