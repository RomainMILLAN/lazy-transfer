use std::io::{Read, Write};
use std::sync::{mpsc, Mutex};
use std::thread;

use suppaftp::FtpStream;

use crate::transfer::backend::RemoteBackend;
use crate::transfer::ls_parse::parse_ls_output;
use crate::transfer::stream::{StreamHandle, StreamLine};
use crate::transfer::types::FileEntry;

/// FTP backend using the `suppaftp` crate.
pub struct FtpBackend {
    ftp: Mutex<FtpStream>,
    home_dir: String,
}

// Safety: FtpStream is !Send by default, but we serialize all access
// through a Mutex, ensuring only one thread accesses it at a time.
unsafe impl Send for FtpBackend {}
unsafe impl Sync for FtpBackend {}

impl FtpBackend {
    pub fn connect(host: &str, port: u16, user: &str, password: &str) -> Result<Self, String> {
        let addr = format!("{}:{}", host, port);
        let mut ftp =
            FtpStream::connect(&addr).map_err(|e| format!("FTP connect failed: {}", e))?;
        ftp.login(user, password)
            .map_err(|e| format!("FTP login failed: {}", e))?;

        let home_dir = ftp.pwd().map_err(|e| format!("FTP pwd failed: {}", e))?;

        Ok(FtpBackend {
            ftp: Mutex::new(ftp),
            home_dir,
        })
    }
}

impl RemoteBackend for FtpBackend {
    fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>, String> {
        let mut ftp = self.ftp.lock().map_err(|e| e.to_string())?;
        let lines = ftp
            .list(Some(path))
            .map_err(|e| format!("FTP list failed: {}", e))?;

        let combined = lines.join("\n");
        let mut entries = parse_ls_output(&combined);

        // Add ".." if not at root
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
        let mut ftp = self.ftp.lock().map_err(|e| e.to_string())?;
        ftp.mkdir(path)
            .map_err(|e| format!("FTP mkdir failed: {}", e))?;
        Ok(())
    }

    fn delete(&self, path: &str) -> Result<(), String> {
        let mut ftp = self.ftp.lock().map_err(|e| e.to_string())?;
        // Try as file first
        match ftp.rm(path) {
            Ok(()) => Ok(()),
            Err(_) => {
                // Try recursive directory delete
                delete_dir_recursive(&mut ftp, path)
            }
        }
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        let mut ftp = self.ftp.lock().map_err(|e| e.to_string())?;
        ftp.rename(from, to)
            .map_err(|e| format!("FTP rename failed: {}", e))?;
        Ok(())
    }

    fn home_dir(&self) -> Result<String, String> {
        Ok(self.home_dir.clone())
    }

    fn test_connection(&self) -> Result<String, String> {
        let mut ftp = self.ftp.lock().map_err(|e| e.to_string())?;
        let pwd = ftp.pwd().map_err(|e| format!("FTP pwd failed: {}", e))?;
        Ok(pwd)
    }

    fn upload(&self, local_path: &str, remote_path: &str) -> Result<StreamHandle, String> {
        let local_path = local_path.to_string();
        let remote_path = remote_path.to_string();

        let local_file =
            std::fs::File::open(&local_path).map_err(|e| format!("open local: {}", e))?;
        let total_size = local_file.metadata().map(|m| m.len()).unwrap_or(0);

        let ftp_ptr = &self.ftp as *const Mutex<FtpStream> as usize;
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let ftp_mutex = unsafe { &*(ftp_ptr as *const Mutex<FtpStream>) };
            let result = (|| -> Result<(), String> {
                let mut ftp = ftp_mutex.lock().map_err(|e| e.to_string())?;
                let mut reader = ProgressReader::new(local_file, total_size, tx.clone());
                ftp.put_file(&remote_path, &mut reader)
                    .map_err(|e| format!("FTP put failed: {}", e))?;
                Ok(())
            })();

            match result {
                Ok(()) => {
                    let _ = tx.send(StreamLine {
                        text: String::new(),
                        err: None,
                        done: true,
                    });
                }
                Err(e) => {
                    let _ = tx.send(StreamLine {
                        text: String::new(),
                        err: Some(e),
                        done: true,
                    });
                }
            }
        });

        Ok(StreamHandle {
            rx,
            child_pid: None,
        })
    }

    fn download(&self, remote_path: &str, local_path: &str) -> Result<StreamHandle, String> {
        let remote_path = remote_path.to_string();
        let local_path = local_path.to_string();

        let ftp_ptr = &self.ftp as *const Mutex<FtpStream> as usize;
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let ftp_mutex = unsafe { &*(ftp_ptr as *const Mutex<FtpStream>) };
            let result = (|| -> Result<(), String> {
                let mut ftp = ftp_mutex.lock().map_err(|e| e.to_string())?;

                // Get file size for progress
                let size = ftp.size(&remote_path).unwrap_or(0);

                let mut local_file = std::fs::File::create(&local_path)
                    .map_err(|e| format!("create local: {}", e))?;

                let cursor = ftp
                    .retr_as_buffer(&remote_path)
                    .map_err(|e| format!("FTP retr failed: {}", e))?;

                let data = cursor.into_inner();
                let total = data.len() as u64;
                let chunk_size = 64 * 1024;
                let mut written: u64 = 0;
                let mut last_pct: u8 = 0;

                for chunk in data.chunks(chunk_size) {
                    local_file
                        .write_all(chunk)
                        .map_err(|e| format!("write: {}", e))?;
                    written += chunk.len() as u64;
                    let report_total = if size > 0 { size as u64 } else { total };
                    if let Some(pct) = (written * 100).checked_div(report_total).map(|p| p as u8) {
                        if pct > last_pct {
                            last_pct = pct;
                            let _ = tx.send(StreamLine {
                                text: format!("{}%", pct),
                                err: None,
                                done: false,
                            });
                        }
                    }
                }

                Ok(())
            })();

            match result {
                Ok(()) => {
                    let _ = tx.send(StreamLine {
                        text: String::new(),
                        err: None,
                        done: true,
                    });
                }
                Err(e) => {
                    let _ = tx.send(StreamLine {
                        text: String::new(),
                        err: Some(e),
                        done: true,
                    });
                }
            }
        });

        Ok(StreamHandle {
            rx,
            child_pid: None,
        })
    }

    fn upload_dir(&self, local_path: &str, remote_dest: &str) -> Result<StreamHandle, String> {
        // FTP has no remote tar — walk local directory recursively
        let local_path = local_path.to_string();
        let remote_dest = remote_dest.to_string();

        let ftp_ptr = &self.ftp as *const Mutex<FtpStream> as usize;
        let (tx, rx) = mpsc::channel();

        // Collect all files to upload
        let files = collect_local_files(&local_path)?;
        let total = files.len();

        thread::spawn(move || {
            let ftp_mutex = unsafe { &*(ftp_ptr as *const Mutex<FtpStream>) };
            let result = (|| -> Result<(), String> {
                let mut ftp = ftp_mutex.lock().map_err(|e| e.to_string())?;

                let base = std::path::Path::new(&local_path);
                let base_name = base
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                for (i, file_path) in files.iter().enumerate() {
                    let rel = file_path.strip_prefix(base).unwrap_or(file_path);
                    let remote = format!(
                        "{}/{}/{}",
                        remote_dest.trim_end_matches('/'),
                        base_name,
                        rel.to_string_lossy()
                    );

                    if file_path.is_dir() {
                        // Ignore errors on mkdir (dir may already exist)
                        let _ = ftp.mkdir(&remote);
                    } else {
                        // Ensure parent dirs exist
                        if let Some(parent) = std::path::Path::new(&remote).parent() {
                            let _ = ftp.mkdir(parent.to_string_lossy());
                        }
                        let mut f = std::fs::File::open(file_path)
                            .map_err(|e| format!("open {}: {}", file_path.display(), e))?;
                        ftp.put_file(&remote, &mut f)
                            .map_err(|e| format!("put {}: {}", remote, e))?;
                    }

                    if let Some(pct) = ((i + 1) * 100).checked_div(total).map(|p| p as u8) {
                        let _ = tx.send(StreamLine {
                            text: format!("{}%", pct),
                            err: None,
                            done: false,
                        });
                    }
                }

                Ok(())
            })();

            match result {
                Ok(()) => {
                    let _ = tx.send(StreamLine {
                        text: String::new(),
                        err: None,
                        done: true,
                    });
                }
                Err(e) => {
                    let _ = tx.send(StreamLine {
                        text: String::new(),
                        err: Some(e),
                        done: true,
                    });
                }
            }
        });

        Ok(StreamHandle {
            rx,
            child_pid: None,
        })
    }

    fn download_dir(&self, remote_path: &str, local_dest: &str) -> Result<StreamHandle, String> {
        let remote_path = remote_path.to_string();
        let local_dest = local_dest.to_string();

        let ftp_ptr = &self.ftp as *const Mutex<FtpStream> as usize;
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let ftp_mutex = unsafe { &*(ftp_ptr as *const Mutex<FtpStream>) };
            let result = (|| -> Result<(), String> {
                let mut ftp = ftp_mutex.lock().map_err(|e| e.to_string())?;
                download_dir_recursive(&mut ftp, &remote_path, &local_dest, &tx)
            })();

            match result {
                Ok(()) => {
                    let _ = tx.send(StreamLine {
                        text: String::new(),
                        err: None,
                        done: true,
                    });
                }
                Err(e) => {
                    let _ = tx.send(StreamLine {
                        text: String::new(),
                        err: Some(e),
                        done: true,
                    });
                }
            }
        });

        Ok(StreamHandle {
            rx,
            child_pid: None,
        })
    }
}

/// A Read wrapper that sends progress updates.
struct ProgressReader {
    inner: std::fs::File,
    total: u64,
    sent: u64,
    last_pct: u8,
    tx: mpsc::Sender<StreamLine>,
}

impl ProgressReader {
    fn new(file: std::fs::File, total: u64, tx: mpsc::Sender<StreamLine>) -> Self {
        ProgressReader {
            inner: file,
            total,
            sent: 0,
            last_pct: 0,
            tx,
        }
    }
}

impl Read for ProgressReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.sent += n as u64;
        if let Some(pct) = (self.sent * 100).checked_div(self.total).map(|p| p as u8) {
            if pct > self.last_pct {
                self.last_pct = pct;
                let _ = self.tx.send(StreamLine {
                    text: format!("{}%", pct),
                    err: None,
                    done: false,
                });
            }
        }
        Ok(n)
    }
}

fn delete_dir_recursive(ftp: &mut FtpStream, path: &str) -> Result<(), String> {
    let lines = ftp
        .list(Some(path))
        .map_err(|e| format!("list {}: {}", path, e))?;

    let combined = lines.join("\n");
    let entries = parse_ls_output(&combined);

    for entry in &entries {
        if entry.name == "." || entry.name == ".." {
            continue;
        }
        let full = format!("{}/{}", path.trim_end_matches('/'), entry.name);
        if entry.is_dir {
            delete_dir_recursive(ftp, &full)?;
        } else {
            ftp.rm(&full).map_err(|e| format!("rm {}: {}", full, e))?;
        }
    }

    ftp.rmdir(path)
        .map_err(|e| format!("rmdir {}: {}", path, e))?;
    Ok(())
}

fn download_dir_recursive(
    ftp: &mut FtpStream,
    remote_path: &str,
    local_dest: &str,
    tx: &mpsc::Sender<StreamLine>,
) -> Result<(), String> {
    let dir_name = remote_path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(remote_path);
    let local_dir = format!("{}/{}", local_dest.trim_end_matches('/'), dir_name);

    std::fs::create_dir_all(&local_dir).map_err(|e| format!("mkdir {}: {}", local_dir, e))?;

    let lines = ftp
        .list(Some(remote_path))
        .map_err(|e| format!("list {}: {}", remote_path, e))?;

    let combined = lines.join("\n");
    let entries = parse_ls_output(&combined);

    for entry in &entries {
        if entry.name == "." || entry.name == ".." {
            continue;
        }
        let remote_full = format!("{}/{}", remote_path.trim_end_matches('/'), entry.name);
        if entry.is_dir {
            download_dir_recursive(ftp, &remote_full, &local_dir, tx)?;
        } else {
            let local_file = format!("{}/{}", local_dir, entry.name);
            let cursor = ftp
                .retr_as_buffer(&remote_full)
                .map_err(|e| format!("retr {}: {}", remote_full, e))?;
            std::fs::write(&local_file, cursor.into_inner())
                .map_err(|e| format!("write {}: {}", local_file, e))?;

            let _ = tx.send(StreamLine {
                text: format!("downloaded {}", entry.name),
                err: None,
                done: false,
            });
        }
    }

    Ok(())
}

fn collect_local_files(path: &str) -> Result<Vec<std::path::PathBuf>, String> {
    let mut result = Vec::new();
    collect_local_files_recursive(std::path::Path::new(path), &mut result)?;
    Ok(result)
}

fn collect_local_files_recursive(
    path: &std::path::Path,
    result: &mut Vec<std::path::PathBuf>,
) -> Result<(), String> {
    // Add the directory itself first
    result.push(path.to_path_buf());

    let entries =
        std::fs::read_dir(path).map_err(|e| format!("readdir {}: {}", path.display(), e))?;

    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_local_files_recursive(&p, result)?;
        } else {
            result.push(p);
        }
    }

    Ok(())
}
