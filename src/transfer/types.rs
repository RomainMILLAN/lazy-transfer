/// Protocol type for remote connections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Protocol {
    Ssh,
    Sftp,
    Ftp,
}

impl Default for Protocol {
    fn default() -> Self {
        Protocol::Ssh
    }
}

impl Protocol {
    pub fn default_port(&self) -> u16 {
        match self {
            Protocol::Ssh | Protocol::Sftp => 22,
            Protocol::Ftp => 21,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Protocol::Ssh => "SSH",
            Protocol::Sftp => "SFTP",
            Protocol::Ftp => "FTP",
        }
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
    pub protocol: Protocol,
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
            protocol: Protocol::Ssh,
            host: host.hostname.clone(),
            user: host.user.clone(),
            port: host.port,
            auth,
            label,
        }
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
