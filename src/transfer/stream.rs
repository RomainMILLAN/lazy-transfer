use std::process::{Command, Stdio};
use std::sync::mpsc;

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
