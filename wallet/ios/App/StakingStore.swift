import Foundation
import SwiftUI
import Core

/// Drives the staking + node-rewards screens. Uses Core.StakingService for the
/// off-chain settlement API and WalletStore for the on-chain KVR transfer.
@MainActor
final class StakingStore: ObservableObject {
    @Published var config: StakingConfig?
    @Published var position: StakePosition?
    @Published var nodeRewards: NodeRewards?
    @Published var nodeStatus: NodeStatus?
    @Published var busy = false
    @Published var message: String?
    @Published var lastSignature: String?

    // MARK: mobile-phone node config (persisted)
    @Published var nodeBackend: String { didSet { UserDefaults.standard.set(nodeBackend, forKey: "node.backend") } }
    @Published var nodeMode: String { didSet { UserDefaults.standard.set(nodeMode, forKey: "node.mode"); syncAgentServer() } }
    @Published var nodeChargingOnly: Bool { didSet { UserDefaults.standard.set(nodeChargingOnly, forKey: "node.chargingOnly"); syncAgentServer() } }
    @Published var nodeLive: Bool { didSet { UserDefaults.standard.set(nodeLive, forKey: "node.live"); syncAgentServer() } }

    /// Why the node isn't actively serving right now, if live is on (for the UI).
    @Published var nodePausedReason: String?

    /// Whether the node should be serving, readable without a store instance
    /// (the background task handler has no view context). Backed by the same
    /// persisted toggles syncAgentServer reads.
    static func nodeIsLive() -> Bool {
        let d = UserDefaults.standard
        return d.bool(forKey: "node.live") && (d.string(forKey: "node.mode") ?? "") == "rpc_worker"
    }

    /// The RPC worker + agent control server live app-wide, not per-screen: they
    /// follow the persisted live toggle, so leaving the settings view (or a cold
    /// app start with the toggle on) keeps the node serving while the app is
    /// foregrounded.
    ///
    /// v1 operating condition: iOS suspends a foregrounded app when the screen
    /// locks, which kills the worker. While serving we therefore keep the screen
    /// awake (idle timer disabled) and, if "charging only" is set, pause when the
    /// device is off charge. The node runs reliably while plugged in with the
    /// screen on — the documented v1 constraint.
    func syncAgentServer() {
        let charging = NodeTelemetry.shared.read().charging
        let blockedByCharge = nodeLive && nodeChargingOnly && !charging
        nodePausedReason = blockedByCharge ? Localizer.shared.t("nodeSettings.pausedOffCharge") : nil
        // The node's worker, hub participation and staking heartbeat are owned
        // app-wide by NodePresence — StakingStore is created per-sheet and doesn't
        // exist on the home screen, so a live node stays online off this screen and
        // resumes on cold start / every return to the foreground. Persisting the
        // live toggle (above) is what makes the resume automatic.
        NodePresence.shared.refresh()
    }

    /// Sign in to a remote hub with the wallet (SIWS node token, no OTP) and
    /// register it for the node to poll/serve. Returns a UI status message.
    func connectHub(url: String) async -> String {
        let u = url.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !u.isEmpty else { return Localizer.shared.t("nodeSettings.hubUrlMissing") }
        guard nodeLive else { return Localizer.shared.t("nodeSettings.enableLiveFirst") }
        guard let phrase = wallet.revealMnemonic() else { return Localizer.shared.t("nodeSettings.walletLocked") }
        do {
            let token = try await HubAuthService(baseUrl: u, mnemonic: phrase).nodeToken()
            HubParticipation.shared.registerHub(url: u, token: token)
            HubParticipation.shared.start(owner: owner ?? "")
            return Localizer.shared.t("nodeSettings.hubConnected")
        } catch {
            return "\(Localizer.shared.t("nodeSettings.hubConnectFailed")): \(error)"
        }
    }

    /// Discard the stored credit API key and mint a new one. Returns a UI status
    /// message.
    ///
    /// Only the key's hash is kept by the gateway, so a key it no longer
    /// recognises cannot be repaired by asking for it back — it has to be
    /// reminted. Without this, "invalid API key" has no way out from inside the
    /// app. Credit balance is held against the wallet, not the key, so nothing
    /// is lost by reissuing.
    func reissueCreditApiKey() async -> String {
        guard let w = wallet.address else { return Localizer.shared.t("nodeSettings.walletLocked") }
        guard let phrase = wallet.revealMnemonic() else { return Localizer.shared.t("nodeSettings.walletLocked") }
        KeyStore.deleteApiKey(wallet: w)
        do {
            let credit = CreditService(baseUrl: wallet.stakingServiceURL)
            try await credit.register(mnemonic: phrase)                   // self-whitelist (idempotent)
            let key = try await credit.mintApiKey(mnemonic: phrase, label: "Kvasir iOS")
            try? KeyStore.saveApiKey(key, wallet: w)
            return Localizer.shared.t("apiKey.ok")
        } catch {
            return "\(Localizer.shared.t("apiKey.failed")): \(error)"
        }
    }

    private unowned let wallet: WalletStore
    private var service: StakingService?

    init(wallet: WalletStore) {
        self.wallet = wallet
        self.service = StakingService(baseURL: wallet.stakingServiceURL)
        let d = UserDefaults.standard
        self.nodeBackend = d.string(forKey: "node.backend") ?? "mlx"
        self.nodeMode = d.string(forKey: "node.mode") ?? "local_shard"
        self.nodeChargingOnly = d.object(forKey: "node.chargingOnly") as? Bool ?? false
        self.nodeLive = d.bool(forKey: "node.live")
        // Make sure the app-wide node lifecycle owner has the wallet (for the owner
        // address + staking URL); it also installs the foreground-resume observer.
        NodePresence.shared.attach(wallet: wallet)
    }

    /// The perf profile for the currently selected backend/mode.
    var nodeProfileCurrent: NodeProfile { nodeProfile(backend: nodeBackend, mode: nodeMode) }

    var owner: String? { wallet.address }
    var serviceURL: String { wallet.stakingServiceURL }

    func updateServiceURL(_ url: String) {
        wallet.setStakingServiceURL(url)
        service = StakingService(baseURL: wallet.stakingServiceURL)
    }

    func explorerURL(sig: String) -> URL? { wallet.explorerURL(sig: sig) }

    /// Genesis discovery: fetch the gateway's `/api/config` and, if it advertises a
    /// public URL and the user hasn't pinned a custom one, adopt it (mirrors the
    /// desktop app). Also caches the config so the settings screen can display the
    /// genesis facts. Silent on failure — an unreachable gateway keeps the current URL.
    func discoverGenesis() async {
        guard let svc = StakingService(baseURL: wallet.stakingServiceURL),
              let cfg = try? await svc.config() else { return }
        config = cfg
        guard let pub = cfg.publicUrl?.trimmingCharacters(in: .whitespacesAndNewlines),
              !pub.isEmpty, pub != wallet.stakingServiceURL, !wallet.hasCustomStakingURL else { return }
        // Only adopt an advertised public URL that is actually reachable — a gateway
        // that advertises a not-yet-live domain (e.g. DNS/ports pending) must not
        // strand the client on an unreachable URL.
        guard let pubSvc = StakingService(baseURL: pub), (try? await pubSvc.config()) != nil else { return }
        wallet.setStakingServiceURL(pub)
        service = StakingService(baseURL: pub)
    }

    func load() async {
        await discoverGenesis()
        guard let service, let owner else {
            message = Localizer.shared.t("error.setStakingUrl")
            return
        }
        do {
            config = try await service.config()
            position = try await service.position(owner: owner)
            nodeRewards = try await service.nodeRewards(owner: owner)
            message = nil
        } catch {
            message = String(describing: error)
        }
    }

    func loadStatus() async {
        guard let service, let owner else {
            message = Localizer.shared.t("error.setStakingUrl")
            return
        }
        do {
            nodeStatus = try await service.nodeStatus(owner: owner)
            message = nil
        } catch {
            message = String(describing: error)
        }
    }

    func stake(amount: Double) async {
        guard let service, let owner, let config else { return }
        busy = true; defer { busy = false }
        message = nil; lastSignature = nil
        do {
            // 1) on-chain: send KVR to the vault owner (SolanaSwift derives the vault ATA)
            let sig = try await wallet.sendToken(to: config.vaultOwner, amount: amount)
            lastSignature = sig
            // 2) settle off-chain
            position = try await service.stake(owner: owner, amount: amount, signature: sig)
            await wallet.refresh()
        } catch {
            message = String(describing: error)
        }
    }

    func unstake(amount: Double?) async {
        guard let service, let owner else { return }
        busy = true; defer { busy = false }
        message = nil; lastSignature = nil
        do {
            let r = try await service.unstake(owner: owner, amount: amount)
            lastSignature = r.signature
            position = try await service.position(owner: owner)
            await wallet.refresh()
        } catch {
            message = String(describing: error)
        }
    }

    func registerNode(_ nodeId: String) async {
        guard let service, let owner else { return }
        busy = true; defer { busy = false }
        message = nil
        do {
            _ = try await service.registerNode(nodeId: nodeId, owner: owner)
            nodeRewards = try await service.nodeRewards(owner: owner)
        } catch {
            message = String(describing: error)
        }
    }

    /// Register the current iOS device as a node under the account and heartbeat.
    /// Reports the selected backend's measured throughput so the hub can tier it.
    func connectThisDevice() async {
        guard let service, let owner else { return }
        busy = true; defer { busy = false }
        message = nil
        let p = nodeProfileCurrent
        do {
            _ = try await service.registerNode(
                nodeId: DeviceInfo.nodeId, owner: owner,
                os: "ios", deviceKind: DeviceInfo.kind,
                accelerator: NodeBackend.accelerator(nodeBackend), label: DeviceInfo.label,
                perfScore: Double(p.tokPerSec), backend: nodeBackend, mode: nodeMode)
            try await service.heartbeat(nodeId: DeviceInfo.nodeId)
            await loadStatus()
        } catch {
            message = String(describing: error)
        }
    }

    /// Register this device with the current node-settings profile (called when the
    /// live toggle turns on). Silent on failure so the settings UI stays responsive.
    func registerDeviceNodeLive() async {
        guard let service, let owner else { return }
        let p = nodeProfileCurrent
        _ = try? await service.registerNode(
            nodeId: DeviceInfo.nodeId, owner: owner,
            os: "ios", deviceKind: DeviceInfo.kind,
            accelerator: NodeBackend.accelerator(nodeBackend),
            label: "\(DeviceInfo.label) · \(nodeMode)",
            perfScore: Double(p.tokPerSec), backend: nodeBackend, mode: nodeMode)
    }

    /// Lightweight heartbeat used by the live gauge loop.
    func heartbeatDeviceLive() async {
        guard let service else { return }
        _ = try? await service.heartbeat(nodeId: DeviceInfo.nodeId)
    }

    /// Remove a registered node from the account, then refresh the monitor.
    func removeNode(_ nodeId: String) async {
        guard let service, let owner else { return }
        busy = true; defer { busy = false }
        message = nil
        do {
            try await service.removeNode(nodeId: nodeId, owner: owner)
            await loadStatus()
        } catch {
            message = String(describing: error)
        }
    }

    func claim() async {
        guard let service, let owner else { return }
        busy = true; defer { busy = false }
        message = nil; lastSignature = nil
        do {
            let r = try await service.claimNodeRewards(owner: owner)
            lastSignature = r.signature
            nodeRewards = try await service.nodeRewards(owner: owner)
            await wallet.refresh()
        } catch {
            message = String(describing: error)
        }
    }
}
