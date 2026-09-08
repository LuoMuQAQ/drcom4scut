//! Read only new complete lines belonging to this core launch, with bounded I/O.
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    os::windows::fs::MetadataExt,
    path::Path,
};

const LIMIT: u64 = 64 * 1024;
#[derive(Default)]
pub struct LogTail {
    created: Option<u64>,
    offset: u64,
    anchor: Vec<u8>,
    pending: Vec<u8>,
    discard_partial: bool,
}
impl LogTail {
    /// Capture the boundary before spawning. Historical success cannot belong to
    /// the new process, even when the logger appends to the previous latest.log.
    pub fn before_launch(path: &Path) -> Self {
        let mut tail = Self::default();
        if let Ok(mut file) = File::open(path) {
            if let Ok(meta) = file.metadata() {
                tail.created = Some(meta.creation_time());
                tail.offset = meta.len();
                if tail.remember_anchor(&mut file).is_ok() {
                    tail.discard_partial = tail.anchor.last().is_some_and(|b| *b != b'\n');
                }
            }
        }
        tail
    }
    fn remember_anchor(&mut self, file: &mut File) -> io::Result<()> {
        let size = self.offset.min(64) as usize;
        file.seek(SeekFrom::Start(self.offset - size as u64))?;
        self.anchor.resize(size, 0);
        file.read_exact(&mut self.anchor)
    }
    pub fn poll(&mut self, path: &Path) -> io::Result<Vec<String>> {
        let mut file = File::open(path)?;
        let meta = file.metadata()?;
        let mut reset = self.created != Some(meta.creation_time()) || meta.len() < self.offset;
        if !reset && !self.anchor.is_empty() {
            file.seek(SeekFrom::Start(self.offset - self.anchor.len() as u64))?;
            let mut anchor = vec![0; self.anchor.len()];
            file.read_exact(&mut anchor)?;
            reset = anchor != self.anchor;
        }
        if reset {
            self.offset = 0;
            self.pending.clear();
            self.discard_partial = false;
        }
        self.created = Some(meta.creation_time());
        if meta.len().saturating_sub(self.offset) > LIMIT {
            self.offset = meta.len() - LIMIT;
            self.pending.clear();
            self.discard_partial = true;
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(meta.len().saturating_sub(self.offset))
            .read_to_end(&mut bytes)?;
        self.offset += bytes.len() as u64;
        self.remember_anchor(&mut file)?;
        self.pending.extend_from_slice(&bytes);
        let Some(end) = self.pending.iter().rposition(|b| *b == b'\n') else {
            if self.pending.len() > LIMIT as usize {
                self.pending.clear();
                self.discard_partial = true;
            }
            return Ok(Vec::new());
        };
        let complete: Vec<u8> = self.pending.drain(..=end).collect();
        let text = String::from_utf8_lossy(&complete);
        let mut lines = text.lines();
        if self.discard_partial {
            lines.next();
            self.discard_partial = false;
        }
        Ok(lines.map(str::to_owned).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "drcom-tail-{}.log",
                crate::install::identity::new_install_id()
            )))
        }
        fn write(&self, s: &[u8]) {
            std::fs::write(&self.0, s).unwrap();
        }
        fn append(&self, s: &[u8]) {
            std::fs::OpenOptions::new()
                .append(true)
                .open(&self.0)
                .unwrap()
                .write_all(s)
                .unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    #[test]
    fn ignores_previous_session_and_never_replays_unchanged_health() {
        let f = Fixture::new();
        f.write(b"Authorization success\n");
        let mut tail = LogTail::before_launch(&f.0);
        assert!(tail.poll(&f.0).unwrap().is_empty());
        f.append(b"Heartbeat done\n");
        assert_eq!(tail.poll(&f.0).unwrap(), ["Heartbeat done"]);
        for _ in 0..100 {
            assert!(tail.poll(&f.0).unwrap().is_empty());
        }
    }
    #[test]
    fn preserves_partial_utf8_lines_and_discards_old_partial_line() {
        let f = Fixture::new();
        f.write(b"old Heartbeat ");
        let mut tail = LogTail::before_launch(&f.0);
        let mut first = b"done\nnew ".to_vec();
        first.push(0xe6);
        f.append(&first);
        assert!(tail.poll(&f.0).unwrap().is_empty());
        f.append(b"\xb6\x88\xe6\x81\xaf\n");
        assert_eq!(tail.poll(&f.0).unwrap(), ["new 消息"]);
    }
    #[test]
    fn follows_truncation_and_rewrite_past_previous_offset() {
        let f = Fixture::new();
        f.write(b"old line\n");
        let mut tail = LogTail::before_launch(&f.0);
        f.write(b"new session Heartbeat done\n");
        assert_eq!(tail.poll(&f.0).unwrap(), ["new session Heartbeat done"]);
        f.write(b"new\n");
        assert_eq!(tail.poll(&f.0).unwrap(), ["new"]);
    }
    #[test]
    fn reads_new_file_and_bounds_large_backlog() {
        let f = Fixture::new();
        let mut tail = LogTail::before_launch(&f.0);
        assert!(tail.poll(&f.0).is_err());
        f.write(&vec![b'x'; 200_000]);
        f.append(b"\nHeartbeat done\n");
        assert_eq!(tail.poll(&f.0).unwrap(), ["Heartbeat done"]);
        assert!(tail.pending.len() <= LIMIT as usize);
    }

    #[test]
    fn incremental_log_and_monitor_keep_a_healthy_session_alive() {
        use crate::{health::HealthMonitor, model::LinkState};
        use std::time::{Duration, Instant};
        let f = Fixture::new();
        f.write(b"old Authorization success\n");
        let mut tail = LogTail::before_launch(&f.0);
        let t = Instant::now();
        let mut monitor = HealthMonitor::new(t);
        assert_eq!(
            monitor.observe(t, Some(&tail.poll(&f.0).unwrap())).state,
            LinkState::Connecting
        );
        for seconds in (2..=900).step_by(2) {
            if seconds == 2 || seconds % 12 == 0 {
                f.append(b"Heartbeat done\n");
            }
            let new = tail.poll(&f.0).unwrap();
            let decision = monitor.observe(t + Duration::from_secs(seconds), Some(&new));
            assert_eq!(decision.state, LinkState::Online);
            assert!(!decision.restart_stalled);
        }
        for seconds in (902..=1082).step_by(2) {
            let new = tail.poll(&f.0).unwrap();
            let decision = monitor.observe(t + Duration::from_secs(seconds), Some(&new));
            assert_eq!(decision.restart_stalled, seconds > 1080);
        }
    }
}
