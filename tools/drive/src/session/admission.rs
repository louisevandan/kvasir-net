//! Logical chain admission owned by the OUTER drive session.
//!
//! This is deliberately not a native stage reservation. It bounds requests at
//! the ingress and records the request attempt/terminal lifecycle so a driver
//! cannot mistake a repeated failure event for permission to enqueue again.

use std::collections::HashMap;

const RETRYABLE_MARKERS: [&str; 8] = [
    "queue",
    "full",
    "timeout",
    "timed out",
    "disconnect",
    "capacity",
    "busy",
    "temporar",
];
const NON_RETRYABLE_MARKERS: [&str; 7] = [
    "malformed",
    "invalid",
    "unsupported",
    "incompatible",
    "identity",
    "lease",
    "protocol",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FailureClass {
    Retryable,
    NonRetryable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Terminal {
    Done,
    Failed(FailureClass),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainLease {
    pub request_id: String,
    pub attempt: u64,
    pub chain_len: usize,
    terminal: Option<Terminal>,
}

impl ChainLease {
    #[allow(dead_code)]
    pub fn terminal(&self) -> Option<&Terminal> {
        self.terminal.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Admission {
    Acquired(ChainLease),
    Full,
    AlreadyActive,
    AlreadyTerminal,
}

pub struct ChainAdmission {
    max_permits: usize,
    active_permits: usize,
    next_attempt: u64,
    leases: HashMap<String, ChainLease>,
}

impl ChainAdmission {
    pub fn new(max_permits: usize) -> Self {
        Self {
            max_permits: max_permits.max(1),
            active_permits: 0,
            next_attempt: 1,
            leases: HashMap::new(),
        }
    }

    /// Lifecycle/idempotency tracking without imposing a capacity policy on
    /// the node. Capacity belongs to the node queue and adapter runner; an
    /// OUTER producer must be able to enqueue work until those components
    /// decide when it can run.
    pub fn new_unbounded() -> Self {
        Self::new(usize::MAX)
    }

    pub fn begin(&mut self, request_id: &str, chain_len: usize) -> Admission {
        if let Some(lease) = self.leases.get(request_id) {
            return if lease.terminal.is_some() {
                Admission::AlreadyTerminal
            } else {
                Admission::AlreadyActive
            };
        }
        if self.active_permits >= self.max_permits {
            return Admission::Full;
        }
        let lease = ChainLease {
            request_id: request_id.to_owned(),
            attempt: self.take_attempt(),
            chain_len,
            terminal: None,
        };
        self.active_permits += 1;
        self.leases.insert(request_id.to_owned(), lease.clone());
        Admission::Acquired(lease)
    }

    pub fn finish(&mut self, request_id: &str, terminal: Terminal) -> bool {
        let Some(lease) = self.leases.get_mut(request_id) else {
            return false;
        };
        if lease.terminal.is_some() {
            return false;
        }
        lease.terminal = Some(terminal);
        self.active_permits = self.active_permits.saturating_sub(1);
        true
    }

    #[allow(dead_code)]
    pub fn retry(&mut self, request_id: &str, chain_len: usize) -> Option<ChainLease> {
        let previous = self.leases.get(request_id)?.terminal.as_ref()?;
        if !matches!(previous, Terminal::Failed(FailureClass::Retryable))
            || self.active_permits >= self.max_permits
        {
            return None;
        }
        let lease = ChainLease {
            request_id: request_id.to_owned(),
            attempt: self.take_attempt(),
            chain_len,
            terminal: None,
        };
        self.active_permits += 1;
        self.leases.insert(request_id.to_owned(), lease.clone());
        Some(lease)
    }

    #[allow(dead_code)]
    pub fn available(&self) -> usize {
        self.max_permits.saturating_sub(self.active_permits)
    }

    #[allow(dead_code)]
    pub fn active(&self) -> usize {
        self.active_permits
    }

    fn take_attempt(&mut self) -> u64 {
        let attempt = self.next_attempt;
        self.next_attempt = self.next_attempt.saturating_add(1);
        attempt
    }
}

pub fn classify_failure(detail: &str) -> FailureClass {
    let detail = detail.to_ascii_lowercase();
    if NON_RETRYABLE_MARKERS
        .iter()
        .any(|marker| detail.contains(marker))
    {
        return FailureClass::NonRetryable;
    }
    if RETRYABLE_MARKERS
        .iter()
        .any(|marker| detail.contains(marker))
    {
        return FailureClass::Retryable;
    }
    FailureClass::NonRetryable
}

#[cfg(test)]
mod tests {
    use super::{Admission, ChainAdmission, FailureClass, Terminal, classify_failure};

    #[test]
    fn bounds_active_logical_chain_permits() {
        let mut admission = ChainAdmission::new(1);
        assert!(matches!(admission.begin("r1", 2), Admission::Acquired(_)));
        assert_eq!(admission.begin("r2", 2), Admission::Full);
        assert_eq!(admission.active(), 1);
        assert_eq!(admission.available(), 0);
    }

    #[test]
    fn outer_submission_is_unbounded_and_only_tracks_lifecycle() {
        let mut admission = ChainAdmission::new_unbounded();
        for index in 0..4096 {
            assert!(matches!(
                admission.begin(&format!("request-{index}"), 4),
                Admission::Acquired(_)
            ));
        }
        assert_eq!(admission.active(), 4096);
    }

    #[test]
    fn duplicate_begin_and_terminal_are_idempotently_rejected() {
        let mut admission = ChainAdmission::new(2);
        assert!(matches!(admission.begin("r1", 3), Admission::Acquired(_)));
        assert_eq!(admission.begin("r1", 3), Admission::AlreadyActive);
        assert!(admission.finish("r1", Terminal::Done));
        assert!(!admission.finish("r1", Terminal::Failed(FailureClass::Retryable)));
        assert_eq!(admission.begin("r1", 3), Admission::AlreadyTerminal);
        assert_eq!(admission.active(), 0);
    }

    #[test]
    fn only_retryable_failure_can_start_a_new_attempt() {
        let mut admission = ChainAdmission::new(1);
        assert!(matches!(admission.begin("r1", 2), Admission::Acquired(_)));
        assert!(admission.finish("r1", Terminal::Failed(FailureClass::Retryable)));
        let retry = admission.retry("r1", 2).expect("retryable attempt");
        assert_eq!(retry.attempt, 2);
        assert_eq!(admission.retry("r1", 2), None);
    }

    #[test]
    fn failure_classification_defaults_safe_and_keeps_protocol_errors_final() {
        assert_eq!(
            classify_failure("node queue is full"),
            FailureClass::Retryable
        );
        assert_eq!(
            classify_failure("unsupported capability"),
            FailureClass::NonRetryable
        );
        assert_eq!(
            classify_failure("unknown backend detail"),
            FailureClass::NonRetryable
        );
    }
}
