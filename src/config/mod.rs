use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Config {
    pub ssh_bin: String,
    pub scp_bin: String,
    pub start_dir: String,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("'{0}' not found in PATH. Please install OpenSSH.")]
    BinaryNotFound(String),
    #[error("cannot determine current directory: {0}")]
    CurrentDir(String),
}

/// Resolves configuration: validates ssh/scp are available, captures CWD.
pub fn resolve() -> Result<Config, ConfigError> {
    let ssh_bin = std::env::var("SSH_BIN").unwrap_or_else(|_| "ssh".to_string());
    let scp_bin = std::env::var("SCP_BIN").unwrap_or_else(|_| "scp".to_string());

    which::which(&ssh_bin).map_err(|_| ConfigError::BinaryNotFound(ssh_bin.clone()))?;
    which::which(&scp_bin).map_err(|_| ConfigError::BinaryNotFound(scp_bin.clone()))?;

    let start_dir = std::env::current_dir()
        .map_err(|e| ConfigError::CurrentDir(e.to_string()))?
        .to_string_lossy()
        .to_string();

    Ok(Config {
        ssh_bin,
        scp_bin,
        start_dir,
    })
}
