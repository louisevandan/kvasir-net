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
use std::sync::Mutex;

static SINK: Mutex<Option<std::fs::File>> = Mutex::new(None);

/// Appends one record. A line is written in a single call so it cannot be
/// interleaved with the next one.
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
            eprintln!("{line}");
            return;
        }
    };
    if sink.is_none() {
        *sink = OpenOptions::new().create(true).append(true).open(&path).ok();
    }
    match sink.as_mut() {
        Some(file) => {
            let _ = file.write_all(format!("{line}\n").as_bytes());
            let _ = file.flush();
        }
        None => eprintln!("{line}"),
    }
}

#[cfg(test)]
mod tests {
    use super::record;

    #[test]
    fn a_record_reaches_the_named_file_whole() {
        let dir = std::env::temp_dir().join(format!("p4-record-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("record.log");
        let _ = std::fs::remove_file(&path);
        // SAFETY: the sink is opened lazily and this test owns the variable
        // for its duration; the suite runs these serially by file.
        unsafe { std::env::set_var("P4_RECORD_FILE", &path) };
        record("P4_TEST_RECORD one");
        record("P4_TEST_RECORD two");
        let text = std::fs::read_to_string(&path).expect("read");
        assert_eq!(
            text.lines().collect::<Vec<_>>(),
            vec!["P4_TEST_RECORD one", "P4_TEST_RECORD two"]
        );
        unsafe { std::env::remove_var("P4_RECORD_FILE") };
        std::fs::remove_dir_all(&dir).ok();
    }
}
