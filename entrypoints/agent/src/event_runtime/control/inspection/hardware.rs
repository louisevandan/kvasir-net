//! Platform-owned hardware observation for the agent inspection snapshot.

use serde::Serialize;
use serde_json::{Value, json};
use std::process::Command;

const MIB: u64 = 1024 * 1024;

#[derive(Debug, Serialize)]
struct GpuCapability {
    index: u32,
    uuid: String,
    vendor: String,
    name: String,
    backend: String,
    memory_kind: &'static str,
    pci_bus_id: Option<String>,
    driver_version: Option<String>,
    memory_total_bytes: Option<u64>,
    // Kept for schema-1 readers. Unified-memory devices deliberately report
    // null here instead of presenting system RAM as dedicated VRAM.
    vram_total_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
struct GpuOccupancy {
    uuid: String,
    memory_used_bytes: Option<u64>,
    memory_free_bytes: Option<u64>,
    // Kept for schema-1 readers; null for unified memory.
    vram_used_bytes: Option<u64>,
    vram_free_bytes: Option<u64>,
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

#[derive(Debug)]
struct GpuProbe {
    source: &'static str,
    result: Result<Vec<GpuSample>, ProbeFailure>,
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
    let gpus = gpu_probes();
    build_observation(memory, gpus)
}

pub(super) fn failed(detail: String) -> HardwareObservation {
    build_observation(
        Err(detail.clone()),
        vec![GpuProbe {
            source: "agent",
            result: Err(ProbeFailure::Error(detail)),
        }],
    )
}

fn build_observation(
    memory: Result<MemorySample, String>,
    gpu_probes: Vec<GpuProbe>,
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

    let mut gpu_capabilities = Vec::new();
    let mut gpu_occupancies = Vec::new();
    let mut gpu_probe = Vec::new();
    for probe in gpu_probes {
        match probe.result {
            Ok(samples) => {
                gpu_capabilities.extend(samples.iter().map(|gpu| json!(&gpu.capability)));
                gpu_occupancies.extend(samples.iter().map(|gpu| json!(&gpu.occupancy)));
                gpu_probe.push(
                    json!({"source":probe.source,"state":"available","detail":null,
                    "devices":samples.len()}),
                );
            }
            Err(ProbeFailure::Unavailable(detail)) => gpu_probe.push(
                json!({"source":probe.source,"state":"unavailable","detail":detail,"devices":0}),
            ),
            Err(ProbeFailure::Error(detail)) => gpu_probe
                .push(json!({"source":probe.source,"state":"error","detail":detail,"devices":0})),
        }
    }
    let available_devices = gpu_probe
        .iter()
        .filter_map(|probe| probe["devices"].as_u64())
        .sum::<u64>();
    let gpu_state = if available_devices > 0 {
        "available"
    } else if gpu_probe.iter().any(|probe| probe["state"] == "error") {
        "error"
    } else {
        "unavailable"
    };
    let gpu_probe = json!({
        "source":"provider-aggregate",
        "state":gpu_state,
        "detail":null,
        "sources":gpu_probe,
    });

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

fn gpu_probes() -> Vec<GpuProbe> {
    let probes = vec![GpuProbe {
        source: "nvidia-smi",
        result: nvidia_samples(),
    }];
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let mut probes = probes;
    #[cfg(target_os = "linux")]
    probes.push(GpuProbe {
        source: "linux-drm-sysfs",
        result: linux_amd_samples(),
    });
    #[cfg(target_os = "macos")]
    probes.push(GpuProbe {
        source: "system_profiler",
        result: apple_gpu_samples(),
    });
    probes
}

fn nvidia_samples() -> Result<Vec<GpuSample>, ProbeFailure> {
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
                    vendor: "NVIDIA".into(),
                    name: required_text(columns[2], "name", line_index)?,
                    backend: "cuda".into(),
                    memory_kind: "dedicated",
                    pci_bus_id: Some(required_text(columns[3], "pci.bus_id", line_index)?),
                    driver_version: Some(required_text(columns[10], "driver_version", line_index)?),
                    memory_total_bytes: Some(total),
                    vram_total_bytes: Some(total),
                },
                occupancy: GpuOccupancy {
                    uuid,
                    memory_used_bytes: Some(used),
                    memory_free_bytes: Some(free),
                    vram_used_bytes: Some(used),
                    vram_free_bytes: Some(free),
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

#[cfg(target_os = "linux")]
fn linux_amd_samples() -> Result<Vec<GpuSample>, ProbeFailure> {
    use std::path::Path;

    let drm = Path::new("/sys/class/drm");
    let entries = std::fs::read_dir(drm).map_err(|error| {
        ProbeFailure::Unavailable(format!("cannot read {}: {error}", drm.display()))
    })?;
    let mut samples = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let card = file_name.to_string_lossy();
        let Some(index) = card
            .strip_prefix("card")
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        let device = entry.path().join("device");
        if read_trimmed(device.join("vendor")).as_deref() != Some("0x1002") {
            continue;
        }
        let canonical = std::fs::canonicalize(&device).ok();
        let pci_bus_id = canonical
            .as_deref()
            .and_then(|path| path.file_name())
            .map(|value| value.to_string_lossy().into_owned());
        let unique_id = read_trimmed(device.join("unique_id"));
        let uuid = format!(
            "AMD-{}",
            unique_id
                .as_deref()
                .or(pci_bus_id.as_deref())
                .unwrap_or(card.as_ref())
        );
        let total = read_u64(device.join("mem_info_vram_total"));
        let used = read_u64(device.join("mem_info_vram_used"));
        let free = total
            .zip(used)
            .map(|(total, used)| total.saturating_sub(used));
        let name = read_trimmed(device.join("product_name"))
            .or_else(|| read_uevent_value(&device, "PCI_ID"))
            .unwrap_or_else(|| "AMD GPU".into());
        let driver_version = std::fs::read_link(device.join("driver"))
            .ok()
            .and_then(|path| {
                path.file_name()
                    .map(|value| value.to_string_lossy().into_owned())
            });
        samples.push(GpuSample {
            capability: GpuCapability {
                index,
                uuid: uuid.clone(),
                vendor: "AMD".into(),
                name,
                backend: "rocm".into(),
                memory_kind: "dedicated",
                pci_bus_id,
                driver_version,
                memory_total_bytes: total,
                vram_total_bytes: total,
            },
            occupancy: GpuOccupancy {
                uuid,
                memory_used_bytes: used,
                memory_free_bytes: free,
                vram_used_bytes: used,
                vram_free_bytes: free,
                utilization_gpu_percent: read_u32(device.join("gpu_busy_percent")),
                temperature_c: None,
                power_draw_w: None,
            },
        });
    }
    if samples.is_empty() {
        Err(ProbeFailure::Unavailable(
            "no AMD DRM device with vendor 0x1002".into(),
        ))
    } else {
        samples.sort_by_key(|sample| sample.capability.index);
        Ok(samples)
    }
}

#[cfg(target_os = "linux")]
fn read_trimmed(path: impl AsRef<std::path::Path>) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(target_os = "linux")]
fn read_u64(path: impl AsRef<std::path::Path>) -> Option<u64> {
    read_trimmed(path)?.parse().ok()
}

#[cfg(target_os = "linux")]
fn read_u32(path: impl AsRef<std::path::Path>) -> Option<u32> {
    read_trimmed(path)?.parse().ok()
}

#[cfg(target_os = "linux")]
fn read_uevent_value(device: &std::path::Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(device.join("uevent")).ok()?;
    text.lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .map(str::to_owned)
}

#[cfg(target_os = "macos")]
fn apple_gpu_samples() -> Result<Vec<GpuSample>, ProbeFailure> {
    let output = Command::new("system_profiler")
        .args(["SPDisplaysDataType", "-json"])
        .output()
        .map_err(|error| {
            ProbeFailure::Unavailable(format!("system_profiler unavailable: {error}"))
        })?;
    if !output.status.success() {
        return Err(ProbeFailure::Error(format!(
            "system_profiler exited with {}",
            output.status
        )));
    }
    let root: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| ProbeFailure::Error(format!("invalid system_profiler JSON: {error}")))?;
    let displays = root
        .get("SPDisplaysDataType")
        .and_then(Value::as_array)
        .ok_or_else(|| ProbeFailure::Error("system_profiler omitted SPDisplaysDataType".into()))?;
    let memory = memory_sample().ok();
    let mut samples = Vec::new();
    for (index, display) in displays.iter().enumerate() {
        let name = display
            .get("sppci_model")
            .or_else(|| display.get("_name"))
            .and_then(Value::as_str)
            .unwrap_or("Apple GPU")
            .to_owned();
        let vendor = display
            .get("spdisplays_vendor")
            .and_then(Value::as_str)
            .unwrap_or("Apple");
        if !vendor.to_ascii_lowercase().contains("apple")
            && !name.to_ascii_lowercase().contains("apple")
        {
            continue;
        }
        let device_id = display
            .get("spdisplays_device-id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| index.to_string());
        let uuid = format!("APPLE-{device_id}");
        samples.push(GpuSample {
            capability: GpuCapability {
                index: index as u32,
                uuid: uuid.clone(),
                vendor: "Apple".into(),
                name,
                backend: "metal".into(),
                memory_kind: "unified",
                pci_bus_id: None,
                driver_version: display
                    .get("spdisplays_metal")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                memory_total_bytes: memory.as_ref().map(|sample| sample.total_bytes),
                vram_total_bytes: None,
            },
            occupancy: GpuOccupancy {
                uuid,
                memory_used_bytes: memory
                    .as_ref()
                    .map(|sample| sample.total_bytes.saturating_sub(sample.available_bytes)),
                memory_free_bytes: memory.as_ref().map(|sample| sample.available_bytes),
                vram_used_bytes: None,
                vram_free_bytes: None,
                utilization_gpu_percent: None,
                temperature_c: None,
                power_draw_w: None,
            },
        });
    }
    if samples.is_empty() {
        Err(ProbeFailure::Unavailable(
            "system_profiler reported no Apple GPU".into(),
        ))
    } else {
        Ok(samples)
    }
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

#[cfg(target_os = "macos")]
fn memory_sample() -> Result<MemorySample, String> {
    let total = Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .map_err(|error| format!("sysctl unavailable: {error}"))?;
    if !total.status.success() {
        return Err(format!("sysctl hw.memsize exited with {}", total.status));
    }
    let total_bytes = String::from_utf8_lossy(&total.stdout)
        .trim()
        .parse::<u64>()
        .map_err(|_| "sysctl hw.memsize returned a non-integer".to_owned())?;
    let vm = Command::new("vm_stat")
        .output()
        .map_err(|error| format!("vm_stat unavailable: {error}"))?;
    if !vm.status.success() {
        return Err(format!("vm_stat exited with {}", vm.status));
    }
    let text = String::from_utf8_lossy(&vm.stdout);
    let page_bytes = text
        .lines()
        .next()
        .and_then(|line| line.split("page size of ").nth(1))
        .and_then(|tail| tail.split_ascii_whitespace().next())
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| "vm_stat omitted page size".to_owned())?;
    let pages = |key: &str| -> u64 {
        text.lines()
            .find_map(|line| line.strip_prefix(key))
            .and_then(|tail| tail.trim().trim_end_matches('.').parse::<u64>().ok())
            .unwrap_or(0)
    };
    let available_pages = [
        "Pages free:",
        "Pages inactive:",
        "Pages speculative:",
        "Pages purgeable:",
    ]
    .iter()
    .map(|key| pages(key))
    .sum::<u64>();
    Ok(MemorySample {
        total_bytes,
        available_bytes: available_pages.saturating_mul(page_bytes).min(total_bytes),
    })
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
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
        assert_eq!(samples[0].capability.backend, "cuda");
        assert_eq!(samples[0].capability.memory_kind, "dedicated");
        assert_eq!(
            samples[0].capability.vram_total_bytes,
            Some(24 * 1024 * MIB)
        );
        assert_eq!(samples[0].occupancy.vram_used_bytes, Some(1024 * MIB));
        assert_eq!(samples[0].occupancy.utilization_gpu_percent, Some(73));
        assert_eq!(samples[0].occupancy.power_draw_w, Some(312.5));
    }

    #[test]
    fn rejects_ambiguous_raw_gpu_rows() {
        let error = parse_nvidia_csv("GPU-a, RTX 3090").expect_err("invalid sample");
        assert!(error.contains("expected 11"));
    }

    #[test]
    fn preserves_non_nvidia_unified_memory_semantics() {
        let sample = GpuSample {
            capability: GpuCapability {
                index: 0,
                uuid: "APPLE-0".into(),
                vendor: "Apple".into(),
                name: "Apple M4 Pro".into(),
                backend: "metal".into(),
                memory_kind: "unified",
                pci_bus_id: None,
                driver_version: Some("Metal 4".into()),
                memory_total_bytes: Some(64 * 1024 * MIB),
                vram_total_bytes: None,
            },
            occupancy: GpuOccupancy {
                uuid: "APPLE-0".into(),
                memory_used_bytes: Some(16 * 1024 * MIB),
                memory_free_bytes: Some(48 * 1024 * MIB),
                vram_used_bytes: None,
                vram_free_bytes: None,
                utilization_gpu_percent: None,
                temperature_c: None,
                power_draw_w: None,
            },
        };
        let observation = build_observation(
            Ok(MemorySample {
                total_bytes: 64 * 1024 * MIB,
                available_bytes: 48 * 1024 * MIB,
            }),
            vec![GpuProbe {
                source: "system_profiler",
                result: Ok(vec![sample]),
            }],
        )
        .with_adapters(["llamacpp"]);
        assert_eq!(observation["capability"]["gpus"][0]["backend"], "metal");
        assert_eq!(
            observation["capability"]["gpus"][0]["memory_kind"],
            "unified"
        );
        assert!(observation["capability"]["gpus"][0]["vram_total_bytes"].is_null());
        assert_eq!(observation["probes"]["gpus"]["sources"][0]["devices"], 1);
    }
}
