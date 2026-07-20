import SwiftUI
import Core

/// App-wide owner of the phone-node lifecycle.
///
/// The staking screens' `StakingStore` is created per-sheet and does not exist on
/// the home screen, so the node's worker, hub participation and staking-service
/// heartbeat cannot live there — they must run app-wide so that:
///  - a live node stays "online" no matter which screen is shown,
///  - once the live toggle is on, the node auto-resumes participation on cold start,
///  - iOS suspending a backgrounded app (which freezes the worker + heartbeat and
///    drops the node to idle) is recovered on every return to the foreground.
///
/// It reads the same persisted toggles `StakingStore` writes, so the UI stays the
/// single place the operator flips state; this object just enacts it app-wide.
@MainActor
final class NodePresence {
    static let shared = NodePresence()

    private weak var wallet: WalletStore?
    private var heartbeatTask: Task<Void, Never>?
    private var installedForegroundObserver = false

    private init() {}

    /// Wire up the wallet (owner address + staking URL) and, once, the foreground
    /// observer. Safe to call repeatedly — StakingStore calls it on creation and the
    /// home screen calls it on appear, so the node resumes even with no sheet open.
    func attach(wallet: WalletStore) {
        self.wallet = wallet
        if !installedForegroundObserver {
            installedForegroundObserver = true
            NotificationCenter.default.addObserver(
                forName: UIApplication.didBecomeActiveNotification, object: nil, queue: .main
            ) { [weak self] _ in Task { @MainActor in self?.refresh() } }
        }
        refresh()
    }

    // MARK: persisted state (mirrors StakingStore's keys)

    private var d: UserDefaults { .standard }
    private var nodeLive: Bool { d.bool(forKey: "node.live") }
    private var nodeMode: String { d.string(forKey: "node.mode") ?? "local_shard" }
    private var nodeBackend: String { d.string(forKey: "node.backend") ?? "mlx" }
    private var chargingOnly: Bool { d.object(forKey: "node.chargingOnly") as? Bool ?? false }
    private var owner: String { wallet?.address ?? "" }
    private var service: StakingService? {
        guard let url = wallet?.stakingServiceURL else { return nil }
        return StakingService(baseURL: url)
    }

    /// (Re)evaluate node state and drive the worker, participation and heartbeat to
    /// match the persisted live toggle + charge state. Idempotent — the underlying
    /// singletons guard against double start/stop.
    func refresh() {
        let charging = NodeTelemetry.shared.read().charging
        let blockedByCharge = nodeLive && chargingOnly && !charging
        let wantServe = nodeLive && nodeMode == "rpc_worker"   // inbound RPC-worker path

        UIApplication.shared.isIdleTimerDisabled = nodeLive && !blockedByCharge

        if wantServe && !blockedByCharge {
            AgentControlServer.shared.start(owner: owner, deviceName: DeviceInfo.nodeId)
            NodeBackgroundTask.schedule()   // ask iOS for background compute windows
        } else {
            AgentControlServer.shared.stop()
            if !wantServe { NodeBackgroundTask.cancel() }
        }

        // Outbound participation: poll every registered remote hub and serve a scarce
        // shard/expert range — how a NAT'd phone serves hub.kvasir-ai.net outbound.
        if nodeLive && !blockedByCharge {
            HubParticipation.shared.start(owner: owner)
        } else {
            HubParticipation.shared.stop()
        }

        // Registration + heartbeat keeps the node "online" while live (off-charge it
        // stays registered-but-paused rather than dropping); the loop also resumes the
        // worker when the cable is plugged back in.
        if nodeLive { startHeartbeat() } else { stopHeartbeat() }
    }

    // MARK: heartbeat loop (single, app-wide)

    private func startHeartbeat() {
        guard heartbeatTask == nil else { return }
        heartbeatTask = Task { [weak self] in
            guard let self else { return }
            await self.register()
            var lastCharging = NodeTelemetry.shared.read().charging
            while !Task.isCancelled && self.nodeLive {
                let charging = NodeTelemetry.shared.read().charging
                if charging != lastCharging { lastCharging = charging; self.refresh() }
                await self.heartbeat()
                try? await Task.sleep(for: .seconds(1.5))
            }
            self.heartbeatTask = nil
        }
    }

    private func stopHeartbeat() {
        heartbeatTask?.cancel(); heartbeatTask = nil
    }

    private func register() async {
        guard let service, !owner.isEmpty else { return }
        let p = nodeProfile(backend: nodeBackend, mode: nodeMode)
        _ = try? await service.registerNode(
            nodeId: DeviceInfo.nodeId, owner: owner,
            os: "ios", deviceKind: DeviceInfo.kind,
            accelerator: NodeBackend.accelerator(nodeBackend),
            label: "\(DeviceInfo.label) · \(nodeMode)",
            perfScore: Double(p.tokPerSec), backend: nodeBackend, mode: nodeMode)
    }

    private func heartbeat() async {
        guard let service else { return }
        _ = try? await service.heartbeat(nodeId: DeviceInfo.nodeId)
    }
}
