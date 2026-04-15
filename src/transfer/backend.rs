use crate::transfer::types::FileEntry;

use super::stream::StreamHandle;

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
    /// Upload a directory: tar locally, scp the archive, extract remotely, cleanup.
    fn upload_dir(&self, local_path: &str, remote_dest: &str) -> Result<StreamHandle, String>;
    /// Download a directory: tar remotely, scp the archive, extract locally, cleanup.
    fn download_dir(&self, remote_path: &str, local_dest: &str) -> Result<StreamHandle, String>;
}
