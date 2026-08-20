/// Protocol type for remote connections.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Protocol {
    #[default]
    Ssh,
    Sftp,
    Ftp,
    WebDav,
}

impl Protocol {
    pub fn default_port(&self) -> u16 {
        match self {
            Protocol::Ssh | Protocol::Sftp => 22,
            Protocol::Ftp => 21,
            // Informative only: the real port always comes from the URL.
            Protocol::WebDav => 443,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Protocol::Ssh => "SSH",
            Protocol::Sftp => "SFTP",
            Protocol::Ftp => "FTP",
            Protocol::WebDav => "WebDAV",
        }
    }

    /// Single source of truth for the on-disk (connections.json) and CLI spelling.
    pub fn as_str(&self) -> &'static str {
        match self {
            Protocol::Ssh => "ssh",
            Protocol::Sftp => "sftp",
            Protocol::Ftp => "ftp",
            Protocol::WebDav => "webdav",
        }
    }

    /// Inverse of `as_str`, case-insensitive. `None` for anything unknown so that
    /// callers stay in control of the fallback (tolerant DTO, strict CLI, ...).
    pub fn from_str_opt(s: &str) -> Option<Protocol> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ssh" | "scp" => Some(Protocol::Ssh),
            "sftp" => Some(Protocol::Sftp),
            "ftp" => Some(Protocol::Ftp),
            "webdav" | "dav" => Some(Protocol::WebDav),
            _ => None,
        }
    }

    /// True when a connection is addressed by URL rather than by host + port.
    pub fn uses_url(&self) -> bool {
        matches!(self, Protocol::WebDav)
    }
}

/// Authentication method for connections.
#[derive(Debug, Clone)]
pub enum AuthMethod {
    /// Path to an identity file (SSH key).
    Key(String),
    /// Password-based authentication.
    Password,
    /// Use the SSH agent.
    Agent,
}

/// Credentials for a WebDAV endpoint. Deliberately NOT folded into `AuthMethod`:
/// SSH/SFTP/FTP have no bearer-token notion and must not have to reject one.
#[derive(Clone, PartialEq, Eq)] // NOT Debug — see the manual impl below.
pub enum WebDavAuth {
    Basic {
        user: String,
        password: String,
    },
    /// HTTP Digest (RFC 7616). Required by SabreDAV-based hosts (BigCommerce among
    /// them), which advertise no other scheme. Unlike the others it cannot be
    /// pre-computed into a header: it needs the server's challenge first.
    Digest {
        user: String,
        password: String,
    },
    Bearer(String),
    Anonymous,
}

/// Debug is implemented by hand to redact the secret. The repo already logs
/// `auth={:?}` (sftp_backend.rs) — harmless today because `AuthMethod::Password`
/// carries nothing. A derived Debug here would leak the token into debug.log the
/// day someone copies that line into `connect_webdav`.
impl std::fmt::Debug for WebDavAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebDavAuth::Basic { user, .. } => f
                .debug_struct("Basic")
                .field("user", user)
                .field("password", &"***")
                .finish(),
            WebDavAuth::Digest { user, .. } => f
                .debug_struct("Digest")
                .field("user", user)
                .field("password", &"***")
                .finish(),
            WebDavAuth::Bearer(_) => f.debug_tuple("Bearer").field(&"***").finish(),
            WebDavAuth::Anonymous => f.write_str("Anonymous"),
        }
    }
}

impl WebDavAuth {
    /// Ready-to-use HTTP header value. Keeps base64 and the secret inside the
    /// domain: the backend only ever holds an already-computed header.
    pub fn header_value(&self) -> Option<String> {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        match self {
            WebDavAuth::Basic { user, password } => Some(format!(
                "Basic {}",
                STANDARD.encode(format!("{user}:{password}"))
            )),
            WebDavAuth::Bearer(token) => Some(format!("Bearer {token}")),
            // Digest is challenge-response: there is nothing to send up front.
            WebDavAuth::Digest { .. } | WebDavAuth::Anonymous => None,
        }
    }

    /// Credentials for the Digest exchange, when that is the chosen scheme.
    pub fn digest_credentials(&self) -> Option<(&str, &str)> {
        match self {
            WebDavAuth::Digest { user, password } => Some((user, password)),
            _ => None,
        }
    }

    /// Stable token persisted in connections.json (`auth_method`).
    pub fn as_str(&self) -> &'static str {
        match self {
            WebDavAuth::Basic { .. } => "basic",
            WebDavAuth::Digest { .. } => "digest",
            WebDavAuth::Bearer(_) => "bearer",
            WebDavAuth::Anonymous => "anonymous",
        }
    }

    /// Password or bearer token — whatever must be persisted (base64) or redacted.
    pub fn secret(&self) -> Option<&str> {
        match self {
            WebDavAuth::Basic { password, .. } | WebDavAuth::Digest { password, .. } => {
                Some(password)
            }
            WebDavAuth::Bearer(token) => Some(token),
            WebDavAuth::Anonymous => None,
        }
    }

    pub fn user(&self) -> Option<&str> {
        match self {
            WebDavAuth::Basic { user, .. } | WebDavAuth::Digest { user, .. } => Some(user),
            WebDavAuth::Bearer(_) | WebDavAuth::Anonymous => None,
        }
    }
}

/// Everything WebDAV-specific about a connection. `Some` iff protocol == WebDav.
#[derive(Debug, Clone)]
pub struct WebDavConfig {
    /// The NORMALIZED collection URL (never the raw user input): explicit scheme,
    /// no query/fragment/userinfo, trailing '/'. On the manual path this is
    /// guaranteed by `parse_webdav_url`; on the restore path (`webdav_saved`) it is
    /// trusted as already-normalized and only re-validated at connect time —
    /// connections.json is hand-editable, so the guarantee is one-sided there and
    /// the backend is the backstop.
    pub url: String,
    pub auth: WebDavAuth,
    /// Per-connection opt-in: accept invalid/self-signed TLS certificates.
    pub insecure_tls: bool,
}

/// Result of parsing a WebDAV URL. Single source of truth: yields BOTH the
/// normalized URL used by the backend and the fields derived for display and
/// persistence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebDavUrl {
    pub normalized: String,
    pub scheme: String,
    pub host: String,
    pub port: u16,
    /// Always starts and ends with '/'.
    pub base_path: String,
    /// Username found in the URL's userinfo, if any. The password half is
    /// deliberately DISCARDED: a secret must be typed, not pasted in an URL.
    pub user: Option<String>,
}

/// The one and only WebDAV URL parser.
pub fn parse_webdav_url(raw: &str) -> Result<WebDavUrl, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("URL vide".to_string());
    }
    // `dav://` and `davs://` are the file-manager spellings (GVFS, Dolphin,
    // Cyberduck, and BigCommerce's docs). They mean plain HTTP and HTTPS.
    let trimmed = match trimmed.split_once("://") {
        Some(("davs", rest)) => format!("https://{rest}"),
        Some(("dav", rest)) => format!("http://{rest}"),
        _ => trimmed.to_string(),
    };
    // A bare host is the common paste; assume TLS rather than silently downgrading.
    let with_scheme = if trimmed.contains("://") {
        trimmed.clone()
    } else {
        format!("https://{trimmed}")
    };

    let mut url = url::Url::parse(&with_scheme).map_err(|e| format!("URL invalide: {e}"))?;

    let scheme = url.scheme().to_string();
    if scheme != "http" && scheme != "https" {
        return Err(format!(
            "schéma '{scheme}' non supporté: utilisez http(s):// ou dav(s)://"
        ));
    }
    if url.host_str().unwrap_or_default().is_empty() {
        return Err("URL sans hôte".to_string());
    }

    // The username is kept for pre-filling; the password half is dropped on purpose.
    let user = match url.username() {
        "" => None,
        u => Some(
            percent_encoding::percent_decode_str(u)
                .decode_utf8_lossy()
                .to_string(),
        ),
    };

    url.set_query(None);
    url.set_fragment(None);
    let _ = url.set_username("");
    let _ = url.set_password(None);

    // Mandatory trailing '/': without it `Url::join` and `path_segments_mut` would
    // replace the last segment instead of appending to it.
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }

    let host = url.host_str().unwrap_or_default().to_string();
    let port = url.port_or_known_default().unwrap_or(443);
    let base_path = url.path().to_string();

    Ok(WebDavUrl {
        normalized: url.to_string(),
        scheme,
        host,
        port,
        base_path,
        user,
    })
}

/// The one place that renders "who is this connection". Both the saved-connection
/// list and `ConnectionConfig::label` go through it, so the "empty user" branch
/// lives in exactly one spot.
pub fn display_identity(
    protocol: &Protocol,
    user: &str,
    host: &str,
    port: u16,
    url: Option<&str>,
) -> String {
    if protocol.uses_url() {
        // Fall back rather than panic: connections.json is hand-editable and may
        // carry protocol "webdav" with no url at all.
        if let Some(u) = url.filter(|u| !u.is_empty()) {
            return u.to_string();
        }
        return format!("{host}:{port}");
    }
    if user.is_empty() {
        format!("{host}:{port}")
    } else {
        format!("{user}@{host}:{port}")
    }
}

/// A parsed SSH config host entry.
#[derive(Debug, Clone, Default)]
pub struct SshHost {
    pub alias: String,
    pub hostname: String,
    pub user: String,
    pub port: u16,
    pub identity_file: String,
}

/// Connection configuration (either from ssh config or manual).
///
/// Fields are private on purpose: it makes the illegal state `webdav: Some(..)`
/// with `protocol: Ssh` unrepresentable, instead of documenting the invariant in a
/// comment nobody enforces. Build one through a named constructor.
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    protocol: Protocol,
    host: String,
    user: String,
    port: u16,
    auth: AuthMethod,
    label: String,
    /// When set, connect via this ssh_config alias and let `ssh` resolve
    /// Hostname/User/Port/IdentityFile/ProxyJump/etc. from ~/.ssh/config.
    /// This is required for wildcard Host blocks (e.g. `Host julbo.*`) to
    /// merge their options correctly.
    ssh_alias: Option<String>,
    /// `Some` iff protocol == Protocol::WebDav. When set, `auth` is not meaningful.
    webdav: Option<WebDavConfig>,
}

impl ConnectionConfig {
    pub fn ssh(
        host: String,
        user: String,
        port: u16,
        auth: AuthMethod,
        ssh_alias: Option<String>,
    ) -> Self {
        let label = display_identity(&Protocol::Ssh, &user, &host, port, None);
        ConnectionConfig {
            protocol: Protocol::Ssh,
            host,
            user,
            port,
            auth,
            label,
            ssh_alias,
            webdav: None,
        }
    }

    pub fn sftp(host: String, user: String, port: u16, auth: AuthMethod) -> Self {
        let label = display_identity(&Protocol::Sftp, &user, &host, port, None);
        ConnectionConfig {
            protocol: Protocol::Sftp,
            host,
            user,
            port,
            auth,
            label,
            ssh_alias: None,
            webdav: None,
        }
    }

    pub fn ftp(host: String, user: String, port: u16) -> Self {
        let label = display_identity(&Protocol::Ftp, &user, &host, port, None);
        ConnectionConfig {
            protocol: Protocol::Ftp,
            host,
            user,
            port,
            auth: AuthMethod::Password,
            label,
            ssh_alias: None,
            webdav: None,
        }
    }

    /// Manual / first-time WebDAV connection: host, user and port are derived from
    /// the parsed URL so that the status bar and persistence keep working unchanged.
    pub fn webdav(parsed: &WebDavUrl, auth: WebDavAuth, insecure_tls: bool) -> Self {
        let user = auth
            .user()
            .map(str::to_string)
            .or_else(|| parsed.user.clone())
            .unwrap_or_default();
        ConnectionConfig {
            protocol: Protocol::WebDav,
            host: parsed.host.clone(),
            user,
            port: parsed.port,
            auth: AuthMethod::Password,
            label: display_identity(
                &Protocol::WebDav,
                "",
                &parsed.host,
                parsed.port,
                Some(&parsed.normalized),
            ),
            ssh_alias: None,
            webdav: Some(WebDavConfig {
                url: parsed.normalized.clone(),
                auth,
                insecure_tls,
            }),
        }
    }

    /// Restored from connections.json: the fields are ALREADY derived and persisted.
    /// Does NOT parse — the loading path must never be able to fail. An invalid URL
    /// is reported by the backend at connect time.
    pub fn webdav_saved(
        url: String,
        host: String,
        user: String,
        port: u16,
        auth: WebDavAuth,
        insecure_tls: bool,
    ) -> Self {
        let label = display_identity(&Protocol::WebDav, &user, &host, port, Some(&url));
        ConnectionConfig {
            protocol: Protocol::WebDav,
            host,
            user,
            port,
            auth: AuthMethod::Password,
            label,
            ssh_alias: None,
            webdav: Some(WebDavConfig {
                url,
                auth,
                insecure_tls,
            }),
        }
    }

    pub fn from_ssh_host(host: &SshHost) -> Self {
        let auth = if host.identity_file.is_empty() {
            AuthMethod::Agent
        } else {
            AuthMethod::Key(host.identity_file.clone())
        };
        let mut conn = ConnectionConfig::ssh(
            host.hostname.clone(),
            host.user.clone(),
            host.port,
            auth,
            Some(host.alias.clone()),
        );
        // The ssh_config label historically shows hostname:port, not the alias.
        conn.label = display_identity(&Protocol::Ssh, &host.user, &host.hostname, host.port, None);
        conn
    }

    // --- Accessors ---

    pub fn protocol(&self) -> &Protocol {
        &self.protocol
    }
    pub fn host(&self) -> &str {
        &self.host
    }
    pub fn user(&self) -> &str {
        &self.user
    }
    pub fn port(&self) -> u16 {
        self.port
    }
    pub fn auth(&self) -> &AuthMethod {
        &self.auth
    }
    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn ssh_alias(&self) -> Option<&str> {
        self.ssh_alias.as_deref()
    }
    pub fn webdav_config(&self) -> Option<&WebDavConfig> {
        self.webdav.as_ref()
    }

    /// Same connection, but accepting invalid TLS certificates. `None` unless this
    /// is a WebDAV connection that is not already insecure — which is exactly the
    /// guard that keeps the retry dialog from looping.
    pub fn webdav_insecure_retry(&self) -> Option<Self> {
        let cfg = self.webdav.as_ref()?;
        if cfg.insecure_tls {
            return None;
        }
        let mut next = self.clone();
        if let Some(w) = next.webdav.as_mut() {
            w.insecure_tls = true;
        }
        Some(next)
    }
}

/// Column to sort file entries by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    Size,
    Date,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Asc,
    Desc,
}

impl SortColumn {
    pub fn next(self) -> Self {
        match self {
            SortColumn::Name => SortColumn::Size,
            SortColumn::Size => SortColumn::Date,
            SortColumn::Date => SortColumn::Name,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortColumn::Name => "name",
            SortColumn::Size => "size",
            SortColumn::Date => "date",
        }
    }
}

impl SortOrder {
    pub fn toggle(self) -> Self {
        match self {
            SortOrder::Asc => SortOrder::Desc,
            SortOrder::Desc => SortOrder::Asc,
        }
    }

    pub fn arrow(self) -> &'static str {
        match self {
            SortOrder::Asc => "↑",
            SortOrder::Desc => "↓",
        }
    }
}

/// A single file/directory entry (used for both local and remote listings).
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: String,
    pub permissions: String,
}

/// Represents the state of an ongoing transfer.
#[derive(Debug, Clone)]
pub enum TransferStatus {
    Queued,
    InProgress { percent: u8, speed: String },
    Completed,
    Failed(String),
}

/// Direction of a file transfer.
#[derive(Debug, Clone)]
pub enum TransferDirection {
    Upload,
    Download,
}

/// A transfer job in the queue.
#[derive(Debug, Clone)]
pub struct TransferJob {
    pub id: usize,
    pub source: String,
    pub destination: String,
    pub direction: TransferDirection,
    pub file_name: String,
    pub file_size: u64,
    pub status: TransferStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Protocol ---

    #[test]
    fn protocol_str_roundtrip() {
        for p in [
            Protocol::Ssh,
            Protocol::Sftp,
            Protocol::Ftp,
            Protocol::WebDav,
        ] {
            assert_eq!(Protocol::from_str_opt(p.as_str()), Some(p.clone()));
        }
        assert_eq!(Protocol::from_str_opt("WEBDAV"), Some(Protocol::WebDav));
        assert_eq!(Protocol::from_str_opt("  dav "), Some(Protocol::WebDav));
        assert_eq!(Protocol::from_str_opt("nope"), None);
    }

    #[test]
    fn webdav_default_port_and_label() {
        assert_eq!(Protocol::WebDav.default_port(), 443);
        assert_eq!(Protocol::WebDav.label(), "WebDAV");
        assert!(Protocol::WebDav.uses_url());
        assert!(!Protocol::Ssh.uses_url());
    }

    // --- WebDavAuth ---

    #[test]
    fn webdav_auth_as_str_secret_and_user() {
        let basic = WebDavAuth::Basic {
            user: "alice".to_string(),
            password: "s3cret".to_string(),
        };
        assert_eq!(basic.as_str(), "basic");
        assert_eq!(basic.secret(), Some("s3cret"));
        assert_eq!(basic.user(), Some("alice"));

        let bearer = WebDavAuth::Bearer("tok".to_string());
        assert_eq!(bearer.as_str(), "bearer");
        assert_eq!(bearer.secret(), Some("tok"));
        assert_eq!(bearer.user(), None);

        assert_eq!(WebDavAuth::Anonymous.as_str(), "anonymous");
        assert_eq!(WebDavAuth::Anonymous.secret(), None);
        assert_eq!(WebDavAuth::Anonymous.user(), None);
    }

    #[test]
    fn webdav_auth_header_value() {
        let basic = WebDavAuth::Basic {
            user: "user".to_string(),
            password: "pw".to_string(),
        };
        // base64("user:pw") == "dXNlcjpwdw=="
        assert_eq!(basic.header_value().unwrap(), "Basic dXNlcjpwdw==");
        assert_eq!(
            WebDavAuth::Bearer("xyz".to_string())
                .header_value()
                .unwrap(),
            "Bearer xyz"
        );
        assert_eq!(WebDavAuth::Anonymous.header_value(), None);
    }

    #[test]
    fn webdav_auth_debug_redacts_the_secret() {
        let bearer = format!("{:?}", WebDavAuth::Bearer("tok-do-not-log".to_string()));
        assert!(!bearer.contains("tok-do-not-log"), "leaked: {bearer}");
        assert!(bearer.contains("***"));

        let basic = format!(
            "{:?}",
            WebDavAuth::Basic {
                user: "alice".to_string(),
                password: "pw-do-not-log".to_string(),
            }
        );
        assert!(!basic.contains("pw-do-not-log"), "leaked: {basic}");
        assert!(basic.contains("alice"));
    }

    // --- parse_webdav_url ---

    #[test]
    fn parse_webdav_url_full_nextcloud() {
        let u = parse_webdav_url("https://cloud.exemple.fr/remote.php/dav/files/romain/").unwrap();
        assert_eq!(u.scheme, "https");
        assert_eq!(u.host, "cloud.exemple.fr");
        assert_eq!(u.port, 443);
        assert_eq!(u.base_path, "/remote.php/dav/files/romain/");
        assert_eq!(u.user, None);
        assert_eq!(
            u.normalized,
            "https://cloud.exemple.fr/remote.php/dav/files/romain/"
        );
    }

    #[test]
    fn parse_webdav_url_adds_trailing_slash() {
        let u = parse_webdav_url("https://host/dav").unwrap();
        assert_eq!(u.base_path, "/dav/");
        assert!(u.normalized.ends_with("/dav/"));
    }

    #[test]
    fn parse_webdav_url_explicit_port() {
        let u = parse_webdav_url("https://nas.local:8443/dav/").unwrap();
        assert_eq!(u.port, 8443);
        assert!(u.normalized.contains(":8443"));
    }

    #[test]
    fn parse_webdav_url_http_defaults_to_80() {
        let u = parse_webdav_url("http://localhost/").unwrap();
        assert_eq!(u.scheme, "http");
        assert_eq!(u.port, 80);
    }

    #[test]
    fn parse_webdav_url_without_scheme_defaults_to_https() {
        let u = parse_webdav_url("cloud.exemple.fr/dav").unwrap();
        assert_eq!(u.scheme, "https");
        assert_eq!(u.port, 443);
        assert_eq!(u.host, "cloud.exemple.fr");
    }

    #[test]
    fn parse_webdav_url_strips_query_and_fragment() {
        let u = parse_webdav_url("https://host/dav/?a=1#frag").unwrap();
        assert!(!u.normalized.contains('?'), "{}", u.normalized);
        assert!(!u.normalized.contains('#'), "{}", u.normalized);
    }

    #[test]
    fn parse_webdav_url_keeps_user_and_drops_password() {
        let u = parse_webdav_url("https://alice:hunter2@host/dav/").unwrap();
        assert_eq!(u.user.as_deref(), Some("alice"));
        assert!(
            !u.normalized.contains("hunter2"),
            "password leaked into {}",
            u.normalized
        );
        assert!(!u.normalized.contains("alice"), "{}", u.normalized);
    }

    #[test]
    fn parse_webdav_url_accepts_the_file_manager_schemes() {
        // What you get when copying out of GVFS/Dolphin/Cyberduck, or out of the
        // BigCommerce control panel.
        let secure = parse_webdav_url("davs://store-abc.example.com/dav/").unwrap();
        assert_eq!(secure.scheme, "https");
        assert_eq!(secure.port, 443);
        assert_eq!(secure.host, "store-abc.example.com");
        assert_eq!(secure.normalized, "https://store-abc.example.com/dav/");

        let plain = parse_webdav_url("dav://nas.local/share").unwrap();
        assert_eq!(plain.scheme, "http");
        assert_eq!(plain.port, 80);
        assert_eq!(plain.normalized, "http://nas.local/share/");
    }

    #[test]
    fn parse_webdav_url_rejects() {
        assert!(parse_webdav_url("").is_err());
        assert!(parse_webdav_url("   ").is_err());
        assert!(parse_webdav_url("ftp://host/pub").is_err());
        assert!(parse_webdav_url("sftp://host").is_err());
        assert!(parse_webdav_url("https://").is_err());
    }

    // --- display_identity ---

    #[test]
    fn display_identity_uses_url_for_webdav() {
        assert_eq!(
            display_identity(
                &Protocol::WebDav,
                "alice",
                "cloud",
                443,
                Some("https://cloud/dav/alice/")
            ),
            "https://cloud/dav/alice/"
        );
    }

    #[test]
    fn display_identity_webdav_without_url_falls_back() {
        // Hand-edited connections.json: protocol "webdav" but no url at all.
        assert_eq!(
            display_identity(&Protocol::WebDav, "alice", "cloud", 443, None),
            "cloud:443"
        );
        assert_eq!(
            display_identity(&Protocol::WebDav, "alice", "cloud", 443, Some("")),
            "cloud:443"
        );
    }

    #[test]
    fn display_identity_omits_empty_user() {
        assert_eq!(
            display_identity(&Protocol::Ssh, "", "srv", 22, None),
            "srv:22"
        );
        assert_eq!(
            display_identity(&Protocol::Ssh, "root", "srv", 22, None),
            "root@srv:22"
        );
    }

    // --- ConnectionConfig ---

    #[test]
    fn plain_protocols_have_no_webdav_config() {
        let ssh = ConnectionConfig::ssh("h".into(), "u".into(), 22, AuthMethod::Agent, None);
        let sftp = ConnectionConfig::sftp("h".into(), "u".into(), 22, AuthMethod::Password);
        let ftp = ConnectionConfig::ftp("h".into(), "u".into(), 21);
        assert!(ssh.webdav_config().is_none());
        assert!(sftp.webdav_config().is_none());
        assert!(ftp.webdav_config().is_none());
    }

    #[test]
    fn all_three_plain_constructors_omit_empty_user_in_label() {
        // Regression guard: connect_direct applies the "empty user" branch for every
        // protocol, so `--protocol sftp --host h` without --user must stay "h:22".
        assert_eq!(
            ConnectionConfig::ssh("h".into(), String::new(), 22, AuthMethod::Agent, None).label(),
            "h:22"
        );
        assert_eq!(
            ConnectionConfig::sftp("h".into(), String::new(), 22, AuthMethod::Agent).label(),
            "h:22"
        );
        assert_eq!(
            ConnectionConfig::ftp("h".into(), String::new(), 21).label(),
            "h:21"
        );
    }

    #[test]
    fn webdav_constructor_derives_fields_from_url() {
        let parsed =
            parse_webdav_url("https://cloud.exemple.fr/remote.php/dav/files/romain/").unwrap();
        let auth = WebDavAuth::Basic {
            user: "romain".to_string(),
            password: "pw".to_string(),
        };
        let conn = ConnectionConfig::webdav(&parsed, auth, false);
        assert_eq!(conn.protocol(), &Protocol::WebDav);
        assert_eq!(conn.host(), "cloud.exemple.fr");
        assert_eq!(conn.port(), 443);
        assert_eq!(conn.user(), "romain");
        assert_eq!(
            conn.label(),
            "https://cloud.exemple.fr/remote.php/dav/files/romain/"
        );
        let cfg = conn.webdav_config().expect("webdav config");
        assert_eq!(cfg.url, parsed.normalized);
        assert!(!cfg.insecure_tls);
    }

    #[test]
    fn webdav_constructor_prefills_user_from_url_userinfo() {
        let parsed = parse_webdav_url("https://alice@cloud/dav/").unwrap();
        let conn = ConnectionConfig::webdav(&parsed, WebDavAuth::Anonymous, false);
        assert_eq!(conn.user(), "alice");
    }

    #[test]
    fn webdav_saved_does_not_parse() {
        // Deliberately not a normalized URL: the loading path must not fail on it.
        let conn = ConnectionConfig::webdav_saved(
            "not a url at all".to_string(),
            "cloud".to_string(),
            "alice".to_string(),
            443,
            WebDavAuth::Bearer("tok".to_string()),
            true,
        );
        assert_eq!(conn.protocol(), &Protocol::WebDav);
        let cfg = conn.webdav_config().unwrap();
        assert_eq!(cfg.url, "not a url at all");
        assert!(cfg.insecure_tls);
    }

    #[test]
    fn webdav_insecure_retry_guards() {
        // Not WebDAV at all.
        let ssh = ConnectionConfig::ssh("h".into(), "u".into(), 22, AuthMethod::Agent, None);
        assert!(ssh.webdav_insecure_retry().is_none());

        let parsed = parse_webdav_url("https://cloud/dav/").unwrap();
        let secure = ConnectionConfig::webdav(&parsed, WebDavAuth::Anonymous, false);
        let retried = secure.webdav_insecure_retry().expect("retry offered once");
        assert!(retried.webdav_config().unwrap().insecure_tls);

        // Already insecure: no second offer, which is what stops the dialog looping.
        assert!(retried.webdav_insecure_retry().is_none());
    }
}
