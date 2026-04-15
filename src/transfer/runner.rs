use std::sync::Arc;

use crate::transfer::backend::RemoteBackend;
use crate::transfer::exec::{Executor, StreamHandle};
use crate::transfer::types::FileEntry;

/// SshRunner implements RemoteBackend using SSH/SCP commands.
pub struct SshRunner {
    exec: Arc<dyn Executor>,
}

impl SshRunner {
    pub fn new(exec: Arc<dyn Executor>) -> Self {
        SshRunner { exec }
    }

    pub fn executor(&self) -> &Arc<dyn Executor> {
        &self.exec
    }
}

impl RemoteBackend for SshRunner {
    fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>, String> {
        let cmd = format!("ls -la --time-style=long-iso {} 2>/dev/null || ls -la {}", path, path);
        let result = self.exec.ssh_run(&cmd)?;

        if result.exit_code != 0 {
            let stderr = String::from_utf8_lossy(&result.stderr);
            return Err(format!("ls failed: {}", stderr.trim()));
        }

        let stdout = String::from_utf8_lossy(&result.stdout);
        Ok(parse_ls_output(&stdout))
    }

    fn mkdir(&self, path: &str) -> Result<(), String> {
        let cmd = format!("mkdir -p '{}'", path.replace('\'', "'\\''"));
        let result = self.exec.ssh_run(&cmd)?;
        check_exit(&result)
    }

    fn delete(&self, path: &str) -> Result<(), String> {
        let cmd = format!("rm -rf '{}'", path.replace('\'', "'\\''"));
        let result = self.exec.ssh_run(&cmd)?;
        check_exit(&result)
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        let cmd = format!(
            "mv '{}' '{}'",
            from.replace('\'', "'\\''"),
            to.replace('\'', "'\\''")
        );
        let result = self.exec.ssh_run(&cmd)?;
        check_exit(&result)
    }

    fn home_dir(&self) -> Result<String, String> {
        let result = self.exec.ssh_run("echo $HOME")?;
        if result.exit_code != 0 {
            return Err("failed to get home directory".to_string());
        }
        let stdout = String::from_utf8_lossy(&result.stdout);
        Ok(stdout.trim().to_string())
    }

    fn test_connection(&self) -> Result<String, String> {
        let result = self.exec.ssh_run("echo ok && echo $HOME")?;
        if result.exit_code != 0 {
            let stderr = String::from_utf8_lossy(&result.stderr);
            return Err(format!("connection failed: {}", stderr.trim()));
        }
        let stdout = String::from_utf8_lossy(&result.stdout);
        let lines: Vec<&str> = stdout.trim().lines().collect();
        if lines.len() >= 2 && lines[0] == "ok" {
            Ok(lines[1].to_string())
        } else {
            Err("unexpected response from server".to_string())
        }
    }

    fn upload(&self, local_path: &str, remote_path: &str) -> Result<StreamHandle, String> {
        let target = format!("{}:{}", self.exec.target(), remote_path);
        self.exec.scp_stream(&[local_path, &target])
    }

    fn download(&self, remote_path: &str, local_path: &str) -> Result<StreamHandle, String> {
        let source = format!("{}:{}", self.exec.target(), remote_path);
        self.exec.scp_stream(&[&source, local_path])
    }

    fn upload_dir(&self, local_path: &str, remote_dest: &str) -> Result<StreamHandle, String> {
        let path = std::path::Path::new(local_path);
        let dir_name = path
            .file_name()
            .ok_or("invalid directory path")?
            .to_string_lossy()
            .to_string();
        let parent_dir = path
            .parent()
            .ok_or("invalid parent directory")?
            .to_string_lossy()
            .to_string();

        let tmp_name = format!("lt-{}.tar.gz", gen_tmp_id());
        let local_tar = format!("/tmp/{}", tmp_name);
        let remote_tar = format!("/tmp/{}", tmp_name);

        // Step 1: tar locally
        log::info!("upload_dir: tar czf {} -C {} {}", local_tar, parent_dir, dir_name);
        let tar_result = std::process::Command::new("tar")
            .args(["czf", &local_tar, "-C", &parent_dir, &dir_name])
            .output()
            .map_err(|e| format!("tar failed: {}", e))?;
        if !tar_result.status.success() {
            let _ = std::fs::remove_file(&local_tar);
            return Err(format!(
                "tar failed: {}",
                String::from_utf8_lossy(&tar_result.stderr)
            ));
        }

        // Step 2: scp the archive (returns StreamHandle for progress)
        let target = format!("{}:{}", self.exec.target(), remote_tar);
        let handle = self.exec.scp_stream(&[&local_tar, &target])?;

        // Step 3+4: extract + cleanup will happen after the stream completes
        // We wrap the handle to do post-processing
        let exec = Arc::clone(&self.exec);
        let (tx, rx) = std::sync::mpsc::channel();
        let child_pid = handle.child_pid;

        let remote_dest = remote_dest.to_string();
        let local_tar_cleanup = local_tar.clone();
        std::thread::spawn(move || {
            // Forward all stream lines
            while let Ok(line) = handle.rx.recv() {
                let done = line.done;
                let err = line.err.clone();
                let _ = tx.send(line);
                if done {
                    if err.is_some() {
                        // Cleanup on error
                        let _ = std::fs::remove_file(&local_tar_cleanup);
                        let _ = exec.ssh_run(&format!("rm -f '{}'", remote_tar.replace('\'', "'\\''")));
                        return;
                    }
                    break;
                }
            }

            // Step 3: extract remotely
            log::info!("upload_dir: extracting {} to {}", remote_tar, remote_dest);
            let extract_cmd = format!(
                "tar xzf '{}' -C '{}' && rm -f '{}'",
                remote_tar.replace('\'', "'\\''"),
                remote_dest.replace('\'', "'\\''"),
                remote_tar.replace('\'', "'\\''"),
            );
            if let Err(e) = exec.ssh_run(&extract_cmd).and_then(|r| check_exit(&r)) {
                let _ = tx.send(crate::transfer::exec::StreamLine {
                    text: String::new(),
                    err: Some(format!("remote extract failed: {}", e)),
                    done: true,
                });
                let _ = std::fs::remove_file(&local_tar_cleanup);
                return;
            }

            // Step 4: cleanup local tar
            let _ = std::fs::remove_file(&local_tar_cleanup);

            let _ = tx.send(crate::transfer::exec::StreamLine {
                text: String::new(),
                err: None,
                done: true,
            });
        });

        Ok(StreamHandle { rx, child_pid })
    }

    fn download_dir(&self, remote_path: &str, local_dest: &str) -> Result<StreamHandle, String> {
        let dir_name = remote_path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .ok_or("invalid remote path")?
            .to_string();
        let parent_dir = {
            let trimmed = remote_path.trim_end_matches('/');
            match trimmed.rfind('/') {
                Some(0) => "/".to_string(),
                Some(pos) => trimmed[..pos].to_string(),
                None => return Err("invalid remote path".to_string()),
            }
        };

        let tmp_name = format!("lt-{}.tar.gz", gen_tmp_id());
        let remote_tar = format!("/tmp/{}", tmp_name);
        let local_tar = format!("/tmp/{}", tmp_name);

        // Step 1: tar remotely
        log::info!("download_dir: remote tar czf {} -C {} {}", remote_tar, parent_dir, dir_name);
        let tar_cmd = format!(
            "tar czf '{}' -C '{}' '{}'",
            remote_tar.replace('\'', "'\\''"),
            parent_dir.replace('\'', "'\\''"),
            dir_name.replace('\'', "'\\''"),
        );
        let tar_result = self.exec.ssh_run(&tar_cmd)?;
        check_exit(&tar_result).map_err(|e| format!("remote tar failed: {}", e))?;

        // Step 2: scp the archive (returns StreamHandle for progress)
        let source = format!("{}:{}", self.exec.target(), remote_tar);
        let handle = self.exec.scp_stream(&[&source, &local_tar])?;

        // Step 3+4+5: extract locally + cleanup
        let exec = Arc::clone(&self.exec);
        let (tx, rx) = std::sync::mpsc::channel();
        let child_pid = handle.child_pid;

        let local_dest = local_dest.to_string();
        let local_tar_cleanup = local_tar.clone();
        let remote_tar_cleanup = remote_tar.clone();
        std::thread::spawn(move || {
            // Forward all stream lines
            while let Ok(line) = handle.rx.recv() {
                let done = line.done;
                let err = line.err.clone();
                let _ = tx.send(line);
                if done {
                    if err.is_some() {
                        let _ = std::fs::remove_file(&local_tar_cleanup);
                        let _ = exec.ssh_run(&format!("rm -f '{}'", remote_tar_cleanup.replace('\'', "'\\''")));
                        return;
                    }
                    break;
                }
            }

            // Step 3: extract locally
            log::info!("download_dir: extracting {} to {}", local_tar_cleanup, local_dest);
            let extract = std::process::Command::new("tar")
                .args(["xzf", &local_tar_cleanup, "-C", &local_dest])
                .output();
            match extract {
                Ok(out) if out.status.success() => {}
                Ok(out) => {
                    let _ = tx.send(crate::transfer::exec::StreamLine {
                        text: String::new(),
                        err: Some(format!(
                            "local extract failed: {}",
                            String::from_utf8_lossy(&out.stderr)
                        )),
                        done: true,
                    });
                    let _ = std::fs::remove_file(&local_tar_cleanup);
                    let _ = exec.ssh_run(&format!("rm -f '{}'", remote_tar_cleanup.replace('\'', "'\\''")));
                    return;
                }
                Err(e) => {
                    let _ = tx.send(crate::transfer::exec::StreamLine {
                        text: String::new(),
                        err: Some(format!("local extract failed: {}", e)),
                        done: true,
                    });
                    let _ = std::fs::remove_file(&local_tar_cleanup);
                    let _ = exec.ssh_run(&format!("rm -f '{}'", remote_tar_cleanup.replace('\'', "'\\''")));
                    return;
                }
            }

            // Step 4: cleanup local tar
            let _ = std::fs::remove_file(&local_tar_cleanup);

            // Step 5: cleanup remote tar
            let _ = exec.ssh_run(&format!("rm -f '{}'", remote_tar_cleanup.replace('\'', "'\\''")));

            let _ = tx.send(crate::transfer::exec::StreamLine {
                text: String::new(),
                err: None,
                done: true,
            });
        });

        Ok(StreamHandle { rx, child_pid })
    }
}

/// Generate a simple unique ID for temp files using timestamp nanos.
fn gen_tmp_id() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn check_exit(result: &crate::transfer::exec::RunResult) -> Result<(), String> {
    if result.exit_code != 0 {
        let stderr = String::from_utf8_lossy(&result.stderr);
        Err(format!(
            "command failed (exit {}): {}",
            result.exit_code,
            stderr.trim()
        ))
    } else {
        Ok(())
    }
}

/// Parse `ls -la` output into FileEntry items.
/// Expects lines like: `drwxr-xr-x 2 user group 4096 2024-01-15 10:30 Documents`
/// or standard ls: `drwxr-xr-x 2 user group 4096 Jan 15 10:30 Documents`
fn parse_ls_output(output: &str) -> Vec<FileEntry> {
    let mut entries = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();

        // Skip "total" line and empty lines
        if trimmed.is_empty() || trimmed.starts_with("total ") {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 8 {
            continue;
        }

        let permissions = parts[0].to_string();
        let is_dir = permissions.starts_with('d');

        // Try to detect if using --time-style=long-iso (date field is YYYY-MM-DD)
        // long-iso: permissions links user group size YYYY-MM-DD HH:MM name...  (8+ parts)
        // standard: permissions links user group size Mon DD HH:MM name...       (9+ parts)
        let (size, modified, name_start) = if parts.len() >= 8 && parts[5].contains('-') && parts[5].len() == 10 {
            let size = parts[4].parse::<u64>().unwrap_or(0);
            let modified = format!("{} {}", parts[5], parts[6]);
            (size, modified, 7)
        } else if parts.len() >= 9 {
            let size = parts[4].parse::<u64>().unwrap_or(0);
            let modified = format!("{} {} {}", parts[5], parts[6], parts[7]);
            (size, modified, 8)
        } else {
            continue;
        };

        // Name is everything from name_start onward (handles spaces in filenames)
        let name = parts[name_start..].join(" ");

        // Skip . entry but keep ..
        if name == "." {
            continue;
        }

        // Handle symlinks: "name -> target" — keep just the name
        let name = if let Some(arrow_pos) = name.find(" -> ") {
            name[..arrow_pos].to_string()
        } else {
            name
        };

        entries.push(FileEntry {
            name,
            is_dir,
            size,
            modified,
            permissions,
        });
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ls_long_iso() {
        let output = "total 48\ndrwxr-xr-x  5 user group  4096 2024-01-15 10:30 Documents\n-rw-r--r--  1 user group 12345 2024-01-14 09:15 file.txt\nlrwxrwxrwx  1 user group    11 2024-01-13 08:00 link -> target\ndrwxr-xr-x  2 user group  4096 2024-01-12 07:00 .\ndrwxr-xr-x  3 user group  4096 2024-01-11 06:00 ..\n";
        let entries = parse_ls_output(output);
        assert_eq!(entries.len(), 4); // Documents, file.txt, link, ..

        assert_eq!(entries[0].name, "Documents");
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].size, 4096);

        assert_eq!(entries[1].name, "file.txt");
        assert!(!entries[1].is_dir);
        assert_eq!(entries[1].size, 12345);

        assert_eq!(entries[2].name, "link");

        assert_eq!(entries[3].name, "..");
        assert!(entries[3].is_dir);
    }

    #[test]
    fn parse_ls_standard_format() {
        let output = "total 8\ndrwxr-xr-x 2 user group 4096 Jan 15 10:30 backups\n-rw-r--r-- 1 user group  420 Jan 14 09:15 config.yml\n";
        let entries = parse_ls_output(output);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "backups");
        assert_eq!(entries[1].name, "config.yml");
    }

    #[test]
    fn parse_ls_empty() {
        let entries = parse_ls_output("total 0\n");
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_ls_filename_with_spaces() {
        let output = "total 4\n-rw-r--r-- 1 user group 100 2024-01-15 10:30 my file name.txt\n";
        let entries = parse_ls_output(output);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "my file name.txt");
    }
}
