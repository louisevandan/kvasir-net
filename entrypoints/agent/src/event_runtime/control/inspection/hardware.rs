//! Platform-owned hardware observation for the agent inspection snapshot.

use serde::Serialize;
use serde_json::{Value, json};
use std::process::Command;

const MIB: u64 = 1024 * 1024;

#[derive(Debug, Serialize)]
struct GpuCapability {
    index: u32,
    uuid: String,
    vendor: &'static str,
    name: String,
    pci_bus_id: String,
    driver_version: String,
    vram_total_bytes: u64,
}

#[derive(Debug, Serialize)]
struct GpuOccupancy {
    uuid: String,
    vram_used_bytes: u64,
    vram_free_bytes: u64,
    utilization_gpu_percent: Option<u32>,
    temperature_c: Option<u32>,
    power_draw_w: Option<f64>,
}

#[derive(Debug)]
struct GpuSample {
    capability: GpuCapability,
    occupancy: GpuOccupancy,
}

#[derive(Debug)]
struct MemorySample {
    total_bytes: u64,
    available_bytes: u64,
}

pub(super) struct HardwareObservation {
    capability: Value,
    occupancy: Value,
    probes: Value,
}

impl HardwareObservation {
    pub(super) fn with_adapters(self, adapters: impl Serialize) -> Value {
        let mut capability = self.capability;
        capability["adapters"] = serde_json::to_value(adapters).unwrap_or_else(|_| json!([]));
        json!({
            "capability": capability,
            "occupancy": self.occupancy,
            "probes": self.probes,
        })
    }
}

pub(super) fn observe() -> HardwareObservation {
    let memory = memory_sample();
    let gpus = gpu_samples();
    build_observation(memory, gpus)
}

pub(super) fn failed(detail: String) -> HardwareObservation {
    build_observation(Err(detail.clone()), Err(ProbeFailure::Error(detail)))
}

fn build_observation(
    memory: Result<MemorySample, String>,
    gpus: Result<Vec<GpuSample>, ProbeFailure>,
) -> HardwareObservation {
    let (memory_capability, memory_occupancy, memory_probe) = match memory {
        Ok(sample) => (
            json!({"total_bytes": sample.total_bytes}),
            json!({
                "available_bytes": sample.available_bytes,
                "used_bytes": sample.total_bytes.saturating_sub(sample.available_bytes),
            }),
            json!({"source":"os","state":"available","detail":null}),
        ),
        Err(detail) => (
            json!({"total_bytes":null}),
            json!({"available_bytes":null,"used_bytes":null}),
            json!({"source":"os","state":"error","detail":detail}),
        ),
    };

    let (gpu_capabilities, gpu_occupancies, gpu_probe) = match gpus {
        Ok(samples) => {
            let capabilities: Vec<_> = samples.iter().map(|gpu| &gpu.capability).collect();
            let occupancies: Vec<_> = samples.iter().map(|gpu| &gpu.occupancy).collect();
            (
                json!(capabilities),
                json!(occupancies),
                json!({"source":"nvidia-smi","state":"available","detail":null}),
            )
        }
        Err(ProbeFailure::Unavailable(detail)) => (
            json!([]),
            json!([]),
            json!({"source":"nvidia-smi","state":"unavailable","detail":detail}),
        ),
        Err(ProbeFailure::Error(detail)) => (
            json!([]),
            json!([]),
            json!({"source":"nvidia-smi","state":"error","detail":detail}),
        ),
    };

    HardwareObservation {
        capability: json!({
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "cpu": {
                "physical_cores": num_cpus::get_physical().max(1),
                "logical_cores": std::thread::available_parallelism()
                    .map(|value| value.get())
                    .unwrap_or_else(|_| num_cpus::get().max(1)),
            },
            "memory": memory_capability,
            "gpus": gpu_capabilities,
        }),
        occupancy: json!({
            "memory": memory_occupancy,
            "gpus": gpu_occupancies,
        }),
        probes: json!({
            "memory": memory_probe,
            "gpus": gpu_probe,
        }),
    }
}

#[derive(Debug)]
enum ProbeFailure {
    Unavailable(String),
    Error(String),
}

fn gpu_samples() -> Result<Vec<GpuSample>, ProbeFailure> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,uuid,name,pci.bus_id,memory.total,memory.used,memory.free,utilization.gpu,temperature.gpu,power.draw,driver_version",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .map_err(|error| ProbeFailure::Unavailable(format!("nvidia-smi unavailable: {error}")))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(ProbeFailure::Error(if detail.is_empty() {
            format!("nvidia-smi exited with {}", output.status)
        } else {
            detail.chars().take(512).collect()
        }));
    }
    parse_nvidia_csv(&String::from_utf8_lossy(&output.stdout)).map_err(ProbeFailure::Error)
}

fn parse_nvidia_csv(csv: &str) -> Result<Vec<GpuSample>, String> {
    csv.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .enumerate()
        .map(|(line_index, line)| {
            let columns: Vec<_> = line.split(',').map(str::trim).collect();
            if columns.len() != 11 {
                return Err(format!(
                    "nvidia-smi row {} has {} columns, expected 11",
                    line_index + 1,
                    columns.len()
                ));
            }
            let index = parse_required::<u32>(columns[0], "index", line_index)?;
            let uuid = required_text(columns[1], "uuid", line_index)?;
            let total = parse_mib(columns[4], "memory.total", line_index)?;
            let used = parse_mib(columns[5], "memory.used", line_index)?;
            let free = parse_mib(columns[6], "memory.free", line_index)?;
            Ok(GpuSample {
                capability: GpuCapability {
                    index,
                    uuid: uuid.clone(),
                    vendor: "NVIDIA",
                    name: required_text(columns[2], "name", line_index)?,
                    pci_bus_id: required_text(columns[3], "pci.bus_id", line_index)?,
                    driver_version: required_text(columns[10], "driver_version", line_index)?,
                    vram_total_bytes: total,
                },
                occupancy: GpuOccupancy {
                    uuid,
                    vram_used_bytes: used,
                    vram_free_bytes: free,
                    utilization_gpu_percent: parse_optional(columns[7]),
                    temperature_c: parse_optional(columns[8]),
                    power_draw_w: parse_optional(columns[9]),
                },
            })
        })
        .collect()
}

fn required_text(value: &str, field: &str, line_index: usize) -> Result<String, String> {
    if is_missing(value) {
        Err(format!(
            "nvidia-smi row {} is missing {field}",
            line_index + 1
        ))
    } else {
        Ok(value.to_owned())
    }
}

fn parse_required<T: std::str::FromStr>(
    value: &str,
    field: &str,
    line_index: usize,
) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("nvidia-smi row {} has invalid {field}", line_index + 1))
}

fn parse_mib(value: &str, field: &str, line_index: usize) -> Result<u64, String> {
    parse_required::<u64>(value, field, line_index)?
        .checked_mul(MIB)
        .ok_or_else(|| format!("nvidia-smi row {} {field} overflows bytes", line_index + 1))
}

fn parse_optional<T: std::str::FromStr>(value: &str) -> Option<T> {
    (!is_missing(value)).then(|| value.parse().ok()).flatten()
}

fn is_missing(value: &str) -> bool {
    value.is_empty() || value.eq_ignore_ascii_case("n/a") || value == "[N/A]"
}

#[cfg(target_os = "windows")]
fn memory_sample() -> Result<MemorySample, String> {
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }

    let mut status = MemoryStatusEx {
        length: std::mem::size_of::<MemoryStatusEx>() as u32,
        memory_load: 0,
        total_phys: 0,
        avail_phys: 0,
        total_page_file: 0,
        avail_page_file: 0,
        total_virtual: 0,
        avail_virtual: 0,
        avail_extended_virtual: 0,
    };
    // SAFETY: `status` is writable, correctly sized, and lives for the call.
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return Err(format!(
            "GlobalMemoryStatusEx failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(MemorySample {
        total_bytes: status.total_phys,
        available_bytes: status.avail_phys,
    })
}

#[cfg(target_os = "linux")]
fn memory_sample() -> Result<MemorySample, String> {
    let text = std::fs::read_to_string("/proc/meminfo")
        .map_err(|error| format!("cannot read /proc/meminfo: {error}"))?;
    let kib = |name: &str| -> Result<u64, String> {
        text.lines()
            .find(|line| line.starts_with(name))
            .and_then(|line| line.split_ascii_whitespace().nth(1))
            .and_then(|value| value.parse::<u64>().ok())
            .and_then(|value| value.checked_mul(1024))
            .ok_or_else(|| format!("/proc/meminfo is missing {name}"))
    };
    Ok(MemorySample {
        total_bytes: kib("MemTotal:")?,
        available_bytes: kib("MemAvailable:")?,
    })
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn memory_sample() -> Result<MemorySample, String> {
    Err(format!(
        "memory probe is not implemented for {}",
        std::env::consts::OS
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_structured_nvidia_capability_and_occupancy() {
        let samples = parse_nvidia_csv(
            "0, GPU-a, NVIDIA GeForce RTX 3090, 00000000:21:00.0, 24576, 1024, 23552, 73, 58, 312.50, 596.21\n",
        )
        .expect("valid sample");
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].capability.uuid, "GPU-a");
        assert_eq!(samples[0].capability.vram_total_bytes, 24 * 1024 * MIB);
        assert_eq!(samples[0].occupancy.vram_used_bytes, 1024 * MIB);
        assert_eq!(samples[0].occupancy.utilization_gpu_percent, Some(73));
        assert_eq!(samples[0].occupancy.power_draw_w, Some(312.5));
    }

    #[test]
    fn rejects_ambiguous_raw_gpu_rows() {
        let error = parse_nvidia_csv("GPU-a, RTX 3090").expect_err("invalid sample");
        assert!(error.contains("expected 11"));
    }
}
