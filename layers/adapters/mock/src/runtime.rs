use super::Mock;
use p4_adapter::{Adapter, Distribution, Event, EventSink, Work};

impl Adapter for Mock {
    fn inspect_model(&self, artifact: &str) -> Result<String, String> {
        if artifact.is_empty() {
            return Err("mock artifact reference is empty".into());
        }
        let fingerprint = stable_fingerprint(
            artifact,
            self.profile.stages,
            self.profile.reserved_per_stage,
        );
        Ok(format!(
            r#"{{"schema":2,"artifact":"{}","fingerprint":"{}","architecture":"llama","layers":{},"embedding":4096,"heads":32,"kv_heads":8,"context":32768,"quantization":"Q8_0","experts":0,"distribution":"{:?}","stage_bytes":{},"boundary_bytes":{}}}"#,
            artifact.replace('"', "\\\""),
            fingerprint,
            self.profile.stages.max(1),
            self.distribution,
            self.profile.reserved_per_stage,
            self.profile.reserved_per_stage / 16,
        ))
    }

    fn distribution(&self) -> Distribution {
        self.distribution
    }

    fn report(&self) -> String {
        let loaded = self.loaded.lock().expect("loaded lock").clone();
        let options = self.options.lock().expect("options log lock").len();
        format!(
            r#"{{"backend":"llama-compatible-mock","loaded":{},"artifact":"{}","width_samples":{},"options_seen":{},"busy_ns":{},"idle_ns":{},"peak_hops":{}}}"#,
            loaded.is_some(),
            loaded.unwrap_or_default().replace('"', "\\\""),
            self.widths.lock().expect("width log lock").len(),
            options,
            self.busy.load(std::sync::atomic::Ordering::Relaxed),
            self.idle.load(std::sync::atomic::Ordering::Relaxed),
            self.peak_running.load(std::sync::atomic::Ordering::Relaxed),
        )
    }

    fn start(&self, work: Work, events: &dyn EventSink) {
        match work {
            Work::Load(load) => self.load(load, events),
            Work::Unload(unload) => {
                *self.loaded.lock().expect("loaded lock") = None;
                events.raise(Event::Unloaded {
                    deployment: unload.deployment,
                });
            }
            Work::Hop(hop) => self.hop(hop, events),
            Work::Cache(cache) => self.cache(cache, events),
        }
    }
}

fn stable_fingerprint(artifact: &str, stages: u32, bytes: u64) -> String {
    let mut hash = 1469598103934665603u64;
    for byte in format!("{artifact}:{stages}:{bytes}:llama").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    format!("mock-{hash:016x}")
}
