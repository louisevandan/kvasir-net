import Foundation
import UIKit
import Darwin

/// Best-effort device telemetry available to an unprivileged iOS app.
struct DeviceStats {
    var ramTotalMb: UInt64 = 0
    var ramUsedMb: UInt64 = 0
    var ramAvailMb: UInt64 = 0
    var cpuLoad: Float = 0        // 0..1 aggregate
    var thermal: Float = 0        // 0..1 (from ProcessInfo.thermalState)
    var thermalWord: String = "thermal.low"   // localization key, resolved in the view
    var charging: Bool = false
}

/// Reads aggregate CPU/RAM/thermal/battery on iOS via mach + ProcessInfo + UIDevice.
final class NodeTelemetry {
    static let shared = NodeTelemetry()
    private var lastBusy: UInt64 = 0
    private var lastTotal: UInt64 = 0

    func read() -> DeviceStats {
        var s = DeviceStats()
        s.ramTotalMb = ProcessInfo.processInfo.physicalMemory / (1024 * 1024)
        let (usedMb, availMb) = memory()
        s.ramUsedMb = usedMb
        s.ramAvailMb = availMb
        s.cpuLoad = cpuLoad()

        switch ProcessInfo.processInfo.thermalState {
        case .nominal:  s.thermal = 0.25; s.thermalWord = "thermal.low"
        case .fair:     s.thermal = 0.50; s.thermalWord = "thermal.medium"
        case .serious:  s.thermal = 0.80; s.thermalWord = "thermal.high"
        case .critical: s.thermal = 1.00; s.thermalWord = "thermal.critical"
        @unknown default: s.thermal = 0.25; s.thermalWord = "thermal.low"
        }

        UIDevice.current.isBatteryMonitoringEnabled = true
        let bs = UIDevice.current.batteryState
        s.charging = (bs == .charging || bs == .full)
        return s
    }

    /// Aggregate CPU busy fraction from mach host_cpu_load_info tick deltas.
    private func cpuLoad() -> Float {
        var info = host_cpu_load_info()
        var count = mach_msg_type_number_t(MemoryLayout<host_cpu_load_info>.stride / MemoryLayout<integer_t>.stride)
        let kr = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                host_statistics(mach_host_self(), HOST_CPU_LOAD_INFO, $0, &count)
            }
        }
        guard kr == KERN_SUCCESS else { return 0 }
        let user = UInt64(info.cpu_ticks.0)
        let system = UInt64(info.cpu_ticks.1)
        let idle = UInt64(info.cpu_ticks.2)
        let nice = UInt64(info.cpu_ticks.3)
        let busy = user + system + nice
        let total = busy + idle
        defer { lastBusy = busy; lastTotal = total }
        let dBusy = busy &- lastBusy
        let dTotal = total &- lastTotal
        guard lastTotal > 0, dTotal > 0 else { return 0 }
        return min(max(Float(dBusy) / Float(dTotal), 0), 1)
    }

    /// (usedMb, availMb) from mach VM statistics; used = active + wired + compressed.
    private func memory() -> (UInt64, UInt64) {
        var stats = vm_statistics64()
        var count = mach_msg_type_number_t(MemoryLayout<vm_statistics64>.stride / MemoryLayout<integer_t>.stride)
        let kr = withUnsafeMutablePointer(to: &stats) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                host_statistics64(mach_host_self(), HOST_VM_INFO64, $0, &count)
            }
        }
        guard kr == KERN_SUCCESS else { return (0, 0) }
        let page = UInt64(vm_kernel_page_size)
        let used = (UInt64(stats.active_count) + UInt64(stats.wire_count)
            + UInt64(stats.compressor_page_count)) * page
        let free = (UInt64(stats.free_count) + UInt64(stats.inactive_count)) * page
        return (used / (1024 * 1024), free / (1024 * 1024))
    }
}

/// Estimated resource profile for a (backend, mode) choice on Apple Silicon.
/// iOS GPU path is **MLX** (Metal) — not OpenCL/Vulkan (which are the Android/Adreno path).
struct NodeProfile {
    let computeUnit: String
    let memImpact: Float   // 0..1
    let thermal: Float     // 0..1
    let performance: Float // 0..1
    let tokPerSec: Int     // decode tok/s (qwen2.5-0.5B Q8_0, estimated on Apple Silicon)
    let note: String
    let estimated: Bool    // iOS numbers are estimates, not on-device measured
}

@MainActor
func nodeProfile(backend: String, mode: String) -> NodeProfile {
    let local = mode == "local_shard"
    switch backend {
    case "mlx":
        return NodeProfile(
            computeUnit: "GPU · Apple Silicon (MLX/Metal)",
            memImpact: 0.50, thermal: 0.55,
            performance: local ? 0.92 : 0.45, tokPerSec: local ? 110 : 40,
            note: Localizer.shared.t("nodeProfile.mlxNote"),
            estimated: true)
    default: // cpu
        return NodeProfile(
            computeUnit: "CPU · Apple Silicon (Accelerate)",
            memImpact: 0.35, thermal: 0.70,
            performance: local ? 0.75 : 0.40, tokPerSec: local ? 85 : 33,
            note: Localizer.shared.t("nodeProfile.cpuNote"),
            estimated: true)
    }
}

/// Backends selectable on iOS (GPU = MLX, plus CPU).
enum NodeBackend {
    static let all: [(id: String, label: String)] = [
        ("mlx", "GPU · MLX"),
        ("cpu", "CPU"),
    ]
    /// Accelerator category reported to the hub for reward/monitor grouping.
    static func accelerator(_ backend: String) -> String { backend == "cpu" ? "cpu" : "gpu" }
}
