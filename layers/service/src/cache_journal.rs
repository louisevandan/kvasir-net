//! Durable coordinator snapshots for multi-stage cache transactions.
//!
//! The journal stores coordinator intent and barrier progress only. Adapter KV
//! bytes, manifests, and rollback data remain owned by the adapter. Records
//! are append-only and checksummed. A final unterminated record is accepted
//! only when it is itself a valid record; malformed data is never silently
//! discarded during recovery.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

const PREFIX: &str = "p4-cache-coordinator-v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalState {
    pub operation_id: String,
    pub sequence: String,
    pub deployment: String,
    pub generation: u64,
    pub kind: String,
    pub state: String,
    pub stages: Vec<String>,
    pub prepared: Vec<String>,
    pub committed: Vec<String>,
    pub aborted: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct CacheJournal {
    path: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl PartialEq for CacheJournal {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Eq for CacheJournal {}

static JOURNAL_LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();

impl CacheJournal {
    pub fn open(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let lock = JOURNAL_LOCKS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .expect("cache journal lock registry")
            .entry(path.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        Ok(Self { path, lock })
    }

    pub fn append(&self, state: &JournalState) -> io::Result<()> {
        let _guard = self.lock.lock().expect("cache journal lock");
        let body = encode(state);
        let checksum = checksum(&body);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(body.as_bytes())?;
        file.write_all(b"|")?;
        file.write_all(checksum.to_string().as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()
    }

    pub fn recover(&self) -> io::Result<Option<JournalState>> {
        let _guard = self.lock.lock().expect("cache journal lock");
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        let mut latest = None;
        for (index, line) in text.split('\n').enumerate() {
            if line.is_empty() {
                continue;
            }
            match decode(line) {
                Ok(state) => latest = Some(state),
                Err(error) => {
                    let suffix = if index + 1 == text.lines().count() && !text.ends_with('\n') {
                        " in unterminated final record"
                    } else {
                        ""
                    };
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{error}{suffix}"),
                    ));
                }
            }
        }
        Ok(latest)
    }

    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn encode(state: &JournalState) -> String {
    [
        PREFIX.to_owned(),
        hex(&state.operation_id),
        hex(&state.sequence),
        hex(&state.deployment),
        state.generation.to_string(),
        hex(&state.kind),
        hex(&state.state),
        list(&state.stages),
        list(&state.prepared),
        list(&state.committed),
        list(&state.aborted),
    ]
    .join("|")
}

fn decode(line: &str) -> Result<JournalState, String> {
    let (body, supplied) = line
        .rsplit_once('|')
        .ok_or("journal record has no checksum")?;
    let expected = supplied
        .parse::<u64>()
        .map_err(|_| "journal checksum is not numeric")?;
    if checksum(body) != expected {
        return Err("journal checksum mismatch".into());
    }
    let mut fields = body.split('|');
    if fields.next() != Some(PREFIX) {
        return Err("unsupported coordinator journal version".into());
    }
    let operation_id = unhex(fields.next().ok_or("missing operation id")?)?;
    let sequence = unhex(fields.next().ok_or("missing sequence")?)?;
    let deployment = unhex(fields.next().ok_or("missing deployment")?)?;
    let generation = fields
        .next()
        .ok_or("missing generation")?
        .parse()
        .map_err(|_| "invalid generation")?;
    let kind = unhex(fields.next().ok_or("missing kind")?)?;
    let state = unhex(fields.next().ok_or("missing state")?)?;
    let stages = decode_list(fields.next().ok_or("missing stages")?)?;
    let prepared = decode_list(fields.next().ok_or("missing prepared set")?)?;
    let committed = decode_list(fields.next().ok_or("missing committed set")?)?;
    let aborted = decode_list(fields.next().ok_or("missing aborted set")?)?;
    if fields.next().is_some() {
        return Err("trailing coordinator journal fields".into());
    }
    let record = JournalState {
        operation_id,
        sequence,
        deployment,
        generation,
        kind,
        state,
        stages,
        prepared,
        committed,
        aborted,
    };
    validate_record(&record)?;
    Ok(record)
}

fn validate_record(record: &JournalState) -> Result<(), String> {
    let stages = record
        .stages
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    for (name, values) in [
        ("prepared", &record.prepared),
        ("committed", &record.committed),
        ("aborted", &record.aborted),
    ] {
        if values.iter().any(|stage| !stages.contains(stage)) {
            return Err(format!("{name} set contains unknown coordinator stage"));
        }
    }
    let committed = record
        .committed
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    let aborted = record
        .aborted
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    if committed
        .iter()
        .any(|stage| !record.prepared.iter().any(|item| item == *stage))
    {
        return Err("committed stage is not prepared".into());
    }
    if committed.iter().any(|stage| aborted.contains(stage)) {
        return Err("stage is both committed and aborted".into());
    }
    let prepared = record
        .prepared
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    match record.state.as_str() {
        "preparing" if !record.committed.is_empty() || !record.aborted.is_empty() => {
            Err("preparing journal record has terminal stage state".into())
        }
        "committing" if prepared != stages => {
            Err("committing journal record does not prepare every stage".into())
        }
        "complete" if committed != stages || !record.aborted.is_empty() => {
            Err("complete journal record does not commit every stage".into())
        }
        "aborting" if record.prepared.is_empty() => {
            Err("aborting journal record has no prepared stage".into())
        }
        "preparing" | "committing" | "aborting" | "complete" | "failed" => Ok(()),
        _ => Err("unknown coordinator journal state".into()),
    }
}

fn list(values: &[String]) -> String {
    values
        .iter()
        .map(|value| hex(value))
        .collect::<Vec<_>>()
        .join(",")
}

fn decode_list(value: &str) -> Result<Vec<String>, String> {
    if value.is_empty() {
        return Ok(Vec::new());
    }
    let values = value.split(',').map(unhex).collect::<Result<Vec<_>, _>>()?;
    let unique = values.iter().collect::<std::collections::BTreeSet<_>>();
    if unique.len() != values.len() {
        return Err("duplicate stage in coordinator journal set".into());
    }
    Ok(values)
}

fn hex(value: &str) -> String {
    value.bytes().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(value: &str) -> Result<String, String> {
    if !value.len().is_multiple_of(2) {
        return Err("odd hexadecimal journal field".into());
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| "invalid hexadecimal journal field")
        })
        .collect::<Result<Vec<_>, _>>()?;
    String::from_utf8(bytes).map_err(|_| "journal field is not UTF-8".into())
}

fn checksum(value: &str) -> u64 {
    value.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(status: &str) -> JournalState {
        let complete = status == "complete";
        JournalState {
            operation_id: "op".into(),
            sequence: "seq".into(),
            deployment: "deployment".into(),
            generation: 7,
            kind: "persist".into(),
            state: status.into(),
            stages: vec!["stage-0".into(), "stage-1".into()],
            prepared: if complete {
                vec!["stage-0".into(), "stage-1".into()]
            } else {
                vec!["stage-0".into()]
            },
            committed: if complete {
                vec!["stage-0".into(), "stage-1".into()]
            } else {
                Vec::new()
            },
            aborted: Vec::new(),
        }
    }

    fn test_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("p4-cache-journal-{name}-{}", std::process::id()))
    }

    #[test]
    fn recovery_returns_latest_valid_record_after_append() {
        let path = test_path("latest");
        let journal = CacheJournal::open(&path).unwrap();
        journal.append(&state("preparing")).unwrap();
        journal.append(&state("complete")).unwrap();

        assert_eq!(journal.recover().unwrap().unwrap().state, "complete");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn checksum_corruption_fails_closed() {
        let path = test_path("checksum");
        let journal = CacheJournal::open(&path).unwrap();
        journal.append(&state("complete")).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        let index = bytes.len().saturating_sub(3);
        bytes[index] = if bytes[index] == b'0' { b'1' } else { b'0' };
        std::fs::write(&path, bytes).unwrap();

        let error = journal.recover().unwrap_err();
        assert!(error.to_string().contains("checksum"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn unterminated_final_record_is_not_accepted() {
        let path = test_path("truncated");
        let journal = CacheJournal::open(&path).unwrap();
        journal.append(&state("complete")).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.pop();
        bytes.pop();
        std::fs::write(&path, bytes).unwrap();

        let error = journal.recover().unwrap_err();
        assert!(error.to_string().contains("unterminated final record"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn corruption_in_an_older_record_fails_recovery() {
        let path = test_path("older-corruption");
        let journal = CacheJournal::open(&path).unwrap();
        journal.append(&state("preparing")).unwrap();
        journal.append(&state("complete")).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        let first_separator = bytes.iter().position(|byte| *byte == b'|').unwrap();
        bytes[first_separator + 1] = if bytes[first_separator + 1] == b'0' {
            b'1'
        } else {
            b'0'
        };
        std::fs::write(&path, bytes).unwrap();

        let error = journal.recover().unwrap_err();
        assert!(error.to_string().contains("checksum"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn concurrent_appends_remain_whole_records() {
        let path = test_path("concurrent");
        let journal = CacheJournal::open(&path).unwrap();
        let mut workers = Vec::new();
        for worker in 0..8 {
            let journal = CacheJournal::open(&path).unwrap();
            workers.push(std::thread::spawn(move || {
                for record in 0..32 {
                    let mut value = state("preparing");
                    value.operation_id = format!("worker-{worker}-{record}");
                    journal.append(&value).unwrap();
                }
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }

        let contents = std::fs::read_to_string(journal.path()).unwrap();
        assert_eq!(contents.lines().count(), 8 * 32);
        assert!(contents.lines().all(|line| decode(line).is_ok()));
        let _ = std::fs::remove_file(path);
    }
}
