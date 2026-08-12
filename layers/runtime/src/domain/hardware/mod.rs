//! Best-effort local hardware observation for `HARDWARE_REPORT`.

use crate::domain::agent::Registry;
use serde_json::json;
use std::env;
use std::process::Command;

pub(super) fn snapshot(state: &Registry) -> String {
    let gpus = Command::new("nvidia-smi")
        .args([
            "--query-gpu=uuid,name,memory.total,memory.free,driver_version",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({
        "observed_at_unix_ms": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or_default(),
        "os": env::consts::OS, "arch": env::consts::ARCH,
        "cpu_physical": num_cpus::get_physical().max(1),
        "cpu_logical": std::thread::available_parallelism().map(|v| v.get()).unwrap_or(1),
        "gpus": gpus,
        "adapters": state.adapters.iter().map(|(id, value)| json!({"adapter_id": id, "kind": value.kind, "descriptor": value.descriptor})).collect::<Vec<_>>(),
        "nodes": state.nodes.iter().map(|(id, value)| json!({"node_id": id, "adapter_id": value.adapter_id, "bindings": value.bindings.len()})).collect::<Vec<_>>(),
    }).to_string()
}
