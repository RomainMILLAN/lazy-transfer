use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::types::{AuthMethod, ConnectionConfig, Protocol};

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
    pub password: Option<String>, // base64 encoded
}

impl SavedConnection {
    pub fn to_connection_config(&self) -> ConnectionConfig {
        let protocol = match self.protocol.as_str() {
            "sftp" => Protocol::Sftp,
            "ftp" => Protocol::Ftp,
            _ => Protocol::Ssh,
        };
        let auth = match self.auth_method.as_str() {
            "key" => AuthMethod::Key(self.identity_file.clone().unwrap_or_default()),
            "password" => AuthMethod::Password,
            _ => AuthMethod::Agent,
        };
        ConnectionConfig {
            protocol: protocol.clone(),
            host: self.host.clone(),
            user: self.user.clone(),
            port: self.port,
            auth,
            label: format!("{}@{}:{}", self.user, self.host, self.port),
            ssh_alias: None,
        }
    }

    pub fn from_connection_config(
        name: &str,
        config: &ConnectionConfig,
        password: Option<&str>,
    ) -> Self {
        let protocol = match config.protocol {
            Protocol::Ssh => "ssh",
            Protocol::Sftp => "sftp",
            Protocol::Ftp => "ftp",
        };
        let (auth_method, identity_file) = match &config.auth {
            AuthMethod::Key(path) => ("key", Some(path.clone())),
            AuthMethod::Password => ("password", None),
            AuthMethod::Agent => ("agent", None),
        };
        SavedConnection {
            name: name.to_string(),
            protocol: protocol.to_string(),
            host: config.host.clone(),
            user: config.user.clone(),
            port: config.port,
            auth_method: auth_method.to_string(),
            identity_file,
            password: password.map(encode_password),
        }
    }

    pub fn decoded_password(&self) -> Option<String> {
        self.password.as_ref().and_then(|p| decode_password(p).ok())
    }

    pub fn matches_protocol(&self, protocol: &Protocol) -> bool {
        match protocol {
            Protocol::Ssh => self.protocol == "ssh",
            Protocol::Sftp => self.protocol == "sftp",
            Protocol::Ftp => self.protocol == "ftp",
        }
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
        };
        let config = saved.to_connection_config();
        assert_eq!(config.protocol, Protocol::Ftp);
        assert_eq!(config.host, "ftp.example.com");
        assert!(matches!(config.auth, AuthMethod::Password));
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
        };
        assert!(saved.matches_protocol(&Protocol::Sftp));
        assert!(!saved.matches_protocol(&Protocol::Ftp));
        assert!(!saved.matches_protocol(&Protocol::Ssh));
    }
}
