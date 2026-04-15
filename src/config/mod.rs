use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Config {
    pub ssh_bin: String,
    pub scp_bin: String,
    pub start_dir: String,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot determine current directory: {0}")]
    CurrentDir(String),
}

/// Resolves configuration: captures CWD, resolves SSH/SCP binaries (non-fatal if missing).
pub fn resolve() -> Result<Config, ConfigError> {
    let ssh_bin = std::env::var("SSH_BIN").unwrap_or_else(|_| "ssh".to_string());
    let scp_bin = std::env::var("SCP_BIN").unwrap_or_else(|_| "scp".to_string());

    // Warn if SSH/SCP not found, but don't fail (user may only use FTP/SFTP)
    if which::which(&ssh_bin).is_err() {
        log::warn!("{ssh_bin} not found in PATH — SSH/SCP protocol will not work");
    }
    if which::which(&scp_bin).is_err() {
        log::warn!("{scp_bin} not found in PATH — SSH/SCP protocol will not work");
    }

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
