//! The self-signed-certificate path: the error must be recognizable as a TLS trust
//! failure, and the `insecure_tls` opt-in must then succeed.
//!
//! Needs a self-signed HTTPS WebDAV endpoint on 127.0.0.1:18443. To create one:
//!
//! ```sh
//! openssl req -x509 -newkey rsa:2048 -keyout key.pem -out cert.pem -days 2 -nodes \
//!   -subj "/CN=localhost" -addext "subjectAltName=DNS:localhost"
//! python3 - <<'PY'
//! import http.server, socketserver, ssl
//! M = b'<?xml version="1.0"?><d:multistatus xmlns:d="DAV:"><d:response><d:href>/</d:href>'\
//!     b'<d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop>'\
//!     b'<d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>'
//! class H(http.server.BaseHTTPRequestHandler):
//!     protocol_version = "HTTP/1.1"
//!     def do_PROPFIND(self):
//!         self.rfile.read(int(self.headers.get("Content-Length") or 0))
//!         self.send_response(207); self.send_header("DAV", "1,2")
//!         self.send_header("Content-Length", str(len(M))); self.end_headers()
//!         self.wfile.write(M)
//! ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); ctx.load_cert_chain("cert.pem", "key.pem")
//! socketserver.TCPServer.allow_reuse_address = True
//! with socketserver.TCPServer(("127.0.0.1", 18443), H) as s:
//!     s.socket = ctx.wrap_socket(s.socket, server_side=True); s.serve_forever()
//! PY
//! ```
//!
//! Then: `cargo test --test webdav_tls -- --ignored --nocapture`
use lazy_transfer::transfer::types::{WebDavAuth, WebDavConfig};
use lazy_transfer::transfer::webdav_backend::{is_tls_untrusted, WebDavBackend};

fn cfg(insecure_tls: bool) -> WebDavConfig {
    WebDavConfig {
        url: "https://localhost:18443/".to_string(),
        auth: WebDavAuth::Anonymous,
        insecure_tls,
    }
}

#[test]
#[ignore]
fn self_signed_is_rejected_then_accepted_on_opt_in() {
    let err = match WebDavBackend::connect(&cfg(false)) {
        Ok(_) => panic!("a self-signed certificate must not be trusted by default"),
        Err(e) => e,
    };
    println!("rustls said: {err}");
    assert!(
        is_tls_untrusted(&err),
        "the retry heuristic must recognize this message: {err}"
    );

    // The per-connection opt-in, which is what the confirm dialog triggers.
    WebDavBackend::connect(&cfg(true)).expect("insecure_tls must accept the certificate");
}
