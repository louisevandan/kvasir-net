//! Fixed-size evidence of head-approved physical issue membership.
//!
//! This is an adapter-owned integrity witness, not a signature, engine result,
//! KV completion receipt or a complete telemetry ledger. The caller
//! advances a candidate only after a native split matches the prepared issue,
//! then commits that candidate together with issued authority. Failed attempts
//! and replayed terminal capsules do not advance this witness.
use super::Phase;
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, OuterEndpoint};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::str::FromStr;

const AUTHORITY_DOMAIN: &[u8] = b"P4_ISSUE_AUTHORITY_V1\0";
const WORK_DOMAIN: &[u8] = b"P4_ISSUED_WORK_V1\0";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueAuthority {
    pub head: Endpoint,
    pub outer: OuterEndpoint,
    pub load_generation: u64,
    pub session_id: String,
    pub request_id: String,
    pub submission_event_id: String,
    pub sequence_id: u32,
    pub incarnation: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IssuedRow {
    #[serde(with = "phase_wire")]
    pub phase: Phase,
    pub position: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssuedExecution {
    pub execution_id: u64,
    pub rows: Vec<IssuedRow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssuedWork {
    pub logical_ordinal: u64,
    pub executions: Vec<IssuedExecution>,
}

/// Exactly two digests and two counters; no history, prompt or tensor is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IssueWitness {
    seed_digest: [u8; 32],
    digest: [u8; 32],
    issue_count: u64,
    last_ordinal: u64,
}

impl IssueWitness {
    pub fn new(authority: &IssueAuthority) -> Result<Self, &'static str> {
        let digest = authority_digest(authority)?;
        Ok(Self {
            seed_digest: digest,
            digest,
            issue_count: 0,
            last_ordinal: 0,
        })
    }

    pub fn advanced(
        &self,
        authority: &IssueAuthority,
        work: &IssuedWork,
    ) -> Result<Self, &'static str> {
        let (count, canonical) = self.prepare(authority, work)?;
        let mut hash = Sha256::new();
        encode_work(
            self.digest,
            count,
            work.logical_ordinal,
            &canonical,
            &mut |bytes| {
                hash.update(bytes);
            },
        );
        Ok(Self {
            seed_digest: self.seed_digest,
            digest: hash.finalize().into(),
            issue_count: count,
            last_ordinal: work.logical_ordinal,
        })
    }

    pub fn issue_count(&self) -> u64 {
        self.issue_count
    }

    pub fn last_ordinal(&self) -> u64 {
        self.last_ordinal
    }

    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }

    pub fn authority_digest(&self) -> [u8; 32] {
        self.seed_digest
    }

    /// Export the already accepted chain. Telemetry never advances it.
    pub fn proof(&self) -> IssuedWorkProof {
        IssuedWorkProof {
            revision: 1,
            issue_count: self.issue_count,
            last_ordinal: self.last_ordinal,
            authority_digest: self.seed_digest,
            digest: self.digest,
        }
    }

    /// Golden-vector API. Production hashing streams through the same encoder
    /// without allocating a copy of the canonical byte stream.
    #[cfg(test)]
    pub(crate) fn canonical_work_bytes(
        &self,
        authority: &IssueAuthority,
        work: &IssuedWork,
    ) -> Result<Vec<u8>, &'static str> {
        let (count, canonical) = self.prepare(authority, work)?;
        let mut bytes = Vec::new();
        encode_work(
            self.digest,
            count,
            work.logical_ordinal,
            &canonical,
            &mut |part| {
                bytes.extend_from_slice(part);
            },
        );
        Ok(bytes)
    }

    fn prepare<'a>(
        &self,
        authority: &IssueAuthority,
        work: &'a IssuedWork,
    ) -> Result<(u64, CanonicalWork<'a>), &'static str> {
        if authority_digest(authority)? != self.seed_digest {
            return Err("issued-work authority differs from the admitted attempt");
        }
        if work.logical_ordinal == 0 || work.logical_ordinal <= self.last_ordinal {
            return Err("issued-work ordinal must strictly advance");
        }
        let count = self
            .issue_count
            .checked_add(1)
            .ok_or("issued-work count exhausted")?;
        Ok((count, canonical_work(work)?))
    }

    /// Fault injection cannot construct impossible counters in a release build.
    #[cfg(test)]
    pub(crate) fn with_test_counters(mut self, issue_count: u64, last_ordinal: u64) -> Self {
        self.issue_count = issue_count;
        self.last_ordinal = last_ordinal;
        self
    }
}

/// Terminal wire snapshot. Digests are exactly 32 octets, not permissive hex
/// or JSON hashes. Integrity evidence is not authentication or KV completion.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IssuedWorkProof {
    pub revision: u8,
    pub issue_count: u64,
    pub last_ordinal: u64,
    pub authority_digest: [u8; 32],
    pub digest: [u8; 32],
}

impl IssuedWorkProof {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.revision != 1 || self.issue_count == 0 || self.last_ordinal < self.issue_count {
            return Err("terminal issued-work proof has invalid revision or counters");
        }
        Ok(())
    }
}

mod phase_wire {
    use super::Phase;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(phase: &Phase, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match phase {
            Phase::Prefill => "prefill",
            Phase::Decode => "decode",
            Phase::Verify => "verify",
            Phase::Replay => "replay",
        })
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Phase, D::Error> {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "prefill" => Ok(Phase::Prefill),
            "decode" => Ok(Phase::Decode),
            "verify" => Ok(Phase::Verify),
            "replay" => Ok(Phase::Replay),
            _ => Err(serde::de::Error::custom("unknown issued-row phase")),
        }
    }
}

/// The encoding is deliberately not serde/JSON or a platform's Hash trait.
/// Strings carry a u32 LE UTF-8 byte length. All scalar widths and phase codes
/// are fixed here, so native/telemetry list order cannot change the digest.
#[cfg(test)]
pub(crate) fn canonical_authority_bytes(
    authority: &IssueAuthority,
) -> Result<Vec<u8>, &'static str> {
    let validated = validate_authority(authority)?;
    let mut bytes = Vec::new();
    encode_authority(authority, &validated, &mut |part| {
        bytes.extend_from_slice(part)
    });
    Ok(bytes)
}

fn authority_digest(authority: &IssueAuthority) -> Result<[u8; 32], &'static str> {
    let validated = validate_authority(authority)?;
    let mut hash = Sha256::new();
    encode_authority(authority, &validated, &mut |bytes| hash.update(bytes));
    Ok(hash.finalize().into())
}

struct AuthorityAddresses {
    head: String,
    outer: String,
}

fn validate_authority(authority: &IssueAuthority) -> Result<AuthorityAddresses, &'static str> {
    let addresses = validate_submission_fields(
        &authority.head,
        &authority.outer,
        authority.load_generation,
        &authority.session_id,
        &authority.request_id,
        &authority.submission_event_id,
    )?;
    if authority.incarnation == 0 {
        return Err("issued-work generations and incarnation must be nonzero");
    }
    Ok(addresses)
}

/// Admission validates the same original identity before tokenization, KV or
/// request bookkeeping. No slot/incarnation or provisional witness is minted.
pub(crate) fn validate_submission_identity(
    head: &Endpoint,
    outer: &OuterEndpoint,
    load_generation: u64,
    session_id: &str,
    request_id: &str,
    submission_event_id: &str,
) -> Result<(), &'static str> {
    validate_submission_fields(
        head,
        outer,
        load_generation,
        session_id,
        request_id,
        submission_event_id,
    )
    .map(|_| ())
}

fn validate_submission_fields(
    head: &Endpoint,
    outer: &OuterEndpoint,
    load_generation: u64,
    session_id: &str,
    request_id: &str,
    submission_event_id: &str,
) -> Result<AuthorityAddresses, &'static str> {
    let Endpoint::Node {
        agent,
        node,
        generation,
    } = head
    else {
        return Err("issued-work authority requires a head node endpoint");
    };
    head.validate()
        .map_err(|_| "issued-work head endpoint is invalid")?;
    Endpoint::Outer(outer.clone())
        .validate()
        .map_err(|_| "issued-work outer endpoint is invalid")?;
    for value in [
        node,
        &outer.channel,
        session_id,
        request_id,
        submission_event_id,
    ] {
        validate_string(value)?;
    }
    if *generation == 0 || load_generation == 0 {
        return Err("issued-work generations and incarnation must be nonzero");
    }
    Ok(AuthorityAddresses {
        head: normalized_address(agent)?,
        outer: normalized_address(&outer.ingress_agent)?,
    })
}

fn normalized_address(address: &Address) -> Result<String, &'static str> {
    // Respect Address's current identity semantics. Do not resolve DNS, fold
    // host case or collapse IP spellings that P4 itself considers different.
    validate_string(&address.host)?;
    let canonical = address.to_string();
    let parsed = Address::from_str(&canonical).map_err(|_| "issued-work address is invalid")?;
    if &parsed != address {
        return Err("issued-work address does not survive its canonical encoding");
    }
    validate_string(&canonical)?;
    Ok(canonical)
}

fn validate_string(value: &str) -> Result<(), &'static str> {
    if value.is_empty() || value.contains('\0') || u32::try_from(value.len()).is_err() {
        return Err("issued-work identity string is empty, contains NUL or is too long");
    }
    Ok(())
}

fn encode_authority(
    authority: &IssueAuthority,
    addresses: &AuthorityAddresses,
    sink: &mut impl FnMut(&[u8]),
) {
    let Endpoint::Node {
        node, generation, ..
    } = &authority.head
    else {
        unreachable!("authority was validated")
    };
    sink(AUTHORITY_DOMAIN);
    put_string(&addresses.head, sink);
    put_string(node, sink);
    sink(&generation.to_le_bytes());
    put_string(&addresses.outer, sink);
    put_string(&authority.outer.channel, sink);
    sink(&authority.outer.connection_generation.to_le_bytes());
    sink(&authority.load_generation.to_le_bytes());
    put_string(&authority.session_id, sink);
    put_string(&authority.request_id, sink);
    put_string(&authority.submission_event_id, sink);
    sink(&authority.sequence_id.to_le_bytes());
    sink(&authority.incarnation.to_le_bytes());
}

fn put_string(value: &str, sink: &mut impl FnMut(&[u8])) {
    sink(
        &u32::try_from(value.len())
            .expect("string length was validated")
            .to_le_bytes(),
    );
    sink(value.as_bytes());
}

struct CanonicalExecution<'a> {
    execution_id: u64,
    rows: Vec<&'a IssuedRow>,
}

struct CanonicalWork<'a>(Vec<CanonicalExecution<'a>>);

fn canonical_work(work: &IssuedWork) -> Result<CanonicalWork<'_>, &'static str> {
    if work.executions.is_empty() || u32::try_from(work.executions.len()).is_err() {
        return Err("issued-work executions are empty or too numerous");
    }
    let mut ids = BTreeSet::new();
    let mut all_rows = BTreeSet::new();
    let mut executions = Vec::with_capacity(work.executions.len());
    for execution in &work.executions {
        if execution.execution_id == 0 || !ids.insert(execution.execution_id) {
            return Err("issued-work execution identity is zero or repeated");
        }
        if execution.rows.is_empty() || u32::try_from(execution.rows.len()).is_err() {
            return Err("issued-work execution rows are empty or too numerous");
        }
        let mut rows = Vec::with_capacity(execution.rows.len());
        for row in &execution.rows {
            if !all_rows.insert((phase_code(row.phase), row.position)) {
                return Err("issued-work repeats a logical phase and position");
            }
            rows.push(row);
        }
        rows.sort_unstable_by_key(|row| (phase_code(row.phase), row.position));
        executions.push(CanonicalExecution {
            execution_id: execution.execution_id,
            rows,
        });
    }
    executions.sort_unstable_by_key(|execution| execution.execution_id);
    Ok(CanonicalWork(executions))
}

fn phase_code(phase: Phase) -> u8 {
    match phase {
        Phase::Prefill => 0,
        Phase::Decode => 1,
        Phase::Verify => 2,
        Phase::Replay => 3,
    }
}

fn encode_work(
    previous: [u8; 32],
    count: u64,
    ordinal: u64,
    work: &CanonicalWork<'_>,
    sink: &mut impl FnMut(&[u8]),
) {
    sink(WORK_DOMAIN);
    sink(&previous);
    sink(&count.to_le_bytes());
    sink(&ordinal.to_le_bytes());
    sink(
        &u32::try_from(work.0.len())
            .expect("execution count was validated")
            .to_le_bytes(),
    );
    for execution in &work.0 {
        sink(&execution.execution_id.to_le_bytes());
        sink(
            &u32::try_from(execution.rows.len())
                .expect("row count was validated")
                .to_le_bytes(),
        );
        for row in &execution.rows {
            sink(&[phase_code(row.phase)]);
            sink(&row.position.to_le_bytes());
        }
    }
}

#[cfg(test)]
mod tests;
