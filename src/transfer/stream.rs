use std::io::Read;
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

/// Byte accounting and `"N%"` throttling for one transfer, shared across all of its
/// files (so a directory transfer reports progress by bytes, not by file count).
///
/// This is the single place in the new code that encodes progress as text. The
/// receiving end (`App::monitor_transfer`) parses it back with a regex — a real
/// debt, but confining the encoding here means that the day `StreamLine` gains a
/// `percent: Option<u8>` field, exactly one function changes.
pub struct ByteProgress {
    total: u64,
    done: u64,
    last_pct: u8,
    tx: mpsc::Sender<StreamLine>,
}

impl ByteProgress {
    /// `total == 0` means "unknown size": no percentage is ever emitted, rather
    /// than a wrong one. `finish()` still reports completion.
    pub fn new(total: u64, tx: mpsc::Sender<StreamLine>) -> Self {
        ByteProgress {
            total,
            done: 0,
            last_pct: 0,
            tx,
        }
    }

    /// Late-binding of the total, for a download that learns its size from the
    /// `Content-Length` header only once the response headers are in.
    pub fn set_total(&mut self, total: u64) {
        self.total = total;
    }

    pub fn advance(&mut self, n: u64) {
        self.done += n;
        if self.total == 0 {
            return;
        }
        let pct = ((self.done.min(self.total) * 100) / self.total) as u8;
        if pct > self.last_pct {
            self.last_pct = pct;
            let _ = self.tx.send(StreamLine {
                text: format!("{pct}%"),
                err: None,
                done: false,
            });
        }
    }

    /// Un-counts `n` bytes, for a request whose body had to be replayed (a PUT
    /// rejected by an auth challenge). `last_pct` is deliberately NOT lowered: the
    /// bar must never travel backwards in front of the user.
    pub fn rewind(&mut self, n: u64) {
        self.done = self.done.saturating_sub(n);
    }

    pub fn finish(&mut self) {
        if self.last_pct < 100 {
            self.last_pct = 100;
            let _ = self.tx.send(StreamLine {
                text: "100%".to_string(),
                err: None,
                done: false,
            });
        }
    }
}

/// Generic `Read` adapter that reports what flows through it. It borrows the
/// accounting rather than sharing it, so no `Arc<Mutex<_>>` is needed: a transfer
/// runs on a single thread.
pub struct ProgressReader<'a, R: Read> {
    inner: R,
    progress: &'a mut ByteProgress,
}

impl<'a, R: Read> ProgressReader<'a, R> {
    pub fn new(inner: R, progress: &'a mut ByteProgress) -> Self {
        ProgressReader { inner, progress }
    }
}

impl<R: Read> Read for ProgressReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.progress.advance(n as u64);
        Ok(n)
    }
}

/// Runs `f` on a background thread and guarantees EXACTLY ONE terminating
/// `StreamLine { done: true }` — the contract `App::monitor_transfer` relies on,
/// since it returns at the first `done`.
///
/// `child_pid` is `None`: like the SFTP and FTP backends, there is no OS process to
/// signal, so such a transfer cannot be cancelled.
pub fn spawn_transfer<F>(f: F) -> StreamHandle
where
    F: FnOnce(&mpsc::Sender<StreamLine>) -> Result<(), String> + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let res = f(&tx);
        let _ = tx.send(StreamLine {
            text: String::new(),
            err: res.err(),
            done: true,
        });
    });
    StreamHandle {
        rx,
        child_pid: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(rx: &mpsc::Receiver<StreamLine>) -> Vec<String> {
        rx.try_iter().map(|l| l.text).collect()
    }

    #[test]
    fn emits_only_on_percent_change() {
        let (tx, rx) = mpsc::channel();
        let mut p = ByteProgress::new(100, tx);
        p.advance(1); // 1%
        p.advance(0); // still 1% -> silent
        p.advance(1); // 2%
        assert_eq!(drain(&rx), vec!["1%", "2%"]);
    }

    #[test]
    fn unknown_total_emits_nothing() {
        let (tx, rx) = mpsc::channel();
        let mut p = ByteProgress::new(0, tx);
        p.advance(4096);
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn advance_past_total_caps_at_100() {
        let (tx, rx) = mpsc::channel();
        let mut p = ByteProgress::new(10, tx);
        p.advance(999);
        assert_eq!(drain(&rx), vec!["100%"]);
    }

    #[test]
    fn finish_emits_100_once() {
        let (tx, rx) = mpsc::channel();
        let mut p = ByteProgress::new(10, tx);
        p.finish();
        p.finish();
        assert_eq!(drain(&rx), vec!["100%"]);
    }

    #[test]
    fn finish_is_silent_when_already_complete() {
        let (tx, rx) = mpsc::channel();
        let mut p = ByteProgress::new(10, tx);
        p.advance(10);
        p.finish();
        assert_eq!(drain(&rx), vec!["100%"]);
    }

    #[test]
    fn unknown_total_still_reports_completion() {
        let (tx, rx) = mpsc::channel();
        let mut p = ByteProgress::new(0, tx);
        p.advance(4096);
        p.finish();
        assert_eq!(drain(&rx), vec!["100%"]);
    }

    #[test]
    fn rewind_uncounts_without_moving_the_bar_backwards() {
        let (tx, rx) = mpsc::channel();
        let mut p = ByteProgress::new(100, tx);
        p.advance(50);
        assert_eq!(drain(&rx), vec!["50%"]);
        // A replayed PUT body: the bytes are un-counted, but the displayed
        // percentage must not travel backwards in front of the user.
        p.rewind(50);
        p.advance(50);
        assert!(drain(&rx).is_empty(), "the bar must not repeat or regress");
        p.advance(50);
        assert_eq!(drain(&rx), vec!["100%"]);
    }

    #[test]
    fn rewind_past_zero_saturates() {
        let (tx, _rx) = mpsc::channel();
        let mut p = ByteProgress::new(10, tx);
        p.advance(1);
        p.rewind(999);
    }

    #[test]
    fn progress_reader_counts_what_it_reads() {
        let (tx, rx) = mpsc::channel();
        let mut p = ByteProgress::new(4, tx);
        let data: &[u8] = b"abcd";
        let mut reader = ProgressReader::new(data, &mut p);
        // One byte at a time, so the throttling is actually exercised: a single
        // read_to_end would consume all four bytes in one call and emit only "100%".
        let mut out = Vec::new();
        let mut byte = [0u8; 1];
        while reader.read(&mut byte).unwrap() == 1 {
            out.push(byte[0]);
        }
        assert_eq!(out, b"abcd");
        assert_eq!(drain(&rx), vec!["25%", "50%", "75%", "100%"]);
    }

    #[test]
    fn progress_reader_passes_data_through_unchanged() {
        let (tx, rx) = mpsc::channel();
        let mut p = ByteProgress::new(0, tx);
        let data: &[u8] = b"hello world";
        let mut out = Vec::new();
        ProgressReader::new(data, &mut p)
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out, b"hello world");
        drop(rx);
    }

    #[test]
    fn spawn_transfer_sends_exactly_one_done() {
        let handle = spawn_transfer(|tx| {
            let _ = tx.send(StreamLine {
                text: "50%".to_string(),
                err: None,
                done: false,
            });
            Ok(())
        });
        let lines: Vec<_> = handle.rx.iter().collect();
        assert_eq!(lines.iter().filter(|l| l.done).count(), 1);
        assert!(lines.last().unwrap().done);
        assert!(lines.last().unwrap().err.is_none());
    }

    #[test]
    fn spawn_transfer_reports_the_error_on_the_done_line() {
        let handle = spawn_transfer(|_| Err("boom".to_string()));
        let lines: Vec<_> = handle.rx.iter().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].done);
        assert_eq!(lines[0].err.as_deref(), Some("boom"));
    }
}
