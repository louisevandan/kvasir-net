//! Where a record that will be read back as evidence gets written.
//!
//! Not stderr. The agent inherits its stage servers' stderr so a load failure
//! is not swallowed, which means four unsynchronised writers share that file
//! and a record can be torn in half by one of them. That is survivable for a
//! log and fatal for evidence: a 2026-09-01 four-node run reported one of
//! forty session keys missing, and the key was there - a stage server had
//! written into the middle of the line.
//!
//! `P4_RECORD_FILE` names a file this process appends whole lines to. Nothing
//! else writes there. Without it the record still goes to stderr, because a
//! developer watching a terminal is the other reader this serves.

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// The open record file, with the path it was opened for. Keyed by path so
/// the sink follows the variable rather than caching the first value it ever
/// saw - which in production never changes, and in a test suite does.
static SINK: Mutex<Option<(std::ffi::OsString, std::fs::File)>> = Mutex::new(None);
/// Set once any record fails to reach the file, and never cleared: a channel
/// that dropped one record cannot be trusted for the rest of the run.
static FAILED: AtomicBool = AtomicBool::new(false);

/// Appends one record. A line is written in a single call so it cannot be
/// interleaved with the next one.
/// Appends one record. A line is written in a single call so it cannot be
/// interleaved with the next one.
///
/// A failure to write is itself recorded - into the file when the file is
/// what failed, this is impossible, so onto stderr and into a flag the
/// harness reads. A silently dropped record makes `delivery=0` mean "the
/// channel is dead" and "nothing was lost" at once.
pub fn record(line: &str) {
    let Some(path) = std::env::var_os("P4_RECORD_FILE") else {
        eprintln!("{line}");
        return;
    };
    let mut sink = match SINK.lock() {
        Ok(sink) => sink,
        // A poisoned sink means another thread panicked mid-record; the
        // terminal is still a place to say this.
        Err(_) => {
            mark_failed(&path, "poisoned");
            eprintln!("P4_RECORD_CHANNEL_FAILED reason=poisoned line={line}");
            return;
        }
    };
    if sink.as_ref().is_none_or(|(opened, _)| opened != &path) {
        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => *sink = Some((path.clone(), file)),
            Err(error) => {
                mark_failed(&path, &format!("open: {error}"));
                eprintln!("P4_RECORD_CHANNEL_FAILED reason=open error={error}");
                eprintln!("{line}");
                return;
            }
        }
    }
    let (_, file) = sink.as_mut().expect("sink is open");
    if let Err(error) = file
        .write_all(format!("{line}
").as_bytes())
        .and_then(|()| file.flush())
    {
        mark_failed(&path, &format!("write: {error}"));
        eprintln!("P4_RECORD_CHANNEL_FAILED reason=write error={error}");
        eprintln!("{line}");
    }
}

/// Whether any record failed to reach the file.
///
/// Also written to `<record file>.failed` the first time it happens, because
/// a caller in another process cannot read this flag and the alternative -
/// announcing it on stderr - puts the announcement on the channel four stage
/// servers share, which is the one that tore a record in half in the first
/// place. A separate file is an independent path: the record file being
/// locked, or opened by something else, does not stop it.
pub fn channel_failed() -> bool {
    FAILED.load(Ordering::Relaxed)
}

/// Marks the failure where a reader in another process can find it.
/// Idempotent: the marker is written every time rather than only on the
/// first failure, so a reader finds it regardless of what else has failed in
/// this process.
fn mark_failed(path: &std::ffi::OsStr, reason: &str) {
    FAILED.store(true, Ordering::Relaxed);
    let mut marker = std::path::PathBuf::from(path);
    let name = marker
        .file_name()
        .map(|value| format!("{}.failed", value.to_string_lossy()))
        .unwrap_or_else(|| "record.failed".to_owned());
    marker.set_file_name(name);
    let _ = std::fs::write(&marker, format!("{reason}
"));
}

#[cfg(test)]
mod tests {
    use super::record;

    /// One test, because `P4_RECORD_FILE` is process-global and the harness
    /// runs tests in threads: two tests pointing it at different files race
    /// each other rather than testing anything.
    #[test]
    fn records_reach_the_named_file_and_a_failure_leaves_a_marker() {
        let dir = std::env::temp_dir().join(format!("p4-record-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");

        let path = dir.join("record.log");
        let _ = std::fs::remove_file(&path);
        // SAFETY: this test owns the variable, and it is the only test that
        // touches it.
        unsafe { std::env::set_var("P4_RECORD_FILE", &path) };
        record("P4_TEST_RECORD one");
        record("P4_TEST_RECORD two");
        let text = std::fs::read_to_string(&path).expect("read");
        assert_eq!(
            text.lines().collect::<Vec<_>>(),
            vec!["P4_TEST_RECORD one", "P4_TEST_RECORD two"]
        );

        // A directory cannot be opened for append, so this is a real failure.
        // The reader is another process: the flag alone is invisible to it,
        // and stderr is the channel four stage servers share.
        let blocked = dir.join("blocked.log");
        std::fs::create_dir_all(&blocked).expect("directory in the file's place");
        let marker = dir.join("blocked.log.failed");
        let _ = std::fs::remove_file(&marker);
        unsafe { std::env::set_var("P4_RECORD_FILE", &blocked) };
        record("P4_TEST_RECORD blocked");
        assert!(marker.exists(), "a failed open must leave a marker");
        assert!(super::channel_failed());

        unsafe { std::env::remove_var("P4_RECORD_FILE") };
        std::fs::remove_dir_all(&dir).ok();
    }
}