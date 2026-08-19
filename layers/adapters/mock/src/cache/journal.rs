use super::super::{CacheIdentity, PreparedCache};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

const PREFIX: &str = "p4-mock-cache-v2";
const LEGACY_PREFIX: &str = "p4-mock-cache-v1";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static JOURNAL_LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JournalStatus {
    Prepared,
    Committed,
    Aborted,
}

type JournalMaps = (
    HashMap<String, PreparedCache>,
    HashMap<String, PreparedCache>,
    HashMap<String, PreparedCache>,
    Option<String>,
);

pub(crate) fn load(root: Option<&Path>) -> JournalMaps {
    let Some(root) = root else {
        return (HashMap::new(), HashMap::new(), HashMap::new(), None);
    };
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return (HashMap::new(), HashMap::new(), HashMap::new(), None);
        }
        Err(error) => {
            return (
                HashMap::new(),
                HashMap::new(),
                HashMap::new(),
                Some(format!("cannot enumerate cache journal directory: {error}")),
            );
        }
    };
    let mut prepared = HashMap::new();
    let mut committed = HashMap::new();
    let mut aborted = HashMap::new();
    let mut error = None;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(entry_error) => {
                error = Some(format!(
                    "cannot enumerate cache transaction directory: {entry_error}"
                ));
                break;
            }
        };
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("txn") {
            continue;
        }
        match read(&path) {
            Ok((operation, value, JournalStatus::Prepared)) => {
                prepared.insert(operation, value);
            }
            Ok((operation, value, JournalStatus::Committed)) => {
                committed.insert(operation, value);
            }
            Ok((operation, value, JournalStatus::Aborted)) => {
                aborted.insert(operation, value);
            }
            Err(detail) => {
                error = Some(format!(
                    "cannot recover cache transaction {}: {detail}",
                    path.display()
                ));
                break;
            }
        }
    }
    (prepared, committed, aborted, error)
}

pub(super) fn write(
    root: Option<&Path>,
    operation: &str,
    prepared: &PreparedCache,
) -> Result<(), String> {
    let Some(root) = root else { return Ok(()) };
    fs::create_dir_all(root).map_err(detail)?;
    let path = path(root, operation, "txn");
    let lock = lock_for(&path);
    let _guard = lock.lock().expect("mock cache journal lock");
    let temporary = temporary_path(&path);
    write_durable(
        &temporary,
        &encode(operation, prepared, JournalStatus::Prepared),
    )?;
    rename_durable(&temporary, &path)
}

pub(super) fn mark_committed(
    root: Option<&Path>,
    operation: &str,
    prepared: &PreparedCache,
) -> Result<(), String> {
    let Some(root) = root else { return Ok(()) };
    fs::create_dir_all(root).map_err(detail)?;
    let path = path(root, operation, "txn");
    let lock = lock_for(&path);
    let _guard = lock.lock().expect("mock cache journal lock");
    let temporary = temporary_path(&path);
    write_durable(
        &temporary,
        &encode(operation, prepared, JournalStatus::Committed),
    )?;
    rename_durable(&temporary, &path)
}

pub(super) fn mark_aborted(
    root: Option<&Path>,
    operation: &str,
    prepared: &PreparedCache,
) -> Result<(), String> {
    let Some(root) = root else { return Ok(()) };
    fs::create_dir_all(root).map_err(detail)?;
    let path = path(root, operation, "txn");
    let lock = lock_for(&path);
    let _guard = lock.lock().expect("mock cache journal lock");
    let temporary = temporary_path(&path);
    write_durable(
        &temporary,
        &encode(operation, prepared, JournalStatus::Aborted),
    )?;
    rename_durable(&temporary, &path)
}

fn lock_for(path: &Path) -> Arc<Mutex<()>> {
    JOURNAL_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("mock cache journal lock registry")
        .entry(path.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

fn rename_durable(temporary: &Path, path: &Path) -> Result<(), String> {
    fs::rename(temporary, path).map_err(detail)?;
    sync_parent(path).map_err(detail)
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::File::open(parent)?.sync_all()
}

#[cfg(windows)]
fn sync_parent(_path: &Path) -> io::Result<()> {
    // std::fs cannot safely open a Windows directory handle for FlushFileBuffers.
    // The file itself is fully flushed before rename; directory-entry durability
    // after rename remains an OS/filesystem-specific limitation here.
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn sync_parent(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn write_durable(path: &Path, contents: &str) -> Result<(), String> {
    let mut file = fs::File::create(path).map_err(detail)?;
    file.write_all(contents.as_bytes()).map_err(detail)?;
    file.sync_all().map_err(detail)
}

fn temporary_path(path: &Path) -> PathBuf {
    let serial = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    path.with_extension(format!("tmp-{}-{serial}", std::process::id()))
}

fn read(path: &Path) -> Result<(String, PreparedCache, JournalStatus), String> {
    let value = fs::read_to_string(path).map_err(detail)?;
    let mut lines = value.split('\n').collect::<Vec<_>>();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let supplied = lines
        .pop()
        .ok_or("empty transaction journal")?
        .strip_prefix("checksum:")
        .ok_or("missing transaction journal checksum")?
        .parse::<u64>()
        .map_err(|_| "invalid transaction journal checksum")?;
    let body = lines.join("\n");
    if checksum(&body) != supplied {
        return Err("transaction journal checksum mismatch".into());
    }
    let mut fields = body.lines();
    let legacy = match fields.next() {
        Some(PREFIX) => false,
        Some(LEGACY_PREFIX) => true,
        _ => {
            return Err("unsupported transaction journal version".into());
        }
    };
    let kind = fields.next().ok_or("missing transaction journal kind")?;
    let status = if kind.starts_with("committed-") {
        JournalStatus::Committed
    } else if kind.starts_with("aborted-") {
        JournalStatus::Aborted
    } else {
        JournalStatus::Prepared
    };
    let kind = kind
        .strip_prefix("committed-")
        .or_else(|| kind.strip_prefix("aborted-"))
        .unwrap_or(kind);
    let operation = unhex(fields.next().ok_or("missing operation")?)?;
    let mut identity = || -> Result<CacheIdentity, String> {
        Ok(CacheIdentity {
            deployment: unhex(fields.next().ok_or("missing deployment")?)?,
            stage_id: unhex(fields.next().ok_or("missing stage")?)?,
            generation: fields
                .next()
                .ok_or("missing generation")?
                .parse()
                .map_err(|_| "invalid generation")?,
            sequence: unhex(fields.next().ok_or("missing sequence")?)?,
        })
    };
    let prepared = match kind {
        "persist" => PreparedCache::Persist {
            identity: identity()?,
            bytes: fields
                .next()
                .ok_or("missing bytes")?
                .parse()
                .map_err(|_| "invalid bytes")?,
            previous: match fields.next().ok_or("missing previous")? {
                "-" => None,
                value => {
                    let bytes = value.parse().map_err(|_| "invalid previous")?;
                    let position = if legacy {
                        0
                    } else {
                        fields
                            .next()
                            .ok_or("missing previous position")?
                            .parse()
                            .map_err(|_| "invalid previous position")?
                    };
                    Some(super::DurableState { bytes, position })
                }
            },
            resident: decode_progress(&mut fields)?,
        },
        "restore" => PreparedCache::Restore {
            identity: identity()?,
            bytes: fields
                .next()
                .ok_or("missing bytes")?
                .parse()
                .map_err(|_| "invalid bytes")?,
            position: if legacy {
                0
            } else {
                fields
                    .next()
                    .ok_or("missing position")?
                    .parse()
                    .map_err(|_| "invalid position")?
            },
            resident: decode_progress(&mut fields)?,
        },
        "discard" => PreparedCache::Discard {
            identity: identity()?,
            previous: decode_optional_state(&mut fields, legacy)?,
        },
        _ => return Err("unknown transaction journal kind".into()),
    };
    if fields.next().is_some() {
        return Err("trailing transaction journal fields".into());
    }
    Ok((operation, prepared, status))
}

fn encode(operation: &str, prepared: &PreparedCache, status: JournalStatus) -> String {
    let (kind, identity, tail) = match prepared {
        PreparedCache::Persist {
            identity,
            bytes,
            previous,
            resident,
        } => (
            "persist",
            identity,
            format!(
                "{bytes}\n{}\n{}",
                previous.map_or_else(
                    || "-".into(),
                    |value| format!("{}\n{}", value.bytes, value.position)
                ),
                encode_progress(*resident)
            ),
        ),
        PreparedCache::Restore {
            identity,
            bytes,
            position,
            resident,
        } => (
            "restore",
            identity,
            format!("{bytes}\n{position}\n{}", encode_progress(*resident)),
        ),
        PreparedCache::Discard { identity, previous } => {
            ("discard", identity, encode_optional_state(*previous))
        }
    };
    let kind = match status {
        JournalStatus::Prepared => kind.to_owned(),
        JournalStatus::Committed => format!("committed-{kind}"),
        JournalStatus::Aborted => format!("aborted-{kind}"),
    };
    let body = format!(
        "{PREFIX}\n{kind}\n{}\n{}\n{}\n{}\n{}\n{tail}",
        hex(operation),
        hex(&identity.deployment),
        hex(&identity.stage_id),
        identity.generation,
        hex(&identity.sequence),
    );
    format!("{body}\nchecksum:{}\n", checksum(&body))
}

fn encode_progress(progress: Option<super::super::Progress>) -> String {
    progress.map_or_else(
        || "-".into(),
        |value| format!("{}:{}:{}", value.turn, value.lifetime, value.position),
    )
}

fn decode_progress<'a>(
    fields: &mut impl Iterator<Item = &'a str>,
) -> Result<Option<super::super::Progress>, String> {
    let value = fields.next().ok_or("missing resident progress")?;
    if value == "-" {
        return Ok(None);
    }
    let mut parts = value.split(':');
    let turn = parts.next().ok_or("invalid resident progress")?;
    let lifetime = parts.next().ok_or("invalid resident progress")?;
    let position = parts.next().unwrap_or("0");
    if parts.next().is_some() {
        return Err("invalid resident progress".into());
    }
    Ok(Some(super::super::Progress {
        turn: turn.parse().map_err(|_| "invalid resident turn")?,
        lifetime: lifetime.parse().map_err(|_| "invalid resident lifetime")?,
        position: position.parse().map_err(|_| "invalid resident position")?,
    }))
}

fn encode_optional_state(value: Option<super::DurableState>) -> String {
    value.map_or_else(
        || "-".into(),
        |state| format!("{}\n{}", state.bytes, state.position),
    )
}

fn decode_optional_state<'a>(
    fields: &mut impl Iterator<Item = &'a str>,
    legacy: bool,
) -> Result<Option<super::DurableState>, String> {
    let value = fields.next().ok_or("missing previous")?;
    if value == "-" {
        Ok(None)
    } else {
        let bytes = value.parse().map_err(|_| "invalid previous".to_owned())?;
        let position = if legacy {
            0
        } else {
            fields
                .next()
                .ok_or("missing previous position")?
                .parse()
                .map_err(|_| "invalid previous position")?
        };
        Ok(Some(super::DurableState { bytes, position }))
    }
}

fn path(root: &Path, value: &str, extension: &str) -> PathBuf {
    root.join(format!("{}.{}", hex(value), extension))
}

fn hex(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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

fn detail(error: io::Error) -> String {
    error.to_string()
}

fn checksum(value: &str) -> u64 {
    value.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}
