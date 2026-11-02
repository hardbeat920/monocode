//! What survives a restart: the record of a join this app made, so a later
//! run can still go back, and a log of every decision for diagnosing a
//! failure in the field.
use super::join::JoinRecord;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const RECORD_FILE: &str = "keep-awake-hotspot.json";
pub const LOG_FILE: &str = "keep-awake-hotspot.log";
/// The log rolls over to `.1` past this size.
const LOG_MAX: u64 = 256 * 1024;

/// Written to a temporary file and renamed, so a crash leaves the old record
/// or the new one, never half of one.
pub fn save(path: &Path, record: &JoinRecord) -> std::io::Result<()> {
    let json = serde_json::to_vec(record).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

pub fn load(path: &Path) -> Option<JoinRecord> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

pub fn clear(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// JSON lines, one per decision.
#[derive(Clone, Debug)]
pub struct Log {
    path: PathBuf,
}
impl Log {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn append(&self, lines: &[String]) {
        if lines.is_empty() {
            return;
        }
        if std::fs::metadata(&self.path).is_ok_and(|m| m.len() > LOG_MAX) {
            let _ = std::fs::rename(&self.path, self.path.with_extension("log.1"));
        }
        let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        else {
            return;
        };
        for line in lines {
            let _ = writeln!(file, "{line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_join_record_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(RECORD_FILE);
        assert_eq!(load(&path), None);
        let record = JoinRecord {
            ssid: "My iPhone".into(),
            ip: "172.20.10.3".parse().ok(),
            router: "172.20.10.1".parse().ok(),
            at: 1_700_000_000,
        };
        save(&path, &record).unwrap();
        assert_eq!(load(&path), Some(record));
        clear(&path);
        assert_eq!(load(&path), None);
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(load(&path), None);
    }

    #[test]
    fn log_rotates() {
        let dir = tempfile::tempdir().unwrap();
        let log = Log::new(dir.path().join(LOG_FILE));
        log.append(&["{\"event\":\"offline\"}".into()]);
        assert_eq!(
            std::fs::read_to_string(log.path()).unwrap(),
            "{\"event\":\"offline\"}\n"
        );
        let big = "x".repeat(LOG_MAX as usize + 1);
        log.append(&[big]);
        log.append(&["next".into()]);
        assert_eq!(std::fs::read_to_string(log.path()).unwrap(), "next\n");
        assert!(dir.path().join("keep-awake-hotspot.log.1").exists());
    }
}
