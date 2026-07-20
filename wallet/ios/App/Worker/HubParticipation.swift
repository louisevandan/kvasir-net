import Foundation
import UIKit

/// Outbound hub participation for iOS — the half of the node that `AgentControlServer`
/// (inbound only) lacks. Mirrors `wallet/android/.../NodeAgentServer.kt`'s
/// `shardPollLoop` + `selfEnroll`: the phone learns remote public hubs (with a
/// SIWS node token), polls each hub's demand market outbound, and when a hub
/// offers an under-covered layer window it self-enrolls — pulls the partial
/// shard, bridges the ring stream over a WebSocket relay, and starts the native
/// ring stage. This is what lets a NAT'd phone serve a hub it can only reach
/// outbound (e.g. hub.kvasir-ai.net behind Cloudflare).
@MainActor
final class HubParticipation: ObservableObject {
    static let shared = HubParticipation()

    // base URL -> node token ("" = LAN hub, origin-authenticated)
    @Published private(set) var hubs: [String] = []
    @Published private(set) var lastStatus: String = ""
    // Whether the outbound poll loop is live (the node is "connected" to its known
    // hubs and volunteering), surfaced on the node dashboard.
    @Published private(set) var connected: Bool = false
    // Host of the hub the node is currently serving a shard/expert range for, if any.
    @Published private(set) var servingHost: String = ""

    private var knownHubs: [String: String] = [:]
    private var pollTask: Task<Void, Never>?
    private var owner: String = ""
    private var relay: RingRelay?
    private var enrolling = false
    private let session = URLSession(configuration: .ephemeral)
    private let maxLayers = 8   // layer budget offered; the hub clips it to the scarce gap
    private let maxExperts = 32          // experts/layer budget for the MoE expert path
    private let expertServePort = 52800
    private var expertRelay: ExpertRelayDial?
    private let defaults = UserDefaults.standard
    private let storeKey = "kvasir.knownHubs"

    private init() { loadHubs() }

    // MARK: registry

    func knownHubList() -> [String] { Array(knownHubs.keys) }

    /// Register a remote hub (with a wallet-auth node token) for the node to poll.
    /// Persisted so the node keeps polling it across launches.
    func registerHub(url: String, token: String) {
        guard let base = normalize(url) else { return }
        knownHubs[base] = token
        hubs = knownHubList()
        saveHubs()
        log("hub registered: \(base)\(token.isEmpty ? "" : " (auth)")")
    }

    func removeHub(_ url: String) {
        guard let base = normalize(url) else { return }
        knownHubs.removeValue(forKey: base)
        hubs = knownHubList()
        saveHubs()
    }

    private func normalize(_ url: String) -> String? {
        var b = url.trimmingCharacters(in: .whitespacesAndNewlines)
        if let r = b.range(of: "/api/") { b = String(b[b.startIndex..<r.lowerBound]) }
        while b.hasSuffix("/") { b.removeLast() }
        return b.hasPrefix("http") ? b : nil
    }

    private func loadHubs() {
        if let data = defaults.data(forKey: storeKey),
           let m = try? JSONDecoder().decode([String: String].self, from: data) {
            knownHubs = m
        }
        hubs = knownHubList()
    }
    private func saveHubs() {
        if let data = try? JSONEncoder().encode(knownHubs) { defaults.set(data, forKey: storeKey) }
    }

    // MARK: poll lifecycle

    func start(owner: String) {
        self.owner = owner
        guard pollTask == nil else { return }
        connected = !knownHubs.isEmpty
        if lastStatus.isEmpty { lastStatus = connected ? statusText("연결됨 · 대기") : "" }
        pollTask = Task.detached(priority: .background) { [weak self] in
            await self?.pollLoop()
        }
    }

    func stop() {
        pollTask?.cancel(); pollTask = nil
        relay?.stop(); relay = nil
        expertRelay?.stop(); expertRelay = nil
        _ = kvasir_stage_request_stop()
        connected = false
        servingHost = ""
        lastStatus = ""
    }

    private func pollLoop() async {
        while !Task.isCancelled {
            let serving = kvasir_expert_running() || kvasir_stage_running()
            await MainActor.run {
                self.connected = !self.knownHubs.isEmpty
                if !serving { self.servingHost = "" }
                if !serving && self.connected { self.lastStatus = self.statusText("연결됨 · 대기") }
            }
            try? await Task.sleep(nanoseconds: 45_000_000_000)   // 45s, matches Android
            if Task.isCancelled { break }
            let snapshot = await MainActor.run { self.knownHubs }
            for (base, token) in snapshot {
                if Task.isCancelled { break }
                if kvasir_expert_running() || kvasir_stage_running() { break }   // already serving
                await expertVolunteer(base: base, token: token)                  // MoE expert path (preferred)
                if kvasir_expert_running() { break }
                await volunteer(base: base, token: token)                        // layer-shard fallback
            }
        }
    }

    /// Localised, per-language connection status label (falls back to the Korean
    /// literal when the key isn't in the active dictionary).
    private func statusText(_ fallback: String) -> String { fallback }

    // MARK: MoE expert path — offer scarce (layer, expert-range), serve it in-process

    /// Model-agnostic by contract: the phone hardcodes NOTHING about a model. The
    /// per-model dims (n_embd, needed to serve; n_layer/n_expert for coverage) come
    /// from the /api/expert-volunteer response; if n_embd is absent the assignment
    /// is skipped with a clear log rather than guessing.
    private func expertVolunteer(base: String, token: String) async {
        let body: [String: Any] = ["model": "", "max_experts": maxExperts]
        guard let resp = try? await postJSON("\(base)/api/expert-volunteer", body: body, token: token),
              resp["assigned"] as? Bool == true, !token.isEmpty, !enrolling else { return }
        enrolling = true
        defer { enrolling = false }
        do { try await expertSelfEnroll(base: base, token: token, a: resp) }
        catch { log("expert enroll @ \(host(base)): \(error)") }
    }

    private func expertSelfEnroll(base: String, token: String, a: [String: Any]) async throws {
        let model = a["model"] as? String ?? ""
        let layer = a["layer"] as? Int ?? -1
        let experts = a["experts"] as? [Int] ?? []
        let nEmbd = a["n_embd"] as? Int ?? 0
        let nLayer = a["n_layer"] as? Int ?? 0
        let nExpert = a["n_expert"] as? Int ?? 0
        guard !model.isEmpty, layer >= 0, experts.count == 2, experts[1] > experts[0] else {
            throw HubPartError.message("bad expert assignment")
        }
        guard nEmbd > 0 else {
            log("hub did not supply n_embd for '\(model)' — cannot serve (hub must carry per-model dims). skipping.")
            return
        }
        let e0 = experts[0], e1 = experts[1]
        let name = (model as NSString).lastPathComponent
        let slice = AgentControlServer.shardsDir.appendingPathComponent("\(name).L\(layer)_e\(e0)-\(e1).gguf")
        if !FileManager.default.fileExists(atPath: slice.path) {
            let url = "\(base)/api/proxy/models/\(name)/expert-shard?layers=\(layer):\(layer + 1)&experts=\(e0):\(e1)"
            log("expert: downloading L\(layer) e[\(e0),\(e1)) -> \(slice.lastPathComponent)")
            try await download(url, token: token, to: slice)
        }
        // Bridge the dispatch stream over the relay (443), then serve in-process.
        let session = "expert-\(DeviceInfo.nodeId)"
        expertRelay?.stop()
        let dial = ExpertRelayDial(hubBase: base, session: session, token: token, localPort: expertServePort)
        dial.start()
        expertRelay = dial
        guard kvasir_expert_start(slice.path, Int32(expertServePort), Int32(layer), Int32(nEmbd)) else {
            expertRelay?.stop(); expertRelay = nil
            throw HubPartError.message("expert worker failed to start")
        }
        await MainActor.run {
            self.servingHost = self.host(base)
            self.connected = true
            self.lastStatus = "expert 서빙 · L\(layer) e[\(e0),\(e1))"
        }
        // Heartbeat coverage while the worker serves.
        let cov: [String: Any] = [
            "worker_id": DeviceInfo.nodeId, "model": name,
            "n_layer": nLayer, "n_expert": nExpert,
            "segments": [[layer, e0, e1]],
            "url": "relay:\(session)",
        ]
        while kvasir_expert_running() {
            _ = try? await postJSON("\(base)/api/expert-coverage", body: cov, token: token)
            try await Task.sleep(nanoseconds: 15_000_000_000)
        }
        expertRelay?.stop(); expertRelay = nil
    }

    /// Offer to serve a shard on one hub; self-enroll if it hands us a window.
    private func volunteer(base: String, token: String) async {
        let body: [String: Any] = ["node_id": DeviceInfo.nodeId, "max_layers": maxLayers]
        guard let resp = try? await postJSON("\(base)/api/shard-volunteer", body: body, token: token),
              resp["assigned"] as? Bool == true else { return }
        let model = resp["model"] as? String ?? ""
        let scarcity = resp["scarcity"] as? Double ?? 0
        await setStatus("hub \(host(base)): 배정 \(model) (희소도 \(String(format: "%.2f", scarcity)))")
        // A token-gated hub won't force-place us; if idle, self-enroll to the
        // window it offered. Single-flight, and never while a stage is running.
        let serving = kvasir_stage_running()
        if !token.isEmpty, !enrolling, !serving, !model.isEmpty {
            enrolling = true
            defer { enrolling = false }
            do { try await selfEnroll(base: base, token: token, model: model) }
            catch { log("auto-enroll @ \(host(base)): \(error)") }
        }
    }

    // MARK: self-enroll -> download shard -> relay -> native stage

    private func selfEnroll(base: String, token: String, model: String, controllerId: String = "") async throws {
        let backend = "metal"
        let enrollBody: [String: Any] = [
            "node_id": DeviceInfo.nodeId, "name": UIDevice.current.name, "model": model,
            "controller_id": controllerId,
            "host_platform": ["system": "ios", "machine": "arm64"],
            "backend": ["backend_kind": backend],
            "vram_budget_gib": 4.0, "ram_budget_gib": 4.0,
            "cores": ProcessInfo.processInfo.activeProcessorCount,
            "stage_port": 51072, "ctx": 512,
        ]
        guard let enr = try? await postJSON("\(base)/api/shard-enroll", body: enrollBody, token: token),
              enr["enrolled"] as? Bool == true else {
            throw HubPartError.message("not enrolled")
        }
        log("self-enrolled to \(host(base)) (controller \(enr["controller_id"] as? String ?? "?"))")

        // Poll for the stage config the hub prepares for us.
        var config: [String: Any]?
        var relayInfo: [String: Any]?
        for _ in 0..<40 {
            try await Task.sleep(nanoseconds: 2_000_000_000)
            guard let r = try? await getJSON("\(base)/api/shard-enroll/config?node_id=\(DeviceInfo.nodeId)", token: token) else { continue }
            if r["ready"] as? Bool == true, let c = r["config"] as? [String: Any] {
                config = c; relayInfo = r["relay"] as? [String: Any]; break
            }
        }
        guard var c = config, let layers = c["layers"] as? [Int], layers.count == 2 else {
            throw HubPartError.message("stage config not ready")
        }

        // Download just our layer window (partial shard).
        let name = (model as NSString).lastPathComponent
        let dest = AgentControlServer.shardsDir.appendingPathComponent(name)
        let dlURL = "\(base)/api/proxy/models/\(name)/stage?layers=\(layers[0]):\(layers[1])"
        log("self-enroll: downloading window [\(layers[0]),\(layers[1]))")
        try await download(dlURL, token: token, to: dest)

        // Bridge the ring stream over a WebSocket when the hub is reachable only
        // over 443 (Cloudflare) — rewrite the stage's dial endpoints to the proxy.
        if let relayInfo {
            relay?.stop()
            let rl = RingRelay(hubBase: base, controllerId: relayInfo["controller_id"] as? String ?? "",
                               token: token, log: { [weak self] m in self?.log(m) })
            let proxyEp = "127.0.0.1:\(try rl.start())"
            relay = rl
            if let prev = c["dial_prev_endpoint"] as? String, !prev.isEmpty { c["dial_prev_endpoint"] = proxyEp }
            c["next_endpoint"] = proxyEp
        }

        try startStage(model: name, config: c, layers: layers)
        await setStatus("서빙 중 · 레이어 [\(layers[0]),\(layers[1]))")
    }

    private func startStage(model: String, config c: [String: Any], layers: [Int]) throws {
        let modelPath = AgentControlServer.shardsDir.appendingPathComponent((model as NSString).lastPathComponent)
        guard FileManager.default.fileExists(atPath: modelPath.path) else { throw HubPartError.message("model missing") }
        if kvasir_stage_running() { throw HubPartError.message("stage already running") }
        let role = c["role"] as? String ?? "stage"
        let listen = c["listen_port"] as? Int ?? 51072
        let next = c["next_endpoint"] as? String ?? ""
        let ok = kvasir_stage_start(modelPath.path, Int32(layers[0]), Int32(layers[1]),
                                    role, Int32(listen), next,
                                    Int32(c["gpu_layers"] as? Int ?? -1),
                                    Int32(c["ctx"] as? Int ?? 512),
                                    Int32(c["parallel"] as? Int ?? 1),
                                    c["cache_type_k"] as? String ?? "f16",
                                    c["cache_type_v"] as? String ?? "f16",
                                    c["kv_offload"] as? Bool ?? true)
        guard ok else { throw HubPartError.message("stage failed to start") }
        log("ring stage started: \(role) layers \(layers[0])..\(layers[1]) -> \(next)")
    }

    // MARK: HTTP helpers

    private func postJSON(_ urlStr: String, body: [String: Any], token: String) async throws -> [String: Any]? {
        guard let url = URL(string: urlStr) else { return nil }
        var req = URLRequest(url: url)
        req.httpMethod = "POST"; req.timeoutInterval = 20
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        if !token.isEmpty {
            // Node tokens are verified from the Authorization bearer (hub
            // _bearer_or_cookie); the M2M header covers a static service token.
            req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
            req.setValue(token, forHTTPHeaderField: "X-Linkcpp-Service-Token")
        }
        req.httpBody = try JSONSerialization.data(withJSONObject: body)
        let (data, resp) = try await session.data(for: req)
        guard (200..<300).contains((resp as? HTTPURLResponse)?.statusCode ?? 0) else { return nil }
        return try? JSONSerialization.jsonObject(with: data) as? [String: Any]
    }

    private func getJSON(_ urlStr: String, token: String) async throws -> [String: Any]? {
        guard let url = URL(string: urlStr) else { return nil }
        var req = URLRequest(url: url); req.timeoutInterval = 20
        if !token.isEmpty {
            // Node tokens are verified from the Authorization bearer (hub
            // _bearer_or_cookie); the M2M header covers a static service token.
            req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
            req.setValue(token, forHTTPHeaderField: "X-Linkcpp-Service-Token")
        }
        let (data, resp) = try await session.data(for: req)
        guard (200..<300).contains((resp as? HTTPURLResponse)?.statusCode ?? 0) else { return nil }
        return try? JSONSerialization.jsonObject(with: data) as? [String: Any]
    }

    private func download(_ urlStr: String, token: String, to dest: URL) async throws {
        guard let url = URL(string: urlStr) else { throw HubPartError.message("bad shard URL") }
        var req = URLRequest(url: url); req.timeoutInterval = 600
        if !token.isEmpty {
            // Node tokens are verified from the Authorization bearer (hub
            // _bearer_or_cookie); the M2M header covers a static service token.
            req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
            req.setValue(token, forHTTPHeaderField: "X-Linkcpp-Service-Token")
        }
        let (tmp, resp) = try await session.download(for: req)
        guard (200..<300).contains((resp as? HTTPURLResponse)?.statusCode ?? 0) else {
            throw HubPartError.message("shard download failed")
        }
        try? FileManager.default.removeItem(at: dest)
        try FileManager.default.moveItem(at: tmp, to: dest)
    }

    // MARK: util

    private func host(_ base: String) -> String { URL(string: base)?.host ?? base }
    private func setStatus(_ s: String) async { await MainActor.run { self.lastStatus = s } }
    private nonisolated func log(_ m: String) { NSLog("[HubParticipation] %@", m) }
}

enum HubPartError: Error, CustomStringConvertible {
    case message(String)
    var description: String { if case let .message(m) = self { return m }; return "hub participation error" }
}
