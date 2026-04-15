use std::io::Read as IoRead;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

/// StreamLine carries either a line of output or a final signal.
#[derive(Debug)]
pub struct StreamLine {
    pub text: String,
    pub err: Option<String>,
    pub done: bool,
}

/// StreamHandle holds a stream receiver and the child PID for kill support.
pub struct StreamHandle {
    pub rx: mpsc::Receiver<StreamLine>,
    pub child_pid: Option<u32>,
}

/// Kills a process by PID using SIGTERM.
pub fn kill_process(pid: u32) {
    let _ = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// RunResult holds the output of a synchronous command.
pub struct RunResult {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

/// Executor abstracts running SSH/SCP commands for testability.
pub trait Executor: Send + Sync {
    /// Run an SSH command on the remote host synchronously.
    fn ssh_run(&self, remote_cmd: &str) -> Result<RunResult, String>;
    /// Run an SCP command, returning a StreamHandle for progress monitoring.
    fn scp_stream(&self, args: &[&str]) -> Result<StreamHandle, String>;
    /// The ssh binary path.
    fn ssh_bin(&self) -> &str;
    /// The scp binary path.
    fn scp_bin(&self) -> &str;
    /// Connection target string (user@host).
    fn target(&self) -> String;
}

/// RealExecutor runs actual SSH/SCP commands via std::process::Command.
pub struct RealExecutor {
    ssh: String,
    scp: String,
    user: String,
    host: String,
    port: u16,
    identity_file: Option<String>,
}

impl RealExecutor {
    pub fn new(
        ssh_bin: &str,
        scp_bin: &str,
        user: &str,
        host: &str,
        port: u16,
        identity_file: Option<String>,
    ) -> Self {
        RealExecutor {
            ssh: ssh_bin.to_string(),
            scp: scp_bin.to_string(),
            user: user.to_string(),
            host: host.to_string(),
            port,
            identity_file,
        }
    }

    fn ssh_target(&self) -> String {
        if self.user.is_empty() {
            self.host.clone()
        } else {
            format!("{}@{}", self.user, self.host)
        }
    }

    fn control_path(&self) -> String {
        format!("/tmp/lt-ssh-{}@{}:{}", self.user, self.host, self.port)
    }

    fn ssh_base_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(),
            "ConnectTimeout=10".to_string(),
            "-o".to_string(),
            format!("ControlPath={}", self.control_path()),
            "-o".to_string(),
            "ControlMaster=auto".to_string(),
            "-o".to_string(),
            "ControlPersist=600".to_string(),
            "-p".to_string(),
            self.port.to_string(),
        ];
        if let Some(ref key) = self.identity_file {
            args.push("-i".to_string());
            args.push(key.clone());
        }
        args
    }

    fn scp_base_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(),
            format!("ControlPath={}", self.control_path()),
            "-o".to_string(),
            "ControlMaster=auto".to_string(),
            "-o".to_string(),
            "ControlPersist=600".to_string(),
            "-P".to_string(),
            self.port.to_string(),
        ];
        if let Some(ref key) = self.identity_file {
            args.push("-i".to_string());
            args.push(key.clone());
        }
        args
    }
}

impl Executor for RealExecutor {
    fn ssh_run(&self, remote_cmd: &str) -> Result<RunResult, String> {
        let mut args = self.ssh_base_args();
        args.push(self.ssh_target());
        args.push(remote_cmd.to_string());

        log::debug!("ssh_run: {} {}", self.ssh, args.join(" "));

        let output = Command::new(&self.ssh)
            .args(&args)
            .output()
            .map_err(|e| e.to_string())?;

        let exit_code = output.status.code().unwrap_or(-1);

        Ok(RunResult {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code,
        })
    }

    fn scp_stream(&self, args: &[&str]) -> Result<StreamHandle, String> {
        let mut full_args = self.scp_base_args();
        full_args.extend(args.iter().map(|s| s.to_string()));

        log::debug!("scp_stream: {} {}", self.scp, full_args.join(" "));

        // Use `script` to allocate a PTY so SCP flushes progress in real-time.
        let inner_cmd = std::iter::once(self.scp.as_str())
            .chain(full_args.iter().map(|s| s.as_str()))
            .map(shell_escape)
            .collect::<Vec<_>>()
            .join(" ");

        let mut child = Command::new("script")
            .args(["-qefc", &inner_cmd, "/dev/null"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;

        let child_pid = child.id();
        let stdout = child.stdout.take().ok_or("failed to capture stdout")?;

        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            // Read byte-by-byte, splitting on both \r and \n.
            // SCP uses \r to update progress on the same line, so
            // BufReader::lines() (which splits on \n only) would buffer
            // the entire transfer as one line.
            let mut reader = std::io::BufReader::new(stdout);
            let mut buf = Vec::with_capacity(512);
            let mut byte = [0u8; 1];

            loop {
                match reader.read(&mut byte) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        if byte[0] == b'\r' || byte[0] == b'\n' {
                            if !buf.is_empty() {
                                let text = String::from_utf8_lossy(&buf).to_string();
                                buf.clear();
                                let _ = tx.send(StreamLine {
                                    text,
                                    err: None,
                                    done: false,
                                });
                            }
                        } else {
                            buf.push(byte[0]);
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(StreamLine {
                            text: String::new(),
                            err: Some(e.to_string()),
                            done: true,
                        });
                        return;
                    }
                }
            }

            // Flush remaining buffer
            if !buf.is_empty() {
                let text = String::from_utf8_lossy(&buf).to_string();
                let _ = tx.send(StreamLine {
                    text,
                    err: None,
                    done: false,
                });
            }

            let status = child.wait();
            match status {
                Ok(s) if s.success() => {
                    let _ = tx.send(StreamLine {
                        text: String::new(),
                        err: None,
                        done: true,
                    });
                }
                Ok(s) => {
                    let _ = tx.send(StreamLine {
                        text: String::new(),
                        err: Some(format!("exit code {}", s.code().unwrap_or(-1))),
                        done: true,
                    });
                }
                Err(e) => {
                    let _ = tx.send(StreamLine {
                        text: String::new(),
                        err: Some(e.to_string()),
                        done: true,
                    });
                }
            }
        });

        Ok(StreamHandle {
            rx,
            child_pid: Some(child_pid),
        })
    }

    fn ssh_bin(&self) -> &str {
        &self.ssh
    }

    fn scp_bin(&self) -> &str {
        &self.scp
    }

    fn target(&self) -> String {
        self.ssh_target()
    }
}

/// Escapes a string for safe use in a shell command.
fn shell_escape(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_alphanumeric() || "-_./=:@^".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_escape_simple() {
        assert_eq!(shell_escape("hello"), "hello");
        assert_eq!(shell_escape("/path/to/file"), "/path/to/file");
    }

    #[test]
    fn shell_escape_special_chars() {
        assert_eq!(shell_escape("hello world"), "'hello world'");
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }

}
