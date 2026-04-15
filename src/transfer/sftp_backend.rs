use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::{mpsc, Mutex};
use std::thread;
use std::time::Duration;

use crate::transfer::backend::RemoteBackend;
use crate::transfer::stream::{StreamHandle, StreamLine};
use crate::transfer::types::{AuthMethod, FileEntry};

/// Holds an SSH session and a persistent SFTP subsystem handle.
/// The Sftp borrows from Session, so we use Box + raw pointer to make it self-referential.
struct SftpHandle {
    sftp: ssh2::Sftp,
    // Session must be kept alive and dropped AFTER sftp.
    // We use Box to get a stable address, then create Sftp from a raw pointer.
    _session: Box<ssh2::Session>,
}

impl SftpHandle {
    fn new(session: ssh2::Session) -> Result<Self, String> {
        let session = Box::new(session);
        // Safety: we keep `_session` alive for the lifetime of this struct,
        // and Sftp is dropped before Session because of field order (sftp first).
        let sftp = unsafe {
            let session_ref: &ssh2::Session = &*(&*session as *const ssh2::Session);
            session_ref
                .sftp()
                .map_err(|e| format!("sftp subsystem: {}", e))?
        };
        Ok(SftpHandle {
            sftp,
            _session: session,
        })
    }
}

/// SFTP backend using the `ssh2` crate (libssh2).
/// Uses a single persistent SFTP subsystem handle to avoid channel startup failures.
pub struct SftpBackend {
    handle: Mutex<SftpHandle>,
    #[allow(dead_code)]
    host: String,
    #[allow(dead_code)]
    user: String,
    home_dir: String,
}

// Safety: all access to the session/sftp is serialized through a Mutex.
unsafe impl Send for SftpBackend {}
unsafe impl Sync for SftpBackend {}

impl SftpBackend {
    fn create_session(host: &str, port: u16) -> Result<ssh2::Session, String> {
        let addr = format!("{}:{}", host, port);
        log::info!("sftp: resolving {}", addr);
        let socket_addr = addr
            .to_socket_addrs()
            .map_err(|e| format!("DNS resolve failed: {}", e))?
            .next()
            .ok_or_else(|| format!("cannot resolve {}", addr))?;

        log::info!("sftp: TCP connect to {} (10s timeout)", socket_addr);
        let tcp = TcpStream::connect_timeout(&socket_addr, Duration::from_secs(10))
            .map_err(|e| format!("TCP connect failed: {}", e))?;
        log::info!("sftp: TCP connected");

        let mut session =
            ssh2::Session::new().map_err(|e| format!("session create failed: {}", e))?;
        session.set_timeout(10_000);
        session.set_tcp_stream(tcp);

        log::info!("sftp: SSH handshake...");
        session
            .handshake()
            .map_err(|e| format!("SSH handshake failed: {}", e))?;
        log::info!("sftp: handshake OK");

        Ok(session)
    }

    fn finalize(session: ssh2::Session, host: &str, user: &str) -> Result<Self, String> {
        log::info!("sftp: opening SFTP subsystem...");
        let handle = SftpHandle::new(session).map_err(|e| {
            log::error!("sftp: SftpHandle::new failed: {}", e);
            e
        })?;

        log::info!("sftp: getting home dir via SFTP realpath...");
        let home_dir = {
            let real = handle
                .sftp
                .realpath(Path::new("."))
                .map_err(|e| format!("realpath failed: {}", e))?;
            real.to_string_lossy().to_string()
        };
        log::info!("sftp: home_dir = {}", home_dir);

        Ok(SftpBackend {
            handle: Mutex::new(handle),
            host: host.to_string(),
            user: user.to_string(),
            home_dir,
        })
    }

    pub fn connect(host: &str, port: u16, user: &str, auth: &AuthMethod) -> Result<Self, String> {
        log::info!("sftp: connect {}@{}:{} auth={:?}", user, host, port, auth);
        let session = Self::create_session(host, port)?;

        match auth {
            AuthMethod::Key(path) => {
                log::info!("sftp: auth with key {}", path);
                session
                    .userauth_pubkey_file(user, None, Path::new(path), None)
                    .map_err(|e| format!("key auth failed: {}", e))?;
            }
            AuthMethod::Agent => {
                log::info!("sftp: auth with agent");
                session
                    .userauth_agent(user)
                    .map_err(|e| format!("agent auth failed: {}", e))?;
            }
            AuthMethod::Password => {
                return Err(
                    "SFTP password auth requires password — use connect_with_password".to_string(),
                );
            }
        }

        if !session.authenticated() {
            log::error!("sftp: authentication failed");
            return Err("authentication failed".to_string());
        }
        log::info!("sftp: authenticated OK");

        session.set_timeout(0);
        Self::finalize(session, host, user)
    }

    pub fn connect_with_password(
        host: &str,
        port: u16,
        user: &str,
        password: &str,
    ) -> Result<Self, String> {
        log::info!("sftp: connect_with_password {}@{}:{}", user, host, port);
        let session = Self::create_session(host, port)?;

        log::info!("sftp: auth with password");
        session
            .userauth_password(user, password)
            .map_err(|e| format!("password auth failed: {}", e))?;

        if !session.authenticated() {
            log::error!("sftp: password auth failed");
            return Err("authentication failed".to_string());
        }
        log::info!("sftp: authenticated OK");

        session.set_timeout(0);
        Self::finalize(session, host, user)
    }
}

fn format_unix_time(mtime: u64) -> String {
    let dt = chrono::DateTime::from_timestamp(mtime as i64, 0);
    match dt {
        Some(d) => d.format("%Y-%m-%d %H:%M").to_string(),
        None => String::new(),
    }
}

fn format_permissions(perm: u32) -> String {
    let file_type = if perm & 0o40000 != 0 {
        'd'
    } else if perm & 0o120000 == 0o120000 {
        'l'
    } else {
        '-'
    };
    let mut s = String::with_capacity(10);
    s.push(file_type);
    for shift in [6, 3, 0] {
        let bits = (perm >> shift) & 0o7;
        s.push(if bits & 4 != 0 { 'r' } else { '-' });
        s.push(if bits & 2 != 0 { 'w' } else { '-' });
        s.push(if bits & 1 != 0 { 'x' } else { '-' });
    }
    s
}

impl RemoteBackend for SftpBackend {
    fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>, String> {
        log::debug!("sftp list_dir: {}", path);
        let handle = self.handle.lock().map_err(|e| e.to_string())?;

        let entries = handle
            .sftp
            .readdir(Path::new(path))
            .map_err(|e| format!("readdir failed: {}", e))?;

        let mut dirs: Vec<FileEntry> = Vec::new();
        let mut files: Vec<FileEntry> = Vec::new();

        if path != "/" {
            dirs.push(FileEntry {
                name: "..".to_string(),
                is_dir: true,
                size: 0,
                modified: String::new(),
                permissions: String::new(),
            });
        }

        for (pathbuf, stat) in entries {
            let name = match pathbuf.file_name() {
                Some(n) => n.to_string_lossy().to_string(),
                None => continue,
            };
            if name == "." || name == ".." {
                continue;
            }

            let is_dir = stat.is_dir();
            let size = stat.size.unwrap_or(0);
            let modified = stat.mtime.map(format_unix_time).unwrap_or_default();
            let permissions = stat.perm.map(format_permissions).unwrap_or_default();

            let entry = FileEntry {
                name,
                is_dir,
                size,
                modified,
                permissions,
            };

            if is_dir {
                dirs.push(entry);
            } else {
                files.push(entry);
            }
        }

        dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        dirs.extend(files);

        log::debug!("sftp list_dir: {} entries", dirs.len());
        Ok(dirs)
    }

    fn mkdir(&self, path: &str) -> Result<(), String> {
        log::debug!("sftp mkdir: {}", path);
        let handle = self.handle.lock().map_err(|e| e.to_string())?;
        handle
            .sftp
            .mkdir(Path::new(path), 0o755)
            .map_err(|e| format!("mkdir: {}", e))
    }

    fn delete(&self, path: &str) -> Result<(), String> {
        log::debug!("sftp delete: {}", path);
        let handle = self.handle.lock().map_err(|e| e.to_string())?;
        match handle.sftp.unlink(Path::new(path)) {
            Ok(()) => Ok(()),
            Err(_) => delete_dir_recursive(&handle.sftp, path),
        }
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        log::debug!("sftp rename: {} -> {}", from, to);
        let handle = self.handle.lock().map_err(|e| e.to_string())?;
        handle
            .sftp
            .rename(Path::new(from), Path::new(to), None)
            .map_err(|e| format!("rename: {}", e))
    }

    fn home_dir(&self) -> Result<String, String> {
        Ok(self.home_dir.clone())
    }

    fn test_connection(&self) -> Result<String, String> {
        let handle = self.handle.lock().map_err(|e| e.to_string())?;
        let real = handle
            .sftp
            .realpath(Path::new("."))
            .map_err(|e| format!("realpath: {}", e))?;
        Ok(real.to_string_lossy().to_string())
    }

    fn upload(&self, local_path: &str, remote_path: &str) -> Result<StreamHandle, String> {
        log::info!("sftp upload: {} -> {}", local_path, remote_path);
        let local_path = local_path.to_string();
        let remote_path = remote_path.to_string();

        let local_file =
            std::fs::File::open(&local_path).map_err(|e| format!("open local: {}", e))?;
        let total_size = local_file.metadata().map(|m| m.len()).unwrap_or(0);

        let handle_ptr = &self.handle as *const Mutex<SftpHandle> as usize;
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let handle_mutex = unsafe { &*(handle_ptr as *const Mutex<SftpHandle>) };
            let result = (|| -> Result<(), String> {
                let handle = handle_mutex.lock().map_err(|e| e.to_string())?;

                let mut remote_file = handle
                    .sftp
                    .create(Path::new(&remote_path))
                    .map_err(|e| format!("create: {}", e))?;
                let mut local = std::io::BufReader::new(
                    std::fs::File::open(&local_path).map_err(|e| format!("open: {}", e))?,
                );

                let mut buf = vec![0u8; 64 * 1024];
                let mut sent: u64 = 0;
                let mut last_pct: u8 = 0;

                loop {
                    let n = local.read(&mut buf).map_err(|e| format!("read: {}", e))?;
                    if n == 0 {
                        break;
                    }
                    remote_file
                        .write_all(&buf[..n])
                        .map_err(|e| format!("write: {}", e))?;
                    sent += n as u64;
                    if total_size > 0 {
                        let pct = (sent * 100 / total_size) as u8;
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
                    log::error!("sftp upload error: {}", e);
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
        log::info!("sftp download: {} -> {}", remote_path, local_path);
        let remote_path = remote_path.to_string();
        let local_path = local_path.to_string();

        let handle_ptr = &self.handle as *const Mutex<SftpHandle> as usize;
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let handle_mutex = unsafe { &*(handle_ptr as *const Mutex<SftpHandle>) };
            let result = (|| -> Result<(), String> {
                let handle = handle_mutex.lock().map_err(|e| e.to_string())?;

                let stat = handle
                    .sftp
                    .stat(Path::new(&remote_path))
                    .map_err(|e| format!("stat: {}", e))?;
                let total_size = stat.size.unwrap_or(0);

                let mut remote_file = handle
                    .sftp
                    .open(Path::new(&remote_path))
                    .map_err(|e| format!("open: {}", e))?;
                let mut local_file =
                    std::fs::File::create(&local_path).map_err(|e| format!("create: {}", e))?;

                let mut buf = vec![0u8; 64 * 1024];
                let mut received: u64 = 0;
                let mut last_pct: u8 = 0;

                loop {
                    let n = remote_file
                        .read(&mut buf)
                        .map_err(|e| format!("read: {}", e))?;
                    if n == 0 {
                        break;
                    }
                    local_file
                        .write_all(&buf[..n])
                        .map_err(|e| format!("write: {}", e))?;
                    received += n as u64;
                    if total_size > 0 {
                        let pct = (received * 100 / total_size) as u8;
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
                    log::error!("sftp download error: {}", e);
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
        log::info!("sftp upload_dir: {} -> {}", local_path, remote_dest);
        let local_path = local_path.to_string();
        let remote_dest = remote_dest.to_string();

        let handle_ptr = &self.handle as *const Mutex<SftpHandle> as usize;
        let (tx, rx) = mpsc::channel();

        let files = collect_local_files(&local_path)?;
        let total = files.len();

        thread::spawn(move || {
            let handle_mutex = unsafe { &*(handle_ptr as *const Mutex<SftpHandle>) };
            let result = (|| -> Result<(), String> {
                let handle = handle_mutex.lock().map_err(|e| e.to_string())?;

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
                        log::debug!("sftp upload_dir: mkdir {}", remote);
                        let _ = handle.sftp.mkdir(Path::new(&remote), 0o755);
                    } else {
                        log::debug!("sftp upload_dir: uploading {}", remote);
                        let mut local_file = std::fs::File::open(file_path)
                            .map_err(|e| format!("open {}: {}", file_path.display(), e))?;
                        let mut remote_file = handle
                            .sftp
                            .create(Path::new(&remote))
                            .map_err(|e| format!("create {}: {}", remote, e))?;
                        std::io::copy(&mut local_file, &mut remote_file)
                            .map_err(|e| format!("copy {}: {}", remote, e))?;
                    }

                    if total > 0 {
                        let pct = ((i + 1) * 100 / total) as u8;
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
                    log::error!("sftp upload_dir error: {}", e);
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
        log::info!("sftp download_dir: {} -> {}", remote_path, local_dest);
        let remote_path = remote_path.to_string();
        let local_dest = local_dest.to_string();

        let handle_ptr = &self.handle as *const Mutex<SftpHandle> as usize;
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let handle_mutex = unsafe { &*(handle_ptr as *const Mutex<SftpHandle>) };
            let result = (|| -> Result<(), String> {
                let handle = handle_mutex.lock().map_err(|e| e.to_string())?;
                sftp_download_recursive(&handle.sftp, &remote_path, &local_dest, &tx)
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
                    log::error!("sftp download_dir error: {}", e);
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

fn delete_dir_recursive(sftp: &ssh2::Sftp, path: &str) -> Result<(), String> {
    let entries = sftp
        .readdir(Path::new(path))
        .map_err(|e| format!("readdir {}: {}", path, e))?;
    for (entry_path, stat) in entries {
        let name = entry_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if name == "." || name == ".." {
            continue;
        }
        let full = format!("{}/{}", path.trim_end_matches('/'), name);
        if stat.is_dir() {
            delete_dir_recursive(sftp, &full)?;
        } else {
            sftp.unlink(Path::new(&full))
                .map_err(|e| format!("rm {}: {}", full, e))?;
        }
    }
    sftp.rmdir(Path::new(path))
        .map_err(|e| format!("rmdir {}: {}", path, e))
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

fn sftp_download_recursive(
    sftp: &ssh2::Sftp,
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

    let entries = sftp
        .readdir(Path::new(remote_path))
        .map_err(|e| format!("readdir {}: {}", remote_path, e))?;

    for (entry_path, stat) in entries {
        let name = entry_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if name == "." || name == ".." {
            continue;
        }

        let remote_full = format!("{}/{}", remote_path.trim_end_matches('/'), name);
        if stat.is_dir() {
            sftp_download_recursive(sftp, &remote_full, &local_dir, tx)?;
        } else {
            let local_file = format!("{}/{}", local_dir, name);
            log::debug!("sftp download_dir: {} -> {}", remote_full, local_file);
            let mut remote_file = sftp
                .open(Path::new(&remote_full))
                .map_err(|e| format!("open {}: {}", remote_full, e))?;
            let mut local = std::fs::File::create(&local_file)
                .map_err(|e| format!("create {}: {}", local_file, e))?;
            std::io::copy(&mut remote_file, &mut local)
                .map_err(|e| format!("copy {}: {}", name, e))?;

            let _ = tx.send(StreamLine {
                text: format!("downloaded {}", name),
                err: None,
                done: false,
            });
        }
    }

    Ok(())
}
