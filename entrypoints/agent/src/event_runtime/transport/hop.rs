use p4_protocol::event::hop::{EventDigest, MAX_DETAIL_BYTES, ReceiptStatus};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

const FIXED_RECEIPT_BYTES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct ReceiptKey {
    pub sender_id: String,
    pub connection_generation: u64,
    pub attempt: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ReceiptView {
    pub status: ReceiptStatus,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReserveError {
    Full,
    TooLarge,
    Conflict,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Reservation {
    Reserved,
    Existing(ReceiptView),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ReceiptSnapshot {
    pub limit_count: usize,
    pub limit_bytes: usize,
    pub records: usize,
    pub reserved_bytes: usize,
    pub pending: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub oldest_unix_ms: Option<u64>,
}

struct Record {
    digest: EventDigest,
    event_id: String,
    status: Option<ReceiptStatus>,
    detail: String,
    reserved_bytes: usize,
    created_unix_ms: u64,
}

pub(super) struct ReceiptStore {
    limit_count: usize,
    limit_bytes: usize,
    reserved_bytes: usize,
    records: HashMap<ReceiptKey, Record>,
}

impl ReceiptStore {
    pub(super) fn new(limit_count: usize, limit_bytes: usize) -> Self {
        assert!(limit_count > 0 && limit_bytes > 0);
        Self {
            limit_count,
            limit_bytes,
            reserved_bytes: 0,
            records: HashMap::with_capacity(limit_count.min(65_536)),
        }
    }

    pub(super) fn reserve(
        &mut self,
        key: ReceiptKey,
        digest: EventDigest,
        event_id: &str,
    ) -> Result<Reservation, ReserveError> {
        if let Some(record) = self.records.get(&key) {
            if record.digest != digest {
                return Err(ReserveError::Conflict);
            }
            return Ok(Reservation::Existing(view(record)));
        }
        let required = receipt_bytes(&key, event_id).ok_or(ReserveError::TooLarge)?;
        if required > self.limit_bytes {
            return Err(ReserveError::TooLarge);
        }
        let total = self
            .reserved_bytes
            .checked_add(required)
            .ok_or(ReserveError::TooLarge)?;
        if self.records.len() == self.limit_count || total > self.limit_bytes {
            return Err(ReserveError::Full);
        }
        self.reserved_bytes = total;
        self.records.insert(
            key,
            Record {
                digest,
                event_id: event_id.into(),
                status: None,
                detail: String::new(),
                reserved_bytes: required,
                created_unix_ms: now(),
            },
        );
        Ok(Reservation::Reserved)
    }

    pub(super) fn commit(
        &mut self,
        key: &ReceiptKey,
        digest: EventDigest,
        status: ReceiptStatus,
        detail: impl Into<String>,
    ) -> Result<ReceiptView, &'static str> {
        let record = self
            .records
            .get_mut(key)
            .ok_or("hop receipt reservation missing")?;
        if record.digest != digest {
            return Err("hop receipt digest changed after reservation");
        }
        if record.status.is_some() {
            return Err("hop receipt already committed");
        }
        let detail = detail.into();
        if detail.len() > MAX_DETAIL_BYTES {
            return Err("hop receipt detail exceeds reservation");
        }
        record.status = Some(status);
        record.detail = detail;
        Ok(view(record))
    }

    pub(super) fn query(&self, key: &ReceiptKey, digest: EventDigest) -> ReceiptView {
        match self.records.get(key) {
            Some(record) if record.digest != digest => ReceiptView {
                status: ReceiptStatus::Conflict,
                detail: "attempt digest differs".into(),
            },
            Some(record) => view(record),
            None => ReceiptView {
                status: ReceiptStatus::Unknown,
                detail: "receipt is not pinned".into(),
            },
        }
    }

    pub(super) fn acknowledge(&mut self, key: &ReceiptKey, digest: EventDigest) -> bool {
        let removable = self
            .records
            .get(key)
            .is_some_and(|record| record.digest == digest && record.status.is_some());
        if !removable {
            return false;
        }
        let removed = self.records.remove(key).expect("checked receipt");
        self.reserved_bytes -= removed.reserved_bytes;
        true
    }

    pub(super) fn snapshot(&self) -> ReceiptSnapshot {
        let mut pending = 0;
        let mut accepted = 0;
        let mut rejected = 0;
        for record in self.records.values() {
            match record.status {
                None => pending += 1,
                Some(ReceiptStatus::AcceptedExact) => accepted += 1,
                Some(
                    ReceiptStatus::Rejected | ReceiptStatus::Conflict | ReceiptStatus::Unknown,
                ) => rejected += 1,
            }
        }
        ReceiptSnapshot {
            limit_count: self.limit_count,
            limit_bytes: self.limit_bytes,
            records: self.records.len(),
            reserved_bytes: self.reserved_bytes,
            pending,
            accepted,
            rejected,
            oldest_unix_ms: self
                .records
                .values()
                .map(|record| record.created_unix_ms)
                .min(),
        }
    }

    #[cfg(test)]
    pub(super) fn event_id(&self, key: &ReceiptKey) -> Option<&str> {
        self.records.get(key).map(|record| record.event_id.as_str())
    }
}

fn view(record: &Record) -> ReceiptView {
    ReceiptView {
        status: record.status.unwrap_or(ReceiptStatus::Unknown),
        detail: if record.status.is_none() {
            "receipt commit is pending".into()
        } else {
            record.detail.clone()
        },
    }
}

fn receipt_bytes(key: &ReceiptKey, event_id: &str) -> Option<usize> {
    FIXED_RECEIPT_BYTES
        .checked_add(key.sender_id.len())?
        .checked_add(event_id.len())?
        .checked_add(MAX_DETAIL_BYTES)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests;
