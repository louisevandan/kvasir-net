use p4_adapter::node_adapter::CompletionStorageSnapshot;
use std::sync::Arc;

pub const RESOURCE_PROFILE_VERSION: u16 = 1;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResourceProfile {
    pub version: u16,
    pub max_requests: u32,
    pub max_request_retained_bytes: u64,
    pub max_input_tokens: u64,
    pub max_request_bytes: u64,
    pub max_output_tokens_per_request: u32,
    pub max_output_tokens: u64,
    pub max_physical_result_bytes: u64,
    pub max_completion_payload_bytes: u64,
    pub max_completion_retained_bytes: u64,
    pub max_edge_retained_bytes: u64,
    pub max_receipt_retained_bytes: u64,
}

/// Read-only capacity from P4's backend-neutral retained stores. The llama
/// adapter owns the profile and interpretation; the composition layer only
/// reports actual count/byte usage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceStorageSnapshot {
    pub count_limit: usize,
    pub retained_count: usize,
    pub byte_limit: usize,
    pub retained_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeResourceSnapshot {
    pub edge: ResourceStorageSnapshot,
    pub receipt: ResourceStorageSnapshot,
}

#[derive(Clone)]
pub struct RuntimeResourceProbe(
    Arc<dyn Fn() -> Result<RuntimeResourceSnapshot, String> + Send + Sync>,
);

impl RuntimeResourceProbe {
    pub fn new(
        probe: impl Fn() -> Result<RuntimeResourceSnapshot, String> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(probe))
    }

    pub(crate) fn snapshot(&self) -> Result<RuntimeResourceSnapshot, String> {
        (self.0)()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ValidatedResourceProfile {
    pub request_limit: super::node::request_budget::RequestCost,
}

impl ResourceProfile {
    pub(crate) fn validate_preload(
        &self,
        completion: CompletionStorageSnapshot,
        runtime: RuntimeResourceSnapshot,
    ) -> Result<ValidatedResourceProfile, String> {
        if self.version != RESOURCE_PROFILE_VERSION {
            return Err(format!(
                "resource profile version {} is unsupported",
                self.version
            ));
        }
        if self.max_requests == 0
            || self.max_request_retained_bytes == 0
            || self.max_input_tokens == 0
            || self.max_request_bytes == 0
            || self.max_output_tokens_per_request == 0
            || self.max_output_tokens == 0
            || self.max_physical_result_bytes == 0
            || self.max_completion_payload_bytes == 0
            || self.max_completion_retained_bytes == 0
            || self.max_edge_retained_bytes == 0
            || self.max_receipt_retained_bytes == 0
        {
            return Err("resource profile fields must be positive".into());
        }
        if self.max_request_bytes > self.max_request_retained_bytes {
            return Err("one request exceeds the aggregate request byte limit".into());
        }
        let reserved_output = u64::from(self.max_requests)
            .checked_mul(u64::from(self.max_output_tokens_per_request))
            .ok_or("resource profile output token reservation overflows")?;
        if reserved_output > self.max_output_tokens {
            return Err("aggregate output token limit cannot cover every request".into());
        }
        if self.max_physical_result_bytes > self.max_completion_payload_bytes
            || self.max_completion_payload_bytes > self.max_completion_retained_bytes
            || self.max_completion_payload_bytes > self.max_edge_retained_bytes
        {
            return Err("physical result, completion and edge byte limits are inconsistent".into());
        }
        let request_limit = super::node::request_budget::RequestCost {
            requests: usize::try_from(self.max_requests)
                .map_err(|_| "resource profile request count exceeds platform range")?,
            bytes: usize::try_from(self.max_request_retained_bytes)
                .map_err(|_| "resource profile request bytes exceed platform range")?,
            prompt_tokens: usize::try_from(self.max_input_tokens)
                .map_err(|_| "resource profile input tokens exceed platform range")?,
            output_tokens: usize::try_from(self.max_output_tokens)
                .map_err(|_| "resource profile output tokens exceed platform range")?,
        };
        let _max_request_bytes = usize::try_from(self.max_request_bytes)
            .map_err(|_| "resource profile request bytes exceed platform range")?;
        validate_completion_storage(
            request_limit.requests,
            usize::try_from(self.max_completion_retained_bytes)
                .map_err(|_| "resource profile completion bytes exceed platform range")?,
            completion,
        )?;
        validate_runtime_storage(
            "edge",
            request_limit.requests,
            usize::try_from(self.max_edge_retained_bytes)
                .map_err(|_| "resource profile edge bytes exceed platform range")?,
            runtime.edge,
        )?;
        validate_runtime_storage(
            "receipt",
            request_limit.requests,
            usize::try_from(self.max_receipt_retained_bytes)
                .map_err(|_| "resource profile receipt bytes exceed platform range")?,
            runtime.receipt,
        )?;
        Ok(ValidatedResourceProfile { request_limit })
    }

    pub(crate) fn validate_ready_bound(&self, actual: u64) -> Result<(), String> {
        if actual != self.max_physical_result_bytes {
            return Err(format!(
                "resource profile physical result mismatch: profile={}, READY={actual}",
                self.max_physical_result_bytes
            ));
        }
        Ok(())
    }
}

fn validate_completion_storage(
    required_count: usize,
    required_bytes: usize,
    actual: CompletionStorageSnapshot,
) -> Result<(), String> {
    let byte_limit = actual
        .byte_limit
        .ok_or("completion retained byte limit is not configured")?;
    validate_runtime_storage(
        "completion",
        required_count,
        required_bytes,
        ResourceStorageSnapshot {
            count_limit: actual.capacity,
            retained_count: actual.retained_count,
            byte_limit,
            retained_bytes: actual.retained_bytes,
        },
    )
}

fn validate_runtime_storage(
    name: &str,
    required_count: usize,
    required_bytes: usize,
    actual: ResourceStorageSnapshot,
) -> Result<(), String> {
    let available_count = actual
        .count_limit
        .checked_sub(actual.retained_count)
        .ok_or_else(|| format!("{name} retained count exceeds its limit"))?;
    let available_bytes = actual
        .byte_limit
        .checked_sub(actual.retained_bytes)
        .ok_or_else(|| format!("{name} retained bytes exceed their limit"))?;
    if required_count > available_count {
        return Err(format!(
            "{name} retained count is insufficient: need {required_count}, available {available_count}"
        ));
    }
    if required_bytes > available_bytes {
        return Err(format!(
            "{name} retained bytes are insufficient: need {required_bytes}, available {available_bytes}"
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn fixture_resource_profile() -> ResourceProfile {
    ResourceProfile {
        version: RESOURCE_PROFILE_VERSION,
        max_requests: 1,
        max_request_retained_bytes: 1 << 20,
        max_input_tokens: 4096,
        max_request_bytes: 1 << 19,
        max_output_tokens_per_request: 2048,
        max_output_tokens: 2048,
        max_physical_result_bytes: 33_554_432,
        max_completion_payload_bytes: 64 << 20,
        max_completion_retained_bytes: 64 << 20,
        max_edge_retained_bytes: 64 << 20,
        max_receipt_retained_bytes: 1 << 20,
    }
}

#[cfg(test)]
pub(crate) fn worker_fixture_resource_profile(max_physical_result_bytes: u64) -> ResourceProfile {
    ResourceProfile {
        version: RESOURCE_PROFILE_VERSION,
        max_requests: 4096,
        max_request_retained_bytes: 512 * 1024 * 1024,
        max_input_tokens: 16 * 1024 * 1024,
        max_request_bytes: 512 * 1024 * 1024,
        max_output_tokens_per_request: 4096,
        max_output_tokens: 16 * 1024 * 1024,
        max_physical_result_bytes: max_physical_result_bytes.max(1),
        max_completion_payload_bytes: 512 * 1024 * 1024,
        max_completion_retained_bytes: 512 * 1024 * 1024,
        max_edge_retained_bytes: 512 * 1024 * 1024,
        max_receipt_retained_bytes: 512 * 1024 * 1024,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn completion(count: usize, bytes: usize) -> CompletionStorageSnapshot {
        CompletionStorageSnapshot {
            capacity: count,
            queue_capacity: count,
            byte_limit: Some(bytes),
            retained_count: 0,
            retained_bytes: 0,
            queued_count: 0,
            reserved_queue_slots: 0,
            queue_backing_bytes: 0,
            closed: false,
        }
    }

    fn runtime(edge_count: usize, edge_bytes: usize) -> RuntimeResourceSnapshot {
        RuntimeResourceSnapshot {
            edge: ResourceStorageSnapshot {
                count_limit: edge_count,
                retained_count: 0,
                byte_limit: edge_bytes,
                retained_bytes: 0,
            },
            receipt: ResourceStorageSnapshot {
                count_limit: 1,
                retained_count: 0,
                byte_limit: 1 << 20,
                retained_bytes: 0,
            },
        }
    }

    #[test]
    fn exact_store_limits_are_accepted_and_minus_one_is_rejected() {
        let profile = fixture_resource_profile();
        assert!(
            profile
                .validate_preload(completion(1, 64 << 20), runtime(1, 64 << 20))
                .is_ok()
        );
        assert!(
            profile
                .validate_preload(completion(0, 64 << 20), runtime(1, 64 << 20))
                .is_err()
        );
        assert!(
            profile
                .validate_preload(completion(1, (64 << 20) - 1), runtime(1, 64 << 20))
                .is_err()
        );
        assert!(
            profile
                .validate_preload(completion(1, 64 << 20), runtime(0, 64 << 20))
                .is_err()
        );
        assert!(
            profile
                .validate_preload(completion(1, 64 << 20), runtime(1, (64 << 20) - 1))
                .is_err()
        );
        let mut receipt_short = runtime(1, 64 << 20);
        receipt_short.receipt.byte_limit -= 1;
        assert!(
            profile
                .validate_preload(completion(1, 64 << 20), receipt_short)
                .is_err()
        );
    }

    #[test]
    fn version_output_reservation_and_physical_payload_fail_closed() {
        let mut profile = fixture_resource_profile();
        profile.max_output_tokens = 2047;
        assert!(
            profile
                .validate_preload(completion(1, 64 << 20), runtime(1, 64 << 20))
                .is_err()
        );
        let mut profile = fixture_resource_profile();
        profile.version += 1;
        assert!(
            profile
                .validate_preload(completion(1, 64 << 20), runtime(1, 64 << 20))
                .is_err()
        );
        let mut profile = fixture_resource_profile();
        profile.max_completion_payload_bytes = profile.max_physical_result_bytes - 1;
        assert!(
            profile
                .validate_preload(completion(1, 64 << 20), runtime(1, 64 << 20))
                .is_err()
        );
    }
}
