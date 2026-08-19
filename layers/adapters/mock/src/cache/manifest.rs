use super::*;

/// Every writer gets its own temporary name. A fixed sibling such as
/// `<sequence>.tmp` lets two processes overwrite one another's incomplete
/// manifest before either rename completes.
pub(super) fn durable_temp_path(path: &std::path::Path) -> std::path::PathBuf {
    let serial = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    path.with_extension(format!("tmp-{}-{serial}", std::process::id()))
}

pub(super) fn reconcile_receipt(
    mock: &Mock,
    cache: &p4_adapter::Cache,
    state: p4_adapter::CacheReceiptState,
    receipt: &PreparedCache,
) -> (p4_adapter::CacheReceiptState, u64, String) {
    let (identity, expected_manifest, receipt_bytes) = match receipt {
        PreparedCache::Persist {
            identity,
            bytes,
            previous,
            resident,
        } => (
            identity,
            if state == p4_adapter::CacheReceiptState::Committed {
                Some(DurableState {
                    bytes: *bytes,
                    position: resident.map_or(0, |progress| progress.position),
                })
            } else {
                *previous
            },
            *bytes,
        ),
        PreparedCache::Restore {
            identity,
            bytes,
            position,
            ..
        } => (
            identity,
            Some(DurableState {
                bytes: *bytes,
                position: *position,
            }),
            *bytes,
        ),
        PreparedCache::Discard { identity, previous } => (
            identity,
            if state == p4_adapter::CacheReceiptState::Committed {
                None
            } else {
                *previous
            },
            0,
        ),
    };
    if !identity.matches(cache) {
        return (
            p4_adapter::CacheReceiptState::Inconsistent,
            0,
            "receipt identity does not match reconciliation request".into(),
        );
    }
    let actual = match mock.available_bytes(cache) {
        Ok(actual) => actual,
        Err(error) => {
            return (
                p4_adapter::CacheReceiptState::Inconsistent,
                0,
                format!("receipt manifest could not be verified: {error}"),
            );
        }
    };
    if actual != expected_manifest {
        return (
            p4_adapter::CacheReceiptState::Inconsistent,
            actual.map_or(0, |state| state.bytes),
            format!(
                "receipt state={} manifest mismatch expected={expected_manifest:?} actual={actual:?}",
                state.as_str()
            ),
        );
    }
    let bytes = if state == p4_adapter::CacheReceiptState::Committed {
        actual.map_or(0, |state| state.bytes)
    } else {
        receipt_bytes
    };
    (
        state,
        bytes,
        format!("receipt state={} manifest=verified", state.as_str()),
    )
}

pub(super) fn io_detail(error: io::Error) -> String {
    error.to_string()
}

const MANIFEST_VERSION: &str = "p4-mock-kv-v2";
const LEGACY_MANIFEST_VERSION: &str = "p4-mock-kv-v1";

pub(super) fn encode_manifest(identity: &CacheIdentity, bytes: u64, position: u32) -> String {
    let checksum = checksum(identity, bytes, position);
    format!(
        "{MANIFEST_VERSION}\n{}\n{}\n{}\n{}\n{bytes}\n{position}\n{checksum:016x}\n",
        hex(&identity.sequence),
        hex(&identity.deployment),
        hex(&identity.stage_id),
        identity.generation,
    )
}

pub(super) fn decode_manifest(
    sequence: &str,
    value: &str,
    expected: Option<&CacheIdentity>,
) -> Result<DurableState, String> {
    let mut fields = value.lines();
    let version = fields
        .next()
        .ok_or("missing durable cache manifest version")?;
    if version != MANIFEST_VERSION && version != LEGACY_MANIFEST_VERSION {
        return Err("unsupported durable cache manifest version".into());
    }
    let manifest = CacheIdentity {
        sequence: unhex(fields.next().ok_or("missing durable sequence")?)?,
        deployment: unhex(fields.next().ok_or("missing durable deployment")?)?,
        stage_id: unhex(fields.next().ok_or("missing durable stage")?)?,
        generation: fields
            .next()
            .ok_or("missing durable generation")?
            .parse()
            .map_err(|_| "invalid durable generation")?,
    };
    if manifest.sequence != sequence {
        return Err("durable cache sequence does not match its path".into());
    }
    let bytes = fields
        .next()
        .ok_or("missing durable bytes")?
        .parse::<u64>()
        .map_err(|_| "invalid durable bytes")?;
    let position = if version == MANIFEST_VERSION {
        fields
            .next()
            .ok_or("missing durable position")?
            .parse::<u32>()
            .map_err(|_| "invalid durable position")?
    } else {
        0
    };
    let actual = fields.next().ok_or("missing durable checksum")?;
    let expected_checksum =
        u64::from_str_radix(actual, 16).map_err(|_| "invalid durable checksum")?;
    if checksum(&manifest, bytes, position) != expected_checksum
        && !(version == LEGACY_MANIFEST_VERSION
            && checksum_v1(&manifest, bytes) == expected_checksum)
    {
        return Err("durable cache checksum mismatch".into());
    }
    if fields.next().is_some() {
        return Err("durable cache manifest has trailing fields".into());
    }
    if let Some(expected) = expected
        && (expected.sequence != manifest.sequence
            || expected.deployment != manifest.deployment
            || expected.stage_id != manifest.stage_id
            || expected.generation != manifest.generation)
    {
        return Err("durable cache identity does not match the request".into());
    }
    Ok(DurableState { bytes, position })
}

fn checksum(identity: &CacheIdentity, bytes: u64, position: u32) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for part in [
        identity.sequence.as_bytes(),
        identity.deployment.as_bytes(),
        identity.stage_id.as_bytes(),
        &identity.generation.to_le_bytes(),
        &bytes.to_le_bytes(),
        &position.to_le_bytes(),
    ] {
        for byte in part {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

fn checksum_v1(identity: &CacheIdentity, bytes: u64) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for part in [
        identity.sequence.as_bytes(),
        identity.deployment.as_bytes(),
        identity.stage_id.as_bytes(),
        &identity.generation.to_le_bytes(),
        &bytes.to_le_bytes(),
    ] {
        for byte in part {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
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
        return Err("odd hexadecimal durable manifest field".into());
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| "invalid hexadecimal durable manifest field")
        })
        .collect::<Result<Vec<_>, _>>()?;
    String::from_utf8(bytes).map_err(|_| "durable manifest field is not UTF-8".into())
}
