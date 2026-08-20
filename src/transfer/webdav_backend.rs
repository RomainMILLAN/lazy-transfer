use std::io::{Read, Write};
use std::time::Duration;

use ureq::http;

use crate::transfer::backend::RemoteBackend;
use crate::transfer::stream::{spawn_transfer, ByteProgress, ProgressReader, StreamHandle};
use crate::transfer::types::{FileEntry, WebDavConfig};

const DAV_NS: &str = "DAV:";

/// Explicit `prop` rather than `allprop`: Nextcloud answers `allprop` with dozens of
/// `oc:`/`nc:` properties, and an explicit list keeps parsing deterministic.
const PROPFIND_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:"><d:prop>
<d:resourcetype/><d:getcontentlength/><d:getlastmodified/><d:displayname/>
</d:prop></d:propfind>"#;

/// PROPFIND on a large directory easily exceeds ureq's ~10 MiB default read limit.
const MAX_BODY: u64 = 64 * 1024 * 1024;
const CHUNK: usize = 64 * 1024;

/// Guard against a symlink/loop pathology on the server while walking.
const MAX_DEPTH: usize = 64;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Translates an HTTP status into something a user can act on.
fn dav_error(op: &str, path: &str, status: u16) -> String {
    let hint = match status {
        // By far the most common WebDAV misconfiguration, which is why it gets its
        // own line — and why redirects must stay visible (see `build_agent`).
        301 | 302 | 307 | 308 => "redirection: vérifiez l'URL (slash final manquant ?)",
        401 => "authentification refusée: vérifiez identifiant/mot de passe ou le token",
        403 => "accès interdit par le serveur",
        404 => "introuvable sur le serveur",
        405 => {
            "méthode non autorisée: la cible existe déjà, ou WebDAV n'est pas activé sur cette URL"
        }
        409 => "conflit: le dossier parent n'existe pas",
        412 => "la destination existe déjà",
        423 => "ressource verrouillée par un autre client",
        502..=504 => "serveur indisponible",
        507 => "quota dépassé (espace insuffisant)",
        _ => "erreur serveur",
    };
    format!("WebDAV {op} '{path}' → HTTP {status}: {hint}")
}

/// Builds the 401 message from the server's `WWW-Authenticate` challenge.
///
/// Saying "check your password" when the server never offered Basic sends the user
/// chasing the wrong problem — their credentials may be perfectly correct.
fn unauthorized_error(op: &str, path: &str, challenge: Option<&str>) -> String {
    let lower = challenge.unwrap_or("").to_ascii_lowercase();
    let offers_basic = lower.contains("basic");
    let offers_bearer = lower.contains("bearer");
    let offers_digest = lower.contains("digest");

    // Digest-only is the notable case: SabreDAV-based hosts (BigCommerce among
    // them) advertise nothing else, and Digest is not implemented here.
    if offers_digest && !offers_basic && !offers_bearer {
        return format!(
            "WebDAV {op} '{path}' → HTTP 401: ce serveur exige l'authentification Digest, \
             qui n'est pas supportée (il n'accepte ni Basic ni Bearer)"
        );
    }
    let mut msg = format!(
        "WebDAV {op} '{path}' → HTTP 401: authentification refusée: vérifiez identifiant/mot de passe ou le token"
    );
    if offers_digest {
        msg.push_str(" (le serveur accepte aussi Digest, non supporté)");
    }
    msg
}

/// Heuristic: does this error look like a TLS trust failure?
///
/// It lives here, next to the code that produces the message, because this module
/// is the only one that knows what rustls says. The UI asks; it does not guess.
pub fn is_tls_untrusted(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("unknownissuer")
        || m.contains("self-signed")
        || m.contains("self signed")
        || m.contains("invalid peer certificate")
        || m.contains("certificate verify failed")
        || m.contains("certificate")
}

// ---------------------------------------------------------------------------
// URLs
// ---------------------------------------------------------------------------

/// Logical path ("/a/b c.txt") -> encoded absolute URL.
///
/// Never concatenates URL strings: `path_segments_mut` applies the correct
/// percent-encode set (space, `#`, `?`, `%`, non-ASCII as UTF-8).
/// `as_collection` appends the trailing '/' that PROPFIND and MKCOL need.
fn url_for(base: &url::Url, logical: &str, as_collection: bool) -> Result<String, String> {
    let mut u = base.clone();
    {
        // A Result rather than .expect(): a panic in a raw-mode TUI leaves the
        // terminal wrecked, and costs more than propagating an error.
        let mut segs = u
            .path_segments_mut()
            .map_err(|_| "URL de base invalide".to_string())?;
        // Drop the empty segment produced by the base's trailing '/'.
        segs.pop_if_empty();
        for s in logical
            .split('/')
            .filter(|s| !s.is_empty() && *s != "." && *s != "..")
        {
            segs.push(s);
        }
        if as_collection {
            segs.push("");
        }
    }
    Ok(u.to_string())
}

/// Percent-decoded path of an href, with query/fragment removed.
///
/// Uses `percent_decode_str`, NOT `form_urlencoded`: the latter turns `+` into a
/// space and would corrupt a file literally named `a+b.txt`.
fn decoded_href_path(href: &str) -> String {
    let path = if href.starts_with("http://") || href.starts_with("https://") {
        match url::Url::parse(href) {
            Ok(u) => u.path().to_string(),
            Err(_) => href.to_string(),
        }
    } else {
        href.to_string()
    };
    let path = path.split(['?', '#']).next().unwrap_or(&path).to_string();
    percent_encoding::percent_decode_str(&path)
        .decode_utf8_lossy()
        .to_string()
}

/// Href of a PROPFIND response -> entry name.
///
/// `dir_path_decoded` is the decoded path of the *requested* directory.
/// Returns `None` ONLY for the "self" entry (the requested collection itself).
///
/// Self is detected by EQUALITY of the decoded paths, not by prefix stripping: a
/// failed `strip_prefix` would return None and make the entry vanish silently — one
/// rewriting proxy and a full directory would render as "Empty directory". A
/// listing that lies by omission is worse than one that errors, because nobody
/// challenges it. Anything that is not self falls back to its last segment.
fn href_to_name(dir_path_decoded: &str, href: &str) -> Option<String> {
    let decoded = decoded_href_path(href);
    let here = decoded.trim_end_matches('/');
    let dir = dir_path_decoded.trim_end_matches('/');
    if here == dir {
        return None;
    }
    let name = here.rsplit('/').next().unwrap_or("");
    if name.is_empty() {
        // Only reachable for "/" itself, which is already handled above.
        log::warn!("webdav: unusable href '{href}' under '{dir_path_decoded}'");
        return None;
    }
    if !decoded.starts_with(dir_path_decoded) {
        // Kept, but diagnosable: a phantom entry in debug.log beats a missing one.
        log::warn!(
            "webdav: href '{decoded}' outside requested dir '{dir_path_decoded}', keeping '{name}'"
        );
    }
    Some(name.to_string())
}

// ---------------------------------------------------------------------------
// PROPFIND parsing
// ---------------------------------------------------------------------------

/// "HTTP/1.1 200 OK" -> true
fn status_is_2xx(status: &str) -> bool {
    status
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse::<u16>().ok())
        .map(|c| (200..300).contains(&c))
        .unwrap_or(false)
}

/// `getlastmodified` is RFC 1123. Formatted like `format_unix_time` in the SFTP
/// backend so every protocol renders dates identically — and that format sorts
/// lexicographically, which is what the date sort relies on.
fn format_http_date(raw: &str) -> String {
    const OUT: &str = "%Y-%m-%d %H:%M";
    let raw = raw.trim();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(raw) {
        return dt.naive_utc().format(OUT).to_string();
    }
    // chrono rejects a weekday that contradicts the date ("Tue" for a Monday), and
    // some servers get it wrong. The weekday is redundant, so drop it and retry
    // rather than blanking the whole date column over it.
    if let Some((_, rest)) = raw.split_once(", ") {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(rest) {
            return dt.naive_utc().format(OUT).to_string();
        }
    }
    // A few servers answer ISO 8601 instead.
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return dt.naive_utc().format(OUT).to_string();
    }
    // No date rather than an invented one.
    String::new()
}

/// Parses a `multistatus` body into entries. Pure: tested on XML literals.
fn parse_propfind(xml: &str, dir_path_decoded: &str) -> Result<Vec<FileEntry>, String> {
    let doc =
        roxmltree::Document::parse(xml).map_err(|e| format!("réponse PROPFIND illisible: {e}"))?;

    if !doc
        .descendants()
        .any(|n| n.has_tag_name((DAV_NS, "multistatus")))
    {
        return Err("réponse inattendue du serveur (pas de multistatus)".to_string());
    }

    let mut out = Vec::new();
    for resp in doc
        .descendants()
        .filter(|n| n.has_tag_name((DAV_NS, "response")))
    {
        let href = match resp
            .children()
            .find(|n| n.has_tag_name((DAV_NS, "href")))
            .and_then(|n| n.text())
        {
            Some(h) => h,
            None => continue,
        };
        let name = match href_to_name(dir_path_decoded, href) {
            Some(n) => n,
            None => continue,
        };

        let mut is_dir = false;
        let mut size = 0u64;
        let mut modified = String::new();

        // A <response> may carry several <propstat>: one 200 with the properties
        // that exist and one 404 with those that do not. Reading the 404 block
        // would overwrite good values with empty ones.
        for ps in resp
            .children()
            .filter(|n| n.has_tag_name((DAV_NS, "propstat")))
        {
            let status = ps
                .children()
                .find(|n| n.has_tag_name((DAV_NS, "status")))
                .and_then(|n| n.text())
                .unwrap_or("");
            if !status_is_2xx(status) {
                continue;
            }
            let prop = match ps.children().find(|n| n.has_tag_name((DAV_NS, "prop"))) {
                Some(p) => p,
                None => continue,
            };
            if let Some(rt) = prop
                .children()
                .find(|n| n.has_tag_name((DAV_NS, "resourcetype")))
            {
                if rt
                    .children()
                    .any(|n| n.has_tag_name((DAV_NS, "collection")))
                {
                    is_dir = true;
                }
            }
            if let Some(t) = prop
                .children()
                .find(|n| n.has_tag_name((DAV_NS, "getcontentlength")))
                .and_then(|n| n.text())
            {
                size = t.trim().parse().unwrap_or(0);
            }
            if let Some(t) = prop
                .children()
                .find(|n| n.has_tag_name((DAV_NS, "getlastmodified")))
                .and_then(|n| n.text())
            {
                modified = format_http_date(t);
            }
        }

        if is_dir {
            // Some servers report a bogus content length on collections.
            size = 0;
        }

        out.push(FileEntry {
            name,
            is_dir,
            size,
            modified,
            // WebDAV has no POSIX bits. An empty column says "unknown"; a filled one
            // would say "here is the truth" and someone would act on it.
            permissions: String::new(),
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Clonable WebDAV client. `ureq::Agent` is `Clone + Send + Sync` and clones share
/// the connection pool, so a transfer thread gets an OWNED value.
///
/// That is what lets this backend avoid the raw-pointer `unsafe` the FTP backend
/// needs (it passes `&self.ftp as *const Mutex<_> as usize` into its thread), and
/// it also makes two transfers genuinely concurrent instead of Mutex-serialized.
#[derive(Clone)]
struct DavClient {
    agent: ureq::Agent,
    base: url::Url,
    /// Pre-computed header value, so the secret itself never lives here.
    auth: Option<String>,
}

impl DavClient {
    fn url_for_file(&self, logical: &str) -> Result<String, String> {
        url_for(&self.base, logical, false)
    }

    fn url_for_dir(&self, logical: &str) -> Result<String, String> {
        url_for(&self.base, logical, true)
    }

    /// 201 = created, 405 = already there. Both count as success, which is what
    /// makes resuming an interrupted directory upload idempotent.
    fn ensure_collection(&self, logical: &str) -> Result<(), String> {
        let url = self.url_for_dir(logical)?;
        let (status, _) = self.request("MKCOL", &url, &[], None)?;
        match status {
            201 | 405 => Ok(()),
            s => Err(dav_error("MKCOL", logical, s)),
        }
    }

    /// Sends a request and returns `(status, body)`.
    ///
    /// Non-standard methods (PROPFIND/MKCOL/MOVE) cannot go through the
    /// `agent.get()`/`agent.put()` helpers: they need `http::Request` + `agent.run`.
    fn request(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, String)],
        body: Option<&'static str>,
    ) -> Result<(u16, String), String> {
        let m = http::Method::from_bytes(method.as_bytes())
            .map_err(|e| format!("méthode HTTP invalide '{method}': {e}"))?;
        let mut builder = http::Request::builder().method(m).uri(url);
        if let Some(a) = &self.auth {
            builder = builder.header(http::header::AUTHORIZATION, a);
        }
        for (k, v) in headers {
            builder = builder.header(*k, v);
        }

        let res = match body {
            Some(b) => {
                let req = builder
                    .body(b)
                    .map_err(|e| format!("requête {method} invalide: {e}"))?;
                self.agent.run(req)
            }
            None => {
                let req = builder
                    .body(())
                    .map_err(|e| format!("requête {method} invalide: {e}"))?;
                self.agent.run(req)
            }
        };

        let mut res = match res {
            Ok(r) => r,
            // Belt and braces: even with max_redirects_will_error(false) set, treat a
            // redirect error as an unknown 3xx so the collection retry still fires.
            Err(ureq::Error::TooManyRedirects) => return Ok((301, String::new())),
            Err(e) => return Err(format!("WebDAV {method} {url}: {e}")),
        };

        let status = res.status().as_u16();
        if status == 401 {
            let challenge = res
                .headers()
                .get("www-authenticate")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            return Err(unauthorized_error(method, url, challenge.as_deref()));
        }
        let text = res
            .body_mut()
            .with_config()
            .limit(MAX_BODY)
            .read_to_string()
            .unwrap_or_default();
        Ok((status, text))
    }

    /// Issues `method` against the resource form, then retries ONCE against the
    /// collection form when the status suggests the target is a collection.
    ///
    /// `RemoteBackend::delete`/`rename` do not carry `is_dir` — the UI knows it but
    /// the trait drops it — and Apache mod_dav answers 301 for a collection
    /// addressed without its trailing slash. Without this, deleting a directory
    /// would fail on the most destructive gesture in the app.
    ///
    /// On a double failure the FIRST status is reported: a rename whose destination
    /// parent is missing answers 409, and the retry's 404 would say "not found"
    /// instead of "the parent directory does not exist".
    fn request_resource_or_collection(
        &self,
        method: &str,
        logical: &str,
        extra: &dyn Fn(bool) -> Result<Vec<(&'static str, String)>, String>,
        ok: &[u16],
    ) -> Result<(u16, String), String> {
        let first_url = self.url_for_file(logical)?;
        let (status, body) = self.request(method, &first_url, &extra(false)?, None)?;
        if ok.contains(&status) {
            return Ok((status, body));
        }
        if matches!(status, 301 | 302 | 307 | 308 | 404 | 409) {
            let dir_url = self.url_for_dir(logical)?;
            let (retry_status, retry_body) = self.request(method, &dir_url, &extra(true)?, None)?;
            if ok.contains(&retry_status) {
                return Ok((retry_status, retry_body));
            }
            // Report the first status, which is the informative one.
            return Err(dav_error(method, logical, status));
        }
        Err(dav_error(method, logical, status))
    }
}

fn build_agent(insecure_tls: bool) -> ureq::Agent {
    let mut cfg = ureq::Agent::config_builder()
        // Without this, ureq REFUSES PROPFIND/MKCOL/MOVE outright.
        .allow_non_standard_methods(true)
        // We read the status ourselves to build useful messages.
        .http_status_as_error(false)
        // Two settings, both required. Following a redirect is actively dangerous
        // here: on 301/302/303 ureq REWRITES the method to GET, so a redirected
        // MOVE or DELETE would "succeed" while doing nothing at all.
        .max_redirects(0)
        // ...and the default for this one is `true`, which would surface a 301 as
        // Err(TooManyRedirects) instead of a readable status — killing both the
        // 3xx hint in dav_error and the collection retry above.
        .max_redirects_will_error(false)
        .user_agent("lazy-transfer/0.1")
        .timeout_connect(Some(Duration::from_secs(10)))
        // No global cap: an upload may legitimately run for a long time.
        .timeout_global(None);
    if insecure_tls {
        cfg = cfg.tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::Rustls)
                .disable_verification(true)
                .build(),
        );
    }
    cfg.build().into()
}

/// WebDAV backend. No `Mutex`, no `unsafe`: every field is already Send + Sync.
pub struct WebDavBackend {
    client: DavClient,
    /// Percent-decoded path of `base`, starting and ending with '/'.
    base_path_decoded: String,
}

impl WebDavBackend {
    /// Builds AND validates. Doing the I/O in the constructor is deliberate: an
    /// unreachable connection must not exist as an object, which is also why every
    /// testable helper above is a free function that needs no `Agent`.
    pub fn connect(cfg: &WebDavConfig) -> Result<Self, String> {
        // Re-parsing the already-normalized URL with the SAME parser is idempotent,
        // not a duplicated invariant. It is also the backstop for a hand-edited
        // connections.json, which `webdav_saved` deliberately does not validate.
        let base = url::Url::parse(&cfg.url).map_err(|e| format!("URL invalide: {e}"))?;
        let base_path_decoded = percent_encoding::percent_decode_str(base.path())
            .decode_utf8_lossy()
            .to_string();

        let backend = WebDavBackend {
            client: DavClient {
                agent: build_agent(cfg.insecure_tls),
                base,
                auth: cfg.auth.header_value(),
            },
            base_path_decoded,
        };
        backend.test_connection()?;
        Ok(backend)
    }

    /// Decoded path of a logical directory, used to spot the PROPFIND "self" entry.
    fn dir_path_decoded(&self, logical: &str) -> String {
        let rel = logical.trim_matches('/');
        if rel.is_empty() {
            self.base_path_decoded.clone()
        } else {
            format!("{}{}/", self.base_path_decoded, rel)
        }
    }

    fn propfind(&self, logical: &str, depth: &str) -> Result<Vec<FileEntry>, String> {
        let url = self.client.url_for_dir(logical)?;
        let headers = [
            ("Depth", depth.to_string()),
            ("Content-Type", "application/xml; charset=utf-8".to_string()),
        ];
        let (status, body) =
            self.client
                .request("PROPFIND", &url, &headers, Some(PROPFIND_BODY))?;
        if status != 207 && status != 200 {
            return Err(dav_error("PROPFIND", logical, status));
        }
        parse_propfind(&body, &self.dir_path_decoded(logical))
    }

    fn put_file(
        client: &DavClient,
        local: &std::path::Path,
        logical: &str,
        progress: &mut ByteProgress,
    ) -> Result<(), String> {
        let file = std::fs::File::open(local)
            .map_err(|e| format!("ouverture {}: {e}", local.display()))?;
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let url = client.url_for_file(logical)?;

        let mut reader = ProgressReader::new(std::io::BufReader::new(file), progress);
        let mut builder = http::Request::builder()
            .method(http::Method::PUT)
            .uri(&url)
            // SendBody::from_reader would otherwise use Transfer-Encoding: chunked,
            // which several WebDAV servers reject on PUT. ureq honours an explicit
            // Content-Length.
            .header(http::header::CONTENT_LENGTH, len.to_string())
            .header(http::header::CONTENT_TYPE, "application/octet-stream");
        if let Some(a) = &client.auth {
            builder = builder.header(http::header::AUTHORIZATION, a);
        }
        let req = builder
            .body(ureq::SendBody::from_reader(&mut reader))
            .map_err(|e| format!("requête PUT invalide: {e}"))?;

        let res = client
            .agent
            .run(req)
            .map_err(|e| format!("WebDAV PUT '{logical}': {e}"))?;
        match res.status().as_u16() {
            200 | 201 | 204 => Ok(()),
            s => Err(dav_error("PUT", logical, s)),
        }
    }

    /// Streams in 64 KiB chunks: the file never lands in RAM in full, unlike the
    /// FTP backend's `retr_as_buffer`.
    fn get_file(
        client: &DavClient,
        logical: &str,
        local: &std::path::Path,
        progress: &mut ByteProgress,
        set_total: bool,
    ) -> Result<(), String> {
        let url = client.url_for_file(logical)?;
        let mut builder = http::Request::builder().method(http::Method::GET).uri(&url);
        if let Some(a) = &client.auth {
            builder = builder.header(http::header::AUTHORIZATION, a);
        }
        let req = builder
            .body(())
            .map_err(|e| format!("requête GET invalide: {e}"))?;
        let mut res = client
            .agent
            .run(req)
            .map_err(|e| format!("WebDAV GET '{logical}': {e}"))?;
        let status = res.status().as_u16();
        if status != 200 {
            return Err(dav_error("GET", logical, status));
        }

        if set_total {
            // Reliable because the gzip feature is disabled: no transparent decoding.
            if let Some(n) = res
                .headers()
                .get(http::header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
            {
                progress.set_total(n);
            }
        }

        if let Some(parent) = local.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("création {}: {e}", parent.display()))?;
        }
        let mut out = std::io::BufWriter::new(
            std::fs::File::create(local)
                .map_err(|e| format!("création {}: {e}", local.display()))?,
        );
        let mut reader = res.body_mut().as_reader();
        let mut buf = vec![0u8; CHUNK];
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| format!("lecture réseau '{logical}': {e}"))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])
                .map_err(|e| format!("écriture locale {}: {e}", local.display()))?;
            progress.advance(n as u64);
        }
        out.flush().map_err(|e| format!("flush: {e}"))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Local / remote directory walks
// ---------------------------------------------------------------------------

enum LocalItem {
    Dir(std::path::PathBuf),
    File(std::path::PathBuf),
}

/// PRE-ORDER walk: a parent is always listed before its children.
///
/// This is a functional guarantee, not a style choice: MKCOL answers 409 when an
/// ancestor is missing and WebDAV has no `mkdir -p`.
fn walk_local(root: &std::path::Path) -> Result<(Vec<LocalItem>, u64), String> {
    let mut items = Vec::new();
    let mut total = 0u64;
    fn rec(
        dir: &std::path::Path,
        items: &mut Vec<LocalItem>,
        total: &mut u64,
        depth: usize,
    ) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err(format!("arborescence trop profonde: {}", dir.display()));
        }
        let entries =
            std::fs::read_dir(dir).map_err(|e| format!("lecture {}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("lecture {}: {e}", dir.display()))?;
            let path = entry.path();
            let meta = entry
                .metadata()
                .map_err(|e| format!("stat {}: {e}", path.display()))?;
            if meta.is_dir() {
                items.push(LocalItem::Dir(path.clone()));
                rec(&path, items, total, depth + 1)?;
            } else if meta.is_file() {
                *total += meta.len();
                items.push(LocalItem::File(path));
            }
        }
        Ok(())
    }
    rec(root, &mut items, &mut total, 0)?;
    Ok((items, total))
}

struct RemoteItem {
    logical: String,
    rel: String,
    is_dir: bool,
}

/// Recursive `Depth: 1` walk.
///
/// `Depth: infinity` in a single request is not portable: Apache refuses it by
/// default (`DavDepthInfinity off` → 403) and so does Nextcloud. The extra requests
/// are negligible next to the bytes transferred, and they buy exact progress.
fn walk_remote(
    backend: &WebDavBackend,
    root_logical: &str,
) -> Result<(Vec<RemoteItem>, u64), String> {
    let mut items = Vec::new();
    let mut total = 0u64;
    let mut queue = vec![(root_logical.to_string(), String::new(), 0usize)];
    while let Some((logical, rel, depth)) = queue.pop() {
        if depth > MAX_DEPTH {
            return Err(format!("arborescence distante trop profonde: {logical}"));
        }
        for entry in backend.propfind(&logical, "1")? {
            if entry.name == "." || entry.name == ".." {
                continue;
            }
            let child_logical = format!("{}/{}", logical.trim_end_matches('/'), entry.name);
            let child_rel = if rel.is_empty() {
                entry.name.clone()
            } else {
                format!("{}/{}", rel, entry.name)
            };
            if entry.is_dir {
                items.push(RemoteItem {
                    logical: child_logical.clone(),
                    rel: child_rel.clone(),
                    is_dir: true,
                });
                queue.push((child_logical, child_rel, depth + 1));
            } else {
                total += entry.size;
                items.push(RemoteItem {
                    logical: child_logical,
                    rel: child_rel,
                    is_dir: false,
                });
            }
        }
    }
    Ok((items, total))
}

fn last_segment(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or("")
}

// ---------------------------------------------------------------------------
// RemoteBackend
// ---------------------------------------------------------------------------

impl RemoteBackend for WebDavBackend {
    fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>, String> {
        let mut entries = self.propfind(path, "1")?;
        entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });
        if path != "/" {
            entries.insert(
                0,
                FileEntry {
                    name: "..".to_string(),
                    is_dir: true,
                    size: 0,
                    modified: String::new(),
                    permissions: String::new(),
                },
            );
        }
        Ok(entries)
    }

    fn mkdir(&self, path: &str) -> Result<(), String> {
        let url = self.client.url_for_dir(path)?;
        let (status, _) = self.client.request("MKCOL", &url, &[], None)?;
        match status {
            201 => Ok(()),
            s => Err(dav_error("MKCOL", path, s)),
        }
    }

    fn delete(&self, path: &str) -> Result<(), String> {
        // The server handles recursion (implicit Depth: infinity), so unlike FTP
        // there is no client-side walk.
        let (status, body) = self.client.request_resource_or_collection(
            "DELETE",
            path,
            &|_| Ok(vec![]),
            &[200, 204, 207],
        )?;
        if status == 207 {
            // Multi-status: a partial delete must not be reported as success.
            let failures = parse_multistatus_failures(&body);
            if !failures.is_empty() {
                return Err(format!("WebDAV DELETE '{path}': {}", failures.join("; ")));
            }
        }
        Ok(())
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        let dest_file = self.client.url_for_file(to)?;
        let dest_dir = self.client.url_for_dir(to)?;
        self.client.request_resource_or_collection(
            "MOVE",
            from,
            &|as_collection| {
                let dest = if as_collection { &dest_dir } else { &dest_file };
                Ok(vec![
                    ("Destination", dest.clone()),
                    ("Overwrite", "F".to_string()),
                ])
            },
            &[201, 204],
        )?;
        Ok(())
    }

    fn home_dir(&self) -> Result<String, String> {
        // The logical root IS the base URL's root: for Nextcloud the URL already
        // contains /remote.php/dav/files/<user>.
        Ok("/".to_string())
    }

    fn test_connection(&self) -> Result<String, String> {
        let url = self.client.url_for_dir("/")?;
        let headers = [
            ("Depth", "0".to_string()),
            ("Content-Type", "application/xml; charset=utf-8".to_string()),
        ];
        let (status, _) = self
            .client
            .request("PROPFIND", &url, &headers, Some(PROPFIND_BODY))?;
        match status {
            207 | 200 => {
                log::info!("webdav: connected to {url}");
                // Must be the home dir: app.rs treats this Ok(String) as one.
                Ok("/".to_string())
            }
            404 | 405 => Err(format!(
                "WebDAV: cette URL ne semble pas être une racine WebDAV (HTTP {status})"
            )),
            s => Err(dav_error("PROPFIND", "/", s)),
        }
    }

    fn upload(&self, local_path: &str, remote_path: &str) -> Result<StreamHandle, String> {
        let local = std::path::PathBuf::from(local_path);
        let size = std::fs::metadata(&local)
            .map_err(|e| format!("stat {local_path}: {e}"))?
            .len();
        let client = self.client.clone();
        let remote = remote_path.to_string();
        Ok(spawn_transfer(move |tx| {
            let mut progress = ByteProgress::new(size, tx.clone());
            WebDavBackend::put_file(&client, &local, &remote, &mut progress)?;
            progress.finish();
            Ok(())
        }))
    }

    fn download(&self, remote_path: &str, local_path: &str) -> Result<StreamHandle, String> {
        let client = self.client.clone();
        let remote = remote_path.to_string();
        let local = std::path::PathBuf::from(local_path);
        Ok(spawn_transfer(move |tx| {
            let mut progress = ByteProgress::new(0, tx.clone());
            WebDavBackend::get_file(&client, &remote, &local, &mut progress, true)?;
            progress.finish();
            Ok(())
        }))
    }

    fn upload_dir(&self, local_path: &str, remote_dest: &str) -> Result<StreamHandle, String> {
        let root = std::path::PathBuf::from(local_path);
        // Walked before spawning so a bad path fails synchronously, like FTP.
        let (items, total) = walk_local(&root)?;
        let base_name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| format!("chemin local invalide: {local_path}"))?;
        let root_logical = format!("{}/{}", remote_dest.trim_end_matches('/'), base_name);

        let client = self.client.clone();
        Ok(spawn_transfer(move |tx| {
            let mut progress = ByteProgress::new(total, tx.clone());
            client.ensure_collection(&root_logical)?;
            // Pre-order: every MKCOL happens after its parent's.
            for item in &items {
                match item {
                    LocalItem::Dir(p) => {
                        let rel = rel_path(&root, p)?;
                        client.ensure_collection(&format!("{root_logical}/{rel}"))?;
                    }
                    LocalItem::File(p) => {
                        let rel = rel_path(&root, p)?;
                        let logical = format!("{root_logical}/{rel}");
                        // No compensation on failure: deleting a partial upload
                        // could destroy pre-existing server data. Naming the path
                        // is what makes a retry actionable — and a retry converges,
                        // since MKCOL 405 counts as success and PUT overwrites.
                        WebDavBackend::put_file(&client, p, &logical, &mut progress)
                            .map_err(|e| format!("{e} (interrompu à '{rel}')"))?;
                    }
                }
            }
            progress.finish();
            Ok(())
        }))
    }

    fn download_dir(&self, remote_path: &str, local_dest: &str) -> Result<StreamHandle, String> {
        let (items, total) = walk_remote(self, remote_path)?;
        let dir_name = last_segment(remote_path).to_string();
        let root_local = std::path::PathBuf::from(local_dest).join(&dir_name);
        let client = self.client.clone();
        Ok(spawn_transfer(move |tx| {
            let mut progress = ByteProgress::new(total, tx.clone());
            std::fs::create_dir_all(&root_local)
                .map_err(|e| format!("création {}: {e}", root_local.display()))?;
            for item in &items {
                let local = root_local.join(&item.rel);
                if item.is_dir {
                    std::fs::create_dir_all(&local)
                        .map_err(|e| format!("création {}: {e}", local.display()))?;
                } else {
                    WebDavBackend::get_file(&client, &item.logical, &local, &mut progress, false)
                        .map_err(|e| format!("{e} (interrompu à '{}')", item.rel))?;
                }
            }
            progress.finish();
            Ok(())
        }))
    }

    // upload_tar / download_tar keep the trait's default Err: they need server-side
    // shell execution, which WebDAV does not have.
}

fn rel_path(root: &std::path::Path, path: &std::path::Path) -> Result<String, String> {
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .map_err(|_| format!("chemin hors racine: {}", path.display()))
}

/// Collects the non-2xx statuses of a `multistatus` body (partial delete).
fn parse_multistatus_failures(xml: &str) -> Vec<String> {
    let doc = match roxmltree::Document::parse(xml) {
        Ok(d) => d,
        Err(_) => return vec![],
    };
    let mut out = Vec::new();
    for resp in doc
        .descendants()
        .filter(|n| n.has_tag_name((DAV_NS, "response")))
    {
        let status = resp
            .descendants()
            .find(|n| n.has_tag_name((DAV_NS, "status")))
            .and_then(|n| n.text())
            .unwrap_or("");
        if !status.is_empty() && !status_is_2xx(status) {
            let href = resp
                .children()
                .find(|n| n.has_tag_name((DAV_NS, "href")))
                .and_then(|n| n.text())
                .unwrap_or("?");
            out.push(format!("suppression partielle: '{href}' → {status}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NC_ROOT: &str = "/remote.php/dav/files/romain/";

    /// Nextcloud: `d:` prefix, root-relative hrefs, a second <propstat> in 404 for
    /// the properties that do not exist on a collection.
    const NEXTCLOUD_XML: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:oc="http://owncloud.org/ns">
  <d:response>
    <d:href>/remote.php/dav/files/romain/</d:href>
    <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  </d:response>
  <d:response>
    <d:href>/remote.php/dav/files/romain/Documents/</d:href>
    <d:propstat><d:prop>
        <d:resourcetype><d:collection/></d:resourcetype>
        <d:getlastmodified>Mon, 15 Jan 2024 10:30:00 GMT</d:getlastmodified>
      </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
    <d:propstat><d:prop><d:getcontentlength/></d:prop>
      <d:status>HTTP/1.1 404 Not Found</d:status></d:propstat>
  </d:response>
  <d:response>
    <d:href>/remote.php/dav/files/romain/notes.txt</d:href>
    <d:propstat><d:prop>
        <d:resourcetype/>
        <d:getcontentlength>1234</d:getcontentlength>
        <d:getlastmodified>Tue, 16 Jan 2024 08:05:00 GMT</d:getlastmodified>
      </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  </d:response>
</d:multistatus>"#;

    /// Apache mod_dav: `D:` prefix, and a bogus content length ON the collection.
    const APACHE_XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/dav/</D:href>
    <D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype>
      <D:getcontentlength>4096</D:getcontentlength></D:prop>
      <D:status>HTTP/1.1 200 OK</D:status></D:propstat>
  </D:response>
  <D:response>
    <D:href>/dav/sub/</D:href>
    <D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype>
      <D:getcontentlength>4096</D:getcontentlength></D:prop>
      <D:status>HTTP/1.1 200 OK</D:status></D:propstat>
  </D:response>
  <D:response>
    <D:href>/dav/file.bin</D:href>
    <D:propstat><D:prop><D:resourcetype/>
      <D:getcontentlength>42</D:getcontentlength></D:prop>
      <D:status>HTTP/1.1 200 OK</D:status></D:propstat>
  </D:response>
</D:multistatus>"#;

    fn names(entries: &[FileEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    // --- parse_propfind ---

    #[test]
    fn parses_nextcloud_and_ignores_the_404_propstat() {
        let entries = parse_propfind(NEXTCLOUD_XML, NC_ROOT).unwrap();
        // The "self" entry (the requested collection) is excluded.
        assert_eq!(names(&entries), vec!["Documents", "notes.txt"]);

        let docs = &entries[0];
        assert!(docs.is_dir);
        assert_eq!(docs.size, 0);
        assert_eq!(docs.modified, "2024-01-15 10:30");

        let notes = &entries[1];
        assert!(!notes.is_dir);
        assert_eq!(notes.size, 1234);
        assert_eq!(notes.modified, "2024-01-16 08:05");
    }

    #[test]
    fn parses_apache_and_zeroes_collection_size() {
        let entries = parse_propfind(APACHE_XML, "/dav/").unwrap();
        assert_eq!(names(&entries), vec!["sub", "file.bin"]);
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].size, 0, "a collection must not report a size");
        assert_eq!(entries[1].size, 42);
        // No getlastmodified at all -> absent, not invented.
        assert_eq!(entries[0].modified, "");
    }

    #[test]
    fn parsing_is_independent_of_the_namespace_prefix() {
        let xml = r#"<?xml version="1.0"?>
<ns0:multistatus xmlns:ns0="DAV:">
  <ns0:response><ns0:href>/dav/</ns0:href>
    <ns0:propstat><ns0:prop><ns0:resourcetype><ns0:collection/></ns0:resourcetype></ns0:prop>
      <ns0:status>HTTP/1.1 200 OK</ns0:status></ns0:propstat></ns0:response>
  <ns0:response><ns0:href>/dav/x.txt</ns0:href>
    <ns0:propstat><ns0:prop><ns0:resourcetype/><ns0:getcontentlength>7</ns0:getcontentlength></ns0:prop>
      <ns0:status>HTTP/1.1 200 OK</ns0:status></ns0:propstat></ns0:response>
</ns0:multistatus>"#;
        let entries = parse_propfind(xml, "/dav/").unwrap();
        assert_eq!(names(&entries), vec!["x.txt"]);
        assert_eq!(entries[0].size, 7);
    }

    #[test]
    fn webdav_entries_have_no_fabricated_permissions() {
        for e in parse_propfind(NEXTCLOUD_XML, NC_ROOT).unwrap() {
            assert_eq!(e.permissions, "", "WebDAV has no POSIX bits to report");
        }
    }

    #[test]
    fn decodes_percent_encoded_names() {
        let xml = format!(
            r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
  <d:response><d:href>{root}</d:href>
    <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
  <d:response><d:href>{root}Docs%20%26%20co/</d:href>
    <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
  <d:response><d:href>{root}caf%C3%A9.txt</d:href>
    <d:propstat><d:prop><d:resourcetype/></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
  <d:response><d:href>{root}%23tag.md</d:href>
    <d:propstat><d:prop><d:resourcetype/></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
  <d:response><d:href>{root}a+b.txt</d:href>
    <d:propstat><d:prop><d:resourcetype/></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#,
            root = NC_ROOT
        );
        let entries = parse_propfind(&xml, NC_ROOT).unwrap();
        let n = names(&entries);
        assert!(n.contains(&"Docs & co"), "{n:?}");
        assert!(n.contains(&"café.txt"), "{n:?}");
        assert!(n.contains(&"#tag.md"), "{n:?}");
        // Regression guard: '+' must NOT become a space (form-urlencoded trap).
        assert!(n.contains(&"a+b.txt"), "{n:?}");
    }

    #[test]
    fn empty_multistatus_is_an_empty_listing() {
        let xml = r#"<d:multistatus xmlns:d="DAV:"></d:multistatus>"#;
        assert!(parse_propfind(xml, "/dav/").unwrap().is_empty());
    }

    #[test]
    fn malformed_xml_is_an_error() {
        assert!(parse_propfind("<not-closed", "/dav/").is_err());
    }

    #[test]
    fn body_without_multistatus_is_an_error() {
        assert!(parse_propfind("<html><body>404</body></html>", "/dav/").is_err());
    }

    // --- href_to_name ---

    #[test]
    fn absolute_and_relative_hrefs_yield_the_same_name() {
        let rel = href_to_name(NC_ROOT, "/remote.php/dav/files/romain/a.txt");
        let abs = href_to_name(
            NC_ROOT,
            "https://cloud.exemple.fr/remote.php/dav/files/romain/a.txt",
        );
        assert_eq!(rel.as_deref(), Some("a.txt"));
        assert_eq!(abs, rel);
    }

    #[test]
    fn self_entry_is_skipped_with_or_without_trailing_slash() {
        assert_eq!(href_to_name(NC_ROOT, NC_ROOT), None);
        assert_eq!(href_to_name(NC_ROOT, NC_ROOT.trim_end_matches('/')), None);
    }

    #[test]
    fn collection_href_yields_a_name_without_slash() {
        assert_eq!(
            href_to_name(NC_ROOT, "/remote.php/dav/files/romain/Docs/").as_deref(),
            Some("Docs")
        );
    }

    /// An unexpected prefix (rewriting proxy, /index.php/ inserted, case change)
    /// must NOT make the entry vanish: a listing that lies by omission is worse
    /// than one that errors, because nobody challenges it.
    #[test]
    fn unexpected_prefix_keeps_the_entry() {
        assert_eq!(
            href_to_name(NC_ROOT, "/index.php/remote.php/dav/files/romain/a.txt").as_deref(),
            Some("a.txt")
        );
        assert_eq!(
            href_to_name(NC_ROOT, "/totally/other/place/b.txt").as_deref(),
            Some("b.txt")
        );
    }

    #[test]
    fn href_query_and_fragment_are_ignored() {
        assert_eq!(
            href_to_name(NC_ROOT, "/remote.php/dav/files/romain/a.txt?v=2").as_deref(),
            Some("a.txt")
        );
    }

    // --- url_for ---

    fn base() -> url::Url {
        url::Url::parse("https://cloud.exemple.fr/remote.php/dav/files/romain/").unwrap()
    }

    #[test]
    fn url_for_root_is_the_base() {
        assert_eq!(
            url_for(&base(), "/", true).unwrap(),
            "https://cloud.exemple.fr/remote.php/dav/files/romain/"
        );
    }

    #[test]
    fn url_for_encodes_special_characters() {
        let u = url_for(&base(), "/a b/caf\u{e9} #1.txt", false).unwrap();
        assert!(u.ends_with("/a%20b/caf%C3%A9%20%231.txt"), "{u}");
    }

    #[test]
    fn url_for_collection_appends_a_slash() {
        let u = url_for(&base(), "/Docs", true).unwrap();
        assert!(u.ends_with("/Docs/"), "{u}");
        let f = url_for(&base(), "/Docs", false).unwrap();
        assert!(f.ends_with("/Docs"), "{f}");
        assert!(!f.ends_with("/Docs/"), "{f}");
    }

    #[test]
    fn url_for_normalizes_empty_and_dot_segments() {
        let a = url_for(&base(), "//a//b/", false).unwrap();
        let b = url_for(&base(), "/a/./b", false).unwrap();
        assert_eq!(a, b);
        assert!(a.ends_with("/a/b"), "{a}");
    }

    #[test]
    fn url_for_drops_parent_traversal() {
        // The UI resolves ".." before calling; never escape the DAV root.
        let u = url_for(&base(), "/a/../b", false).unwrap();
        assert!(u.ends_with("/a/b"), "{u}");
    }

    // --- format_http_date ---

    #[test]
    fn formats_rfc1123() {
        assert_eq!(
            format_http_date("Mon, 15 Jan 2024 10:30:00 GMT"),
            "2024-01-15 10:30"
        );
        assert_eq!(
            format_http_date("Mon, 15 Jan 2024 10:30:00 -0000"),
            "2024-01-15 10:30"
        );
        assert_eq!(
            format_http_date("Mon, 15 Jan 2024 10:30:00 +0000"),
            "2024-01-15 10:30"
        );
    }

    /// chrono refuses a weekday that contradicts the date; a server that gets it
    /// wrong must not blank the whole date column.
    #[test]
    fn tolerates_a_wrong_weekday() {
        // 15 Jan 2024 was a Monday, not a Tuesday.
        assert_eq!(
            format_http_date("Tue, 15 Jan 2024 10:30:00 GMT"),
            "2024-01-15 10:30"
        );
    }

    #[test]
    fn formats_iso8601_as_a_fallback() {
        assert_eq!(format_http_date("2024-01-15T10:30:00Z"), "2024-01-15 10:30");
    }

    #[test]
    fn unparseable_date_is_empty_not_invented() {
        assert_eq!(format_http_date(""), "");
        assert_eq!(format_http_date("yesterday-ish"), "");
    }

    #[test]
    fn the_date_format_sorts_lexicographically() {
        // What the `s` date sort relies on, since `modified` is a String.
        let a = format_http_date("Mon, 15 Jan 2024 10:30:00 GMT");
        let b = format_http_date("Tue, 16 Jan 2024 08:05:00 GMT");
        assert!(a < b, "{a} !< {b}");
    }

    // --- status / errors ---

    #[test]
    fn status_2xx_detection() {
        assert!(status_is_2xx("HTTP/1.1 200 OK"));
        assert!(status_is_2xx("HTTP/1.1 207 Multi-Status"));
        assert!(!status_is_2xx("HTTP/1.1 404 Not Found"));
        assert!(!status_is_2xx(""));
        assert!(!status_is_2xx("garbage"));
    }

    #[test]
    fn dav_error_messages_carry_the_useful_hint() {
        for (status, needle) in [
            (301u16, "slash"),
            (401, "authentification"),
            (403, "interdit"),
            (404, "introuvable"),
            (405, "existe déjà"),
            (409, "parent"),
            (412, "existe déjà"),
            (423, "verrouillée"),
            (503, "indisponible"),
            (507, "quota"),
        ] {
            let msg = dav_error("DELETE", "/x", status);
            assert!(
                msg.to_lowercase().contains(needle),
                "status {status} -> {msg} (expected {needle:?})"
            );
        }
    }

    #[test]
    fn dav_error_names_the_operation_and_path() {
        let msg = dav_error("MOVE", "/a/b.txt", 412);
        assert!(msg.contains("MOVE"), "{msg}");
        assert!(msg.contains("/a/b.txt"), "{msg}");
    }

    // --- unauthorized_error ---

    #[test]
    fn digest_only_server_is_named_as_such() {
        // The real challenge from a SabreDAV host (BigCommerce).
        let msg = unauthorized_error(
            "PROPFIND",
            "/dav/",
            Some(r#"Digest realm="SabreDAV",qop="auth",nonce="6a86ecab",opaque="df58bd""#),
        );
        assert!(msg.contains("Digest"), "{msg}");
        assert!(
            !msg.contains("vérifiez identifiant"),
            "must not blame the credentials: {msg}"
        );
    }

    #[test]
    fn basic_server_blames_the_credentials() {
        let msg = unauthorized_error("PROPFIND", "/dav/", Some(r#"Basic realm="dav""#));
        assert!(msg.contains("vérifiez identifiant"), "{msg}");
        assert!(!msg.contains("Digest"), "{msg}");
    }

    #[test]
    fn a_server_offering_both_still_blames_the_credentials_first() {
        let msg = unauthorized_error(
            "PROPFIND",
            "/dav/",
            Some(r#"Basic realm="x", Digest realm="x""#),
        );
        assert!(msg.contains("vérifiez identifiant"), "{msg}");
        assert!(msg.contains("Digest"), "should still mention it: {msg}");
    }

    #[test]
    fn missing_challenge_falls_back_to_the_generic_message() {
        let msg = unauthorized_error("PROPFIND", "/dav/", None);
        assert!(msg.contains("401"), "{msg}");
        assert!(msg.contains("vérifiez identifiant"), "{msg}");
    }

    // --- is_tls_untrusted ---

    #[test]
    fn recognizes_tls_trust_failures() {
        assert!(is_tls_untrusted("invalid peer certificate: UnknownIssuer"));
        assert!(is_tls_untrusted("certificate verify failed"));
        assert!(is_tls_untrusted("self signed certificate in chain"));
        assert!(is_tls_untrusted("self-signed cert"));
        // Verified against a real self-signed endpoint: rustls says neither
        // "UnknownIssuer" nor "self-signed" here, which is why the broad
        // "certificate" catch-all has to stay.
        assert!(is_tls_untrusted(
            "WebDAV PROPFIND https://localhost:18443/: io: invalid peer certificate: Other(OtherError(CaUsedAsEndEntity))"
        ));
    }

    #[test]
    fn does_not_mistake_other_failures_for_tls() {
        assert!(!is_tls_untrusted(
            "WebDAV PROPFIND '/' → HTTP 401: authentification refusée"
        ));
        assert!(!is_tls_untrusted("connection refused"));
        assert!(!is_tls_untrusted("host not found"));
        assert!(!is_tls_untrusted(""));
    }

    // --- multistatus failures ---

    #[test]
    fn detects_a_partial_delete() {
        let xml = r#"<d:multistatus xmlns:d="DAV:">
  <d:response><d:href>/dav/keep.txt</d:href><d:status>HTTP/1.1 423 Locked</d:status></d:response>
</d:multistatus>"#;
        let f = parse_multistatus_failures(xml);
        assert_eq!(f.len(), 1);
        assert!(f[0].contains("keep.txt"), "{:?}", f);
    }

    #[test]
    fn a_fully_successful_multistatus_has_no_failures() {
        let xml = r#"<d:multistatus xmlns:d="DAV:">
  <d:response><d:href>/dav/a</d:href><d:status>HTTP/1.1 200 OK</d:status></d:response>
</d:multistatus>"#;
        assert!(parse_multistatus_failures(xml).is_empty());
    }

    // --- walk_local (the MKCOL ordering invariant) ---

    #[test]
    fn walk_local_is_pre_order_and_sums_sizes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("top.txt"), b"12345").unwrap();
        std::fs::write(root.join("a/mid.txt"), b"123").unwrap();
        std::fs::write(root.join("a/b/deep.txt"), b"1").unwrap();

        let (items, total) = walk_local(root).unwrap();
        assert_eq!(total, 9);

        let paths: Vec<String> = items
            .iter()
            .map(|i| match i {
                LocalItem::Dir(p) => rel_path(root, p).unwrap(),
                LocalItem::File(p) => rel_path(root, p).unwrap(),
            })
            .collect();

        // Pre-order is a functional guarantee: MKCOL answers 409 if an ancestor is
        // missing, and WebDAV has no `mkdir -p`.
        let idx = |needle: &str| paths.iter().position(|p| p == needle).expect(needle);
        assert!(idx("a") < idx("a/b"), "{paths:?}");
        assert!(idx("a") < idx("a/mid.txt"), "{paths:?}");
        assert!(idx("a/b") < idx("a/b/deep.txt"), "{paths:?}");
    }

    #[test]
    fn walk_local_reports_a_missing_directory() {
        assert!(walk_local(std::path::Path::new("/nonexistent-lazy-transfer")).is_err());
    }

    #[test]
    fn last_segment_handles_trailing_slashes() {
        assert_eq!(last_segment("/a/b/"), "b");
        assert_eq!(last_segment("/a/b"), "b");
        assert_eq!(last_segment("/"), "");
    }
}
