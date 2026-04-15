use crate::transfer::types::FileEntry;

use super::exec::StreamHandle;

/// RemoteBackend abstracts remote filesystem operations.
/// V1: SSH/SCP implementation. Future: FTP, SFTP.
pub trait RemoteBackend: Send + Sync {
    fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>, String>;
    fn mkdir(&self, path: &str) -> Result<(), String>;
    fn delete(&self, path: &str) -> Result<(), String>;
    fn rename(&self, from: &str, to: &str) -> Result<(), String>;
    fn home_dir(&self) -> Result<String, String>;
    fn test_connection(&self) -> Result<String, String>;
    fn upload(&self, local_path: &str, remote_path: &str) -> Result<StreamHandle, String>;
    fn download(&self, remote_path: &str, local_path: &str) -> Result<StreamHandle, String>;
}
