/// Authentication method for SSH connections.
#[derive(Debug, Clone)]
pub enum AuthMethod {
    /// Path to an identity file (SSH key).
    Key(String),
    /// Password-based authentication.
    Password,
    /// Use the SSH agent.
    Agent,
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
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    pub host: String,
    pub user: String,
    pub port: u16,
    pub auth: AuthMethod,
    pub label: String,
}

impl ConnectionConfig {
    pub fn from_ssh_host(host: &SshHost) -> Self {
        let auth = if host.identity_file.is_empty() {
            AuthMethod::Agent
        } else {
            AuthMethod::Key(host.identity_file.clone())
        };
        let label = if host.user.is_empty() {
            format!("{}:{}", host.hostname, host.port)
        } else {
            format!("{}@{}:{}", host.user, host.hostname, host.port)
        };
        ConnectionConfig {
            host: host.hostname.clone(),
            user: host.user.clone(),
            port: host.port,
            auth,
            label,
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
