use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::types::{AuthMethod, ConnectionConfig, Protocol, WebDavAuth};

/// All saved connections, persisted to ~/.config/lazy-transfer/connections.json.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SavedConnections {
    #[serde(default)]
    pub entries: Vec<SavedConnection>,
}

/// A single saved connection entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedConnection {
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub user: String,
    pub port: u16,
    pub auth_method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>, // base64 encoded (also holds the WebDAV bearer token)
    /// WebDAV only: the normalized collection URL. For WebDAV, `host`/`user`/`port`
    /// above are DERIVED duplicates kept for display and for the JSON schema — the
    /// backend only ever reads this field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// WebDAV only: accept invalid/self-signed TLS certificates.
    ///
    /// `#[serde(default)]` is NOT optional here: `load()` falls back to
    /// `Default::default()` on any deserialization error, so a mandatory field would
    /// silently wipe every existing connection of every user.
    #[serde(default, skip_serializing_if = "is_false")]
    pub insecure_tls: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl SavedConnection {
    pub fn to_connection_config(&self) -> ConnectionConfig {
        // Tolerant on purpose: an unknown protocol string must not break loading.
        let protocol = Protocol::from_str_opt(&self.protocol).unwrap_or(Protocol::Ssh);

        // WebDAV branches out before the AuthMethod logic: `auth` is not meaningful
        // for it, and the URL is deliberately NOT parsed here so that this path
        // cannot fail. An invalid URL surfaces at connect time instead.
        if protocol.uses_url() {
            return ConnectionConfig::webdav_saved(
                self.url.clone().unwrap_or_default(),
                self.host.clone(),
                self.user.clone(),
                self.port,
                self.to_webdav_auth(),
                self.insecure_tls,
            );
        }

        let auth = match self.auth_method.as_str() {
            "key" => AuthMethod::Key(self.identity_file.clone().unwrap_or_default()),
            "password" => AuthMethod::Password,
            _ => AuthMethod::Agent,
        };
        match protocol {
            Protocol::Sftp => {
                ConnectionConfig::sftp(self.host.clone(), self.user.clone(), self.port, auth)
            }
            Protocol::Ftp => ConnectionConfig::ftp(self.host.clone(), self.user.clone(), self.port),
            // Ssh, and the tolerant fallback for anything unknown.
            _ => ConnectionConfig::ssh(self.host.clone(), self.user.clone(), self.port, auth, None),
        }
    }

    /// Rebuild the WebDAV credentials from the persisted `auth_method` + secret.
    fn to_webdav_auth(&self) -> WebDavAuth {
        match self.auth_method.as_str() {
            "basic" => WebDavAuth::Basic {
                user: self.user.clone(),
                password: self.decoded_password().unwrap_or_default(),
            },
            "bearer" => WebDavAuth::Bearer(self.decoded_password().unwrap_or_default()),
            _ => WebDavAuth::Anonymous,
        }
    }

    pub fn from_connection_config(
        name: &str,
        config: &ConnectionConfig,
        password: Option<&str>,
    ) -> Self {
        // For WebDAV the secret lives in the config itself, so the `password`
        // argument is IGNORED: the caller's `pending_password` is never populated on
        // that path, and honouring it would save a connection with no credentials.
        if let Some(dav) = config.webdav_config() {
            return SavedConnection {
                name: name.to_string(),
                protocol: Protocol::WebDav.as_str().to_string(),
                host: config.host().to_string(),
                user: dav.auth.user().unwrap_or_default().to_string(),
                port: config.port(),
                auth_method: dav.auth.as_str().to_string(),
                identity_file: None,
                password: dav.auth.secret().map(encode_password),
                url: Some(dav.url.clone()),
                insecure_tls: dav.insecure_tls,
            };
        }

        let (auth_method, identity_file) = match config.auth() {
            AuthMethod::Key(path) => ("key", Some(path.clone())),
            AuthMethod::Password => ("password", None),
            AuthMethod::Agent => ("agent", None),
        };
        SavedConnection {
            name: name.to_string(),
            protocol: config.protocol().as_str().to_string(),
            host: config.host().to_string(),
            user: config.user().to_string(),
            port: config.port(),
            auth_method: auth_method.to_string(),
            identity_file,
            password: password.map(encode_password),
            url: None,
            insecure_tls: false,
        }
    }

    pub fn decoded_password(&self) -> Option<String> {
        self.password.as_ref().and_then(|p| decode_password(p).ok())
    }

    pub fn matches_protocol(&self, protocol: &Protocol) -> bool {
        self.protocol == protocol.as_str()
    }
}

fn connections_path() -> Option<PathBuf> {
    let config = dirs::config_dir()?;
    Some(config.join("lazy-transfer").join("connections.json"))
}

pub fn load() -> SavedConnections {
    let path = match connections_path() {
        Some(p) => p,
        None => return SavedConnections::default(),
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return SavedConnections::default(),
    };
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn save(conns: &SavedConnections) -> Result<(), String> {
    let path = connections_path().ok_or("cannot determine config directory")?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }

    let json = serde_json::to_string_pretty(conns).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, &json).map_err(|e| format!("write: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&path, perms).map_err(|e| format!("chmod: {e}"))?;
    }

    log::info!("saved connections to {}", path.display());
    Ok(())
}

pub fn encode_password(plain: &str) -> String {
    STANDARD.encode(plain)
}

pub fn decode_password(encoded: &str) -> Result<String, String> {
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|e| format!("base64 decode: {e}"))?;
    String::from_utf8(bytes).map_err(|e| format!("utf8: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let plain = "my-secret-password!@#$%";
        let encoded = encode_password(plain);
        assert_ne!(encoded, plain);
        let decoded = decode_password(&encoded).unwrap();
        assert_eq!(decoded, plain);
    }

    #[test]
    fn decode_invalid_base64() {
        assert!(decode_password("not-valid-base64!!!").is_err());
    }

    #[test]
    fn saved_connection_to_config() {
        let saved = SavedConnection {
            name: "My Server".to_string(),
            protocol: "ftp".to_string(),
            host: "ftp.example.com".to_string(),
            user: "admin".to_string(),
            port: 21,
            auth_method: "password".to_string(),
            identity_file: None,
            password: Some(encode_password("secret")),
            url: None,
            insecure_tls: false,
        };
        let config = saved.to_connection_config();
        assert_eq!(config.protocol(), &Protocol::Ftp);
        assert_eq!(config.host(), "ftp.example.com");
        assert!(matches!(config.auth(), AuthMethod::Password));
    }

    #[test]
    fn matches_protocol_filter() {
        let saved = SavedConnection {
            name: "test".to_string(),
            protocol: "sftp".to_string(),
            host: "host".to_string(),
            user: "user".to_string(),
            port: 22,
            auth_method: "agent".to_string(),
            identity_file: None,
            password: None,
            url: None,
            insecure_tls: false,
        };
        assert!(saved.matches_protocol(&Protocol::Sftp));
        assert!(!saved.matches_protocol(&Protocol::Ftp));
        assert!(!saved.matches_protocol(&Protocol::Ssh));
        assert!(!saved.matches_protocol(&Protocol::WebDav));
    }

    // --- WebDAV ---

    fn webdav_conn(auth: WebDavAuth, insecure_tls: bool) -> ConnectionConfig {
        let parsed = crate::transfer::types::parse_webdav_url(
            "https://cloud.exemple.fr/remote.php/dav/files/romain/",
        )
        .unwrap();
        ConnectionConfig::webdav(&parsed, auth, insecure_tls)
    }

    #[test]
    fn webdav_basic_roundtrip() {
        let conn = webdav_conn(
            WebDavAuth::Basic {
                user: "romain".to_string(),
                password: "secret".to_string(),
            },
            true,
        );
        // The `password` argument is deliberately None: the secret must come from
        // the config itself, otherwise the connection is saved without credentials.
        let saved = SavedConnection::from_connection_config("dav", &conn, None);
        assert_eq!(saved.protocol, "webdav");
        assert_eq!(saved.auth_method, "basic");
        assert_eq!(saved.user, "romain");
        assert_eq!(
            saved.url.as_deref(),
            Some("https://cloud.exemple.fr/remote.php/dav/files/romain/")
        );
        assert!(saved.insecure_tls);
        assert_eq!(saved.decoded_password().as_deref(), Some("secret"));

        let back = saved.to_connection_config();
        assert_eq!(back.protocol(), &Protocol::WebDav);
        let cfg = back.webdav_config().expect("webdav config restored");
        assert!(cfg.insecure_tls);
        assert_eq!(
            cfg.auth,
            WebDavAuth::Basic {
                user: "romain".to_string(),
                password: "secret".to_string()
            }
        );
    }

    #[test]
    fn webdav_bearer_roundtrip() {
        let conn = webdav_conn(WebDavAuth::Bearer("tok".to_string()), false);
        let saved = SavedConnection::from_connection_config("dav", &conn, None);
        assert_eq!(saved.auth_method, "bearer");
        assert_eq!(saved.decoded_password().as_deref(), Some("tok"));
        assert!(!saved.insecure_tls);

        let cfg = saved.to_connection_config();
        assert_eq!(
            cfg.webdav_config().unwrap().auth,
            WebDavAuth::Bearer("tok".to_string())
        );
    }

    #[test]
    fn webdav_anonymous_has_no_secret() {
        let conn = webdav_conn(WebDavAuth::Anonymous, false);
        let saved = SavedConnection::from_connection_config("dav", &conn, None);
        assert_eq!(saved.auth_method, "anonymous");
        assert!(saved.password.is_none());
        assert_eq!(
            saved.to_connection_config().webdav_config().unwrap().auth,
            WebDavAuth::Anonymous
        );
    }

    #[test]
    fn from_connection_config_ignores_password_arg_for_webdav() {
        let conn = webdav_conn(WebDavAuth::Bearer("real-token".to_string()), false);
        let saved = SavedConnection::from_connection_config("dav", &conn, Some("stale"));
        assert_eq!(saved.decoded_password().as_deref(), Some("real-token"));
    }

    /// THE critical backward-compatibility test: `load()` does
    /// `unwrap_or_default()`, so a mandatory new field would silently erase every
    /// connection the user has.
    #[test]
    fn legacy_json_without_new_fields() {
        let json = r#"{"entries":[{"name":"s","protocol":"ssh","host":"h","user":"u","port":22,"auth_method":"agent"}]}"#;
        let conns: SavedConnections = serde_json::from_str(json).expect("legacy json must load");
        assert_eq!(conns.entries.len(), 1);
        let e = &conns.entries[0];
        assert_eq!(e.name, "s");
        assert!(e.url.is_none());
        assert!(!e.insecure_tls);
    }

    #[test]
    fn serialized_json_omits_absent_webdav_fields() {
        let conn = ConnectionConfig::ssh("h".into(), "u".into(), 22, AuthMethod::Agent, None);
        let saved = SavedConnection::from_connection_config("s", &conn, None);
        let json = serde_json::to_string(&saved).unwrap();
        assert!(!json.contains("insecure_tls"), "{json}");
        assert!(!json.contains("\"url\""), "{json}");
    }

    #[test]
    fn unknown_protocol_falls_back_to_ssh() {
        let json = r#"{"entries":[{"name":"x","protocol":"quantum","host":"h","user":"u","port":1,"auth_method":"agent"}]}"#;
        let conns: SavedConnections = serde_json::from_str(json).unwrap();
        assert_eq!(
            conns.entries[0].to_connection_config().protocol(),
            &Protocol::Ssh
        );
    }
}
