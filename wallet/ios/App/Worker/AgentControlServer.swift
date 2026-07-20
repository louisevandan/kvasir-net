import Foundation
import Network
import UIKit

/// Minimal managed-node-agent control plane (the phone-side counterpart of
/// controller/nodeagent.py). The hub polls `GET /control/status`, hands us a
/// report URL + M2M token on bind via `POST /control/join`, and drives loads
/// with `POST /control/load|unload|load/cancel`. The data plane is the in-app
/// ggml RPC worker (KvasirRpcWorker); the master streams tensors directly to
/// it, so no model file ever needs to be on the phone.
@MainActor
final class AgentControlServer: ObservableObject {
    static let shared = AgentControlServer()

    @Published private(set) var running = false
    @Published private(set) var boundController: String?
    @Published private(set) var lastEvent = ""

    let agentPort: UInt16 = 9101
    let rpcPort: Int32 = 50072

    private var listener: NWListener?
    private var reportURL: String?
    private var serviceToken: String?
    private var desiredLoad: [String: Any]?
    private var reportSeq = 0
    private var logLines: [String] = []
    private var owner = ""
    private var deviceName = "iPhone"

    // Advisory-only versions (rpc_abi is the hub's hard gate); llama.cpp rev is
    // stamped by the build script into Info.plist when available.
    private var llamaRev: String {
        (Bundle.main.object(forInfoDictionaryKey: "KvasirLlamaCppRev") as? String) ?? "unknown"
    }
    private var unitVersion: String {
        (Bundle.main.object(forInfoDictionaryKey: "KvasirUnitVersion") as? String) ?? "0.0.7"
    }

    /// App-writable model store the hub can stage GGUFs into (ring stages read
    /// only their layer window, but need the file present).
    static var modelsDir: URL {
        let dir = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("models", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }
    /// Node-serving artifacts — ring-stage windows and expert slices — live in a
    /// subdirectory so they never appear in the on-device inference picker, which
    /// lists only top-level GGUFs in modelsDir (ModelStore). These are partial
    /// shards, not runnable standalone models.
    static var shardsDir: URL {
        let dir = modelsDir.appendingPathComponent("shards", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }
    static var workerLog: URL {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("kvasir-worker.log")
    }
    private var downloads: [[String: Any]] = []  // ops surfaced via /control/status
    private var downloadingModels: Set<String> = []  // in-flight, so re-POST is a no-op

    /// GGUFs the hub has staged onto this phone — reported so the hub can poll
    /// until an auto-staged model is present before starting the ring stage.
    private func stagedModels() -> [String] {
        (try? FileManager.default.contentsOfDirectory(atPath: Self.shardsDir.path))?
            .filter { $0.hasSuffix(".gguf") } ?? []
    }

    /// Re-arm the listener from a background task window if it was torn down
    /// while suspended. Reuses the last owner/device so no view context is needed.
    func resumeForBackground() {
        if listener == nil { start(owner: owner, deviceName: deviceName) }
    }

    func start(owner: String, deviceName: String) {
        self.owner = owner
        self.deviceName = deviceName
        guard listener == nil else { return }
        kvasir_worker_redirect_stderr(Self.workerLog.path)
        _ = KvasirRpcWorkerStartIfNeeded(rpcPort: rpcPort)
        do {
            let params = NWParameters.tcp
            params.allowLocalEndpointReuse = true
            let l = try NWListener(using: params, on: NWEndpoint.Port(rawValue: agentPort)!)
            l.newConnectionHandler = { [weak self] conn in self?.handle(conn) }
            l.stateUpdateHandler = { [weak self] st in
                Task { @MainActor in self?.running = (st == .ready) }
            }
            l.start(queue: .global(qos: .userInitiated))
            listener = l
            log("control server starting on :\(agentPort), rpc worker on :\(rpcPort)")
        } catch {
            log("listener failed: \(error)")
        }
    }

    func stop() {
        listener?.cancel()
        listener = nil
        running = false
        // The ggml RPC thread has no stop API; it idles until the app exits.
        log("control server stopped (rpc worker idles until app exit)")
    }

    private func log(_ s: String) {
        lastEvent = s
        logLines.append("\(Date().timeIntervalSince1970) \(s)")
        if logLines.count > 500 { logLines.removeFirst(logLines.count - 500) }
    }

    // MARK: HTTP plumbing

    private nonisolated func handle(_ conn: NWConnection) {
        conn.start(queue: .global(qos: .userInitiated))
        receiveRequest(conn, buffer: Data())
    }

    private nonisolated func receiveRequest(_ conn: NWConnection, buffer: Data) {
        conn.receive(minimumIncompleteLength: 1, maximumLength: 1 << 16) { [weak self] data, _, complete, error in
            guard let self else { conn.cancel(); return }
            var buf = buffer
            if let data { buf.append(data) }
            if let (request, body) = Self.parseIfComplete(buf) {
                Task { @MainActor in
                    let (status, payload) = self.route(request, body: body)
                    Self.respond(conn, status: status, json: payload)
                }
            } else if error != nil || complete {
                conn.cancel()
            } else {
                self.receiveRequest(conn, buffer: buf)
            }
        }
    }

    /// Returns (requestLine+headers, body) once the full body per Content-Length arrived.
    private nonisolated static func parseIfComplete(_ buf: Data) -> (String, Data)? {
        guard let sep = buf.range(of: Data("\r\n\r\n".utf8)) else { return nil }
        guard let head = String(data: buf[..<sep.lowerBound], encoding: .utf8) else { return nil }
        let body = buf[sep.upperBound...]
        let contentLength = head.split(separator: "\r\n")
            .first { $0.lowercased().hasPrefix("content-length:") }
            .flatMap { Int($0.split(separator: ":")[1].trimmingCharacters(in: .whitespaces)) } ?? 0
        guard body.count >= contentLength else { return nil }
        return (head, Data(body.prefix(contentLength)))
    }

    private nonisolated static func respond(_ conn: NWConnection, status: Int, json: [String: Any]) {
        let body = (try? JSONSerialization.data(withJSONObject: json)) ?? Data("{}".utf8)
        var head = "HTTP/1.1 \(status) \(status == 200 ? "OK" : "Error")\r\n"
        head += "Content-Type: application/json\r\nContent-Length: \(body.count)\r\nConnection: close\r\n\r\n"
        var out = Data(head.utf8); out.append(body)
        conn.send(content: out, completion: .contentProcessed { _ in conn.cancel() })
    }

    // MARK: routing

    private func route(_ head: String, body: Data) -> (Int, [String: Any]) {
        let line = head.split(separator: "\r\n").first.map(String.init) ?? ""
        let parts = line.split(separator: " ")
        guard parts.count >= 2 else { return (400, ["error": "bad request"]) }
        let method = String(parts[0])
        let path = String(parts[1]).split(separator: "?").first.map(String.init) ?? ""
        let json = (try? JSONSerialization.jsonObject(with: body)) as? [String: Any] ?? [:]

        switch (method, path) {
        case ("GET", "/control/status"), ("GET", "/info"), ("GET", "/status"):
            return (200, info())
        case ("POST", "/control/join"):
            boundController = json["controller_id"] as? String
            reportURL = json["report_url"] as? String
            if let tok = json["service_token"] as? String, !tok.isEmpty { serviceToken = tok }
            log("joined controller \(boundController ?? "?") report=\(reportURL != nil)")
            pushReport(opType: "join", phase: "joined", status: "done", progress: 100, message: "agent joined")
            return (200, ["joined": true, "status": info()])
        case ("POST", "/bind"):
            boundController = json["controller_id"] as? String
            log("bound to \(boundController ?? "?")")
            return (200, ["bound": true, "status": info()])
        case ("POST", "/unbind"):
            _ = kvasir_stage_request_stop()
            desiredLoad = nil
            reportURL = nil
            boundController = nil
            log("unbound; stage released")
            return (200, ["unbound": true, "released": true])
        case ("POST", "/control/load"):
            _ = KvasirRpcWorkerStartIfNeeded(rpcPort: rpcPort)
            desiredLoad = json
            let op = (json["op_id"] as? String) ?? "load-\(UUID().uuidString.prefix(6))"
            log("load requested: \((json["model"] as? String) ?? "?")")
            pushReport(opId: op, opType: "load", phase: "worker_started", status: "done", progress: 100,
                       message: "worker started; waiting for controller master RPC load",
                       model: json["model"] as? String)
            return (200, ["accepted": true, "op_id": op, "rpc_port": rpcPort, "status": info()])
        case ("POST", "/control/unload"), ("POST", "/control/load/cancel"):
            desiredLoad = nil
            let op = (json["op_id"] as? String) ?? "unload-\(UUID().uuidString.prefix(6))"
            log("unload/cancel")
            pushReport(opId: op, opType: "unload", phase: "unloaded", status: "done", progress: 100,
                       message: "worker idle (in-app rpc thread persists)")
            return (200, ["unloaded": true, "canceled": true, "op_id": op, "status": info()])
        case ("POST", "/control/load-monitor/stop"):
            return (200, ["stopped": true, "status": info()])
        case ("GET", "/control/logs"):
            return (200, ["log": logLines.suffix(200).joined(separator: "\n"),
                          "worker_running": KvasirRpcWorkerIsRunning()])

        // ---- ring stage control plane (phone-side controller/proxy/node_api.py) ----
        case ("GET", "/control/proxy/runtime"):
            return (200, ["runtime_mode": ringCatalogEntry(),
                          "installed_packs": [], "install_enabled": false])
        case ("POST", "/control/proxy/stage/start"):
            return stageStart(json)
        case ("GET", "/control/proxy/stage/status"):
            return (200, stageStatus())
        case ("POST", "/control/proxy/stage/stop"):
            _ = kvasir_stage_request_stop()
            log("stage stop requested")
            return (200, ["stopped": true, "reason": (json["reason"] as? String) ?? "requested"])
        case ("POST", "/control/download"):
            return downloadModel(json)
        default:
            return (404, ["error": "unknown path \(path)"])
        }
    }

    // MARK: ring stage

    /// The bundled runtime identity: iOS cannot install executable packs, so the
    /// app itself is the pack — the hub matches protocol/ABI/build_id against
    /// the identity compiled into liblinkcpp-stage.
    private func ringCatalogEntry() -> [String: Any] {
        let raw = String(cString: kvasir_stage_runtime_info_json())
        var identity: [String: Any] = [:]
        if let data = raw.data(using: .utf8),
           let parsed = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            identity = parsed
        }
        return ["id": "ring_proxy", "label": "linkcpp Proxy", "stability": "preview",
                "available": true, "description": "In-app ring stage (bundled pack)",
                "protocol": identity["protocol"] ?? "linkcpp-stage-v1",
                "adapter_abi": identity["adapter_abi"] ?? 0,
                "build_id": identity["build_id"] ?? "unknown",
                "state_snapshot": true, "chunked_state": true]
    }

    private func stageStart(_ json: [String: Any]) -> (Int, [String: Any]) {
        guard let model = json["model"] as? String,
              let layers = json["layers"] as? [Int], layers.count == 2,
              let role = json["role"] as? String,
              let listen = json["listen_port"] as? Int,
              let next = json["next_endpoint"] as? String else {
            return (400, ["error": "stage start parameters are incomplete"])
        }
        if json["coordinator"] as? Bool == true {
            return (400, ["error": "a phone stage cannot be the ring coordinator"])
        }
        let modelPath = Self.shardsDir.appendingPathComponent((model as NSString).lastPathComponent)
        guard FileManager.default.fileExists(atPath: modelPath.path) else {
            return (404, ["error": "model file is not present on node"])
        }
        if kvasir_stage_running() { return (409, ["error": "a stage is already running"]) }
        let ok = kvasir_stage_start(modelPath.path, Int32(layers[0]), Int32(layers[1]),
                                    role, Int32(listen), next,
                                    Int32(json["gpu_layers"] as? Int ?? -1),
                                    Int32(json["ctx"] as? Int ?? 4096),
                                    Int32(json["parallel"] as? Int ?? 1),
                                    json["cache_type_k"] as? String ?? "f16",
                                    json["cache_type_v"] as? String ?? "f16",
                                    json["kv_offload"] as? Bool ?? true)
        guard ok else { return (409, ["error": "stage failed to start"]) }
        desiredLoad = ["model": model, "layers": layers, "stage": true, "role": role,
                       "listen_port": listen, "next_endpoint": next, "coordinator": false]
        log("ring stage started: \(role) layers \(layers[0])..\(layers[1]) :\(listen) -> \(next)")
        return (200, ["accepted": true, "pid": 0, "log": Self.workerLog.path,
                      "status": stageStatus()])
    }

    private func stageStatus() -> [String: Any] {
        let tail = (try? String(contentsOf: Self.workerLog, encoding: .utf8))
            .map { $0.split(separator: "\n").suffix(160).joined(separator: "\n") } ?? ""
        return ["running": kvasir_stage_running(),
                "exit_code": kvasir_stage_running() ? NSNull() : Int(kvasir_stage_last_exit()),
                "desired_load": desiredLoad as Any, "log": tail]
    }

    // MARK: model download (hub -> phone staging)

    private func downloadModel(_ json: [String: Any]) -> (Int, [String: Any]) {
        guard let model = json["model"] as? String,
              let source = json["source"] as? [String: Any],
              let urlStr = source["url"] as? String, let url = URL(string: urlStr) else {
            return (400, ["error": "download requires model + source.url"])
        }
        let opId = (json["op_id"] as? String) ?? "dl-\(UUID().uuidString.prefix(6))"
        let name = (model as NSString).lastPathComponent
        let dest = Self.shardsDir.appendingPathComponent(name)
        if FileManager.default.fileExists(atPath: dest.path) {
            log("download skipped, already present: \(model)")
            return (200, ["accepted": true, "op_id": opId, "already_present": true])
        }
        if downloadingModels.contains(name) {
            return (200, ["accepted": true, "op_id": opId, "in_progress": true])
        }
        downloadingModels.insert(name)
        log("downloading \(model) from \(urlStr)")
        let task = URLSession.shared.downloadTask(with: url) { [weak self] tmp, _, error in
            // The tmp file is deleted the moment this handler returns — move it
            // NOW, synchronously, before hopping to the main actor for logging.
            var moveError: String?
            if let tmp, error == nil {
                do { try FileManager.default.moveItem(at: tmp, to: dest) }
                catch { moveError = String(describing: error) }
            } else {
                moveError = error.map(String.init(describing:)) ?? "no file"
            }
            Task { @MainActor in
                guard let self else { return }
                self.downloadingModels.remove(name)
                if let moveError {
                    self.log("download failed: \(moveError)")
                    self.pushReport(opId: opId, opType: "download", phase: "error",
                                    status: "error", progress: 0, message: "download failed", model: model)
                } else {
                    self.log("download complete: \(model)")
                    self.pushReport(opId: opId, opType: "download", phase: "completed",
                                    status: "done", progress: 100, message: "download complete", model: model)
                }
            }
        }
        task.resume()
        pushReport(opId: opId, opType: "download", phase: "downloading", status: "running",
                   progress: 5, message: "download started", model: model)
        return (200, ["accepted": true, "op_id": opId])
    }

    // MARK: status payload (mirrors nodeagent._info)

    private func info() -> [String: Any] {
        let totalMem = Double(KvasirRpcWorkerTotalMem()) / 1_073_741_824.0
        let freeMem = Double(KvasirRpcWorkerFreeMem()) / 1_073_741_824.0
        let ram = Double(ProcessInfo.processInfo.physicalMemory) / 1_073_741_824.0
        // Conservative budget: what Metal reports as workable, capped under device RAM.
        let vramBudget = (totalMem > 0 ? min(totalMem, ram * 0.6) : ram * 0.5).rounded(toPlaces: 1)
        let resources: [String: Any] = [
            "vram_total_gib": totalMem.rounded(toPlaces: 2),
            "vram_used_gib": max(0, totalMem - freeMem).rounded(toPlaces: 2),
            "vram_budget_gib": vramBudget,
            "ram_total_gib": ram.rounded(toPlaces: 1),
            "ram_used_gib": 0.0,
            "ram_budget_gib": (ram * 0.4).rounded(toPlaces: 1),
            "cores_total": ProcessInfo.processInfo.processorCount,
            "cores_budget": max(2, ProcessInfo.processInfo.processorCount - 2),
            "cpu_used_percent": 0.0,
            "disk_free_gib": 0.0,
        ]
        let runtime: [String: Any] = [
            "unit_version": unitVersion,
            "runtime_pack_version": unitVersion,
            "llama_cpp_version": llamaRev,
            "rpc_abi": "llama.cpp-rpc",
            "llama_cpp_backend": "metal",
        ]
        let backend: [String: Any] = [
            "backend_kind": "metal",
            "backend_runtime_version": UIDevice.current.systemVersion,
            "backend_driver_version": "",
            "backend_device": String(cString: KvasirRpcWorkerDeviceName()),
            "backend_pack_version": "",
        ]
        return [
            "node_id": DeviceInfo.nodeId,
            "owner": owner,
            "hostname": deviceName,
            "host_platform": ["system": "ios", "release": UIDevice.current.systemVersion,
                              "machine": "arm64", "hostname": deviceName],
            "name": deviceName,
            "gpu": String(cString: KvasirRpcWorkerDeviceName()),
            "gpu_uuid": "metal-\(DeviceInfo.nodeId)",
            "vram_budget_gib": vramBudget,
            "ram_budget_gib": resources["ram_budget_gib"] ?? 0,
            "cores": resources["cores_budget"] ?? 0,
            "resources": resources,
            "bound_to": boundController as Any,
            "rpc_port": rpcPort,
            "worker_running": KvasirRpcWorkerIsRunning(),
            "worker_port": KvasirRpcWorkerIsRunning() ? rpcPort : nil as Any? as Any,
            "operations": [],
            "models": stagedModels(),
            "desired_load": desiredLoad as Any,
            "last_reports": [],
            "runtime": runtime,
            "backend": backend,
            "capabilities": [
                "managed": true, "download": false, "pause_resume_cancel": false,
                "load": true, "unload": true, "reports": reportURL != nil,
                "native_agent": true, "host_system": "ios", "backend_kind": "metal",
            ],
        ]
    }

    // MARK: report push (hub /api/node-reports)

    private func pushReport(opId: String = UUID().uuidString, opType: String, phase: String,
                            status: String, progress: Double, message: String, model: String? = nil) {
        guard let reportURL, let url = URL(string: reportURL) else { return }
        reportSeq += 1
        var payload: [String: Any] = [
            "node_id": DeviceInfo.nodeId, "controller_id": boundController as Any,
            "op_id": opId, "op_type": opType, "phase": phase, "status": status,
            "progress": progress, "message": message, "seq": reportSeq,
            "ts": Date().timeIntervalSince1970,
            "resources": (info()["resources"] as? [String: Any]) ?? [:],
        ]
        if let model { payload["model"] = model }
        var req = URLRequest(url: url)
        req.httpMethod = "POST"
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        if let serviceToken { req.setValue(serviceToken, forHTTPHeaderField: "X-Linkcpp-Service-Token") }
        req.httpBody = try? JSONSerialization.data(withJSONObject: payload)
        URLSession.shared.dataTask(with: req).resume()
    }
}

// MARK: - C bridge conveniences

private func KvasirRpcWorkerStartIfNeeded(rpcPort: Int32) -> Bool {
    kvasir_rpc_worker_start("0.0.0.0", rpcPort, UInt32(max(2, ProcessInfo.processInfo.processorCount - 2)))
}
private func KvasirRpcWorkerIsRunning() -> Bool { kvasir_rpc_worker_running() }
private func KvasirRpcWorkerDeviceName() -> UnsafePointer<CChar> { kvasir_rpc_worker_device_name() }
private func KvasirRpcWorkerTotalMem() -> UInt64 { kvasir_rpc_worker_device_total_mem() }
private func KvasirRpcWorkerFreeMem() -> UInt64 { kvasir_rpc_worker_device_free_mem() }

private extension Double {
    func rounded(toPlaces places: Int) -> Double {
        let d = pow(10.0, Double(places))
        return (self * d).rounded() / d
    }
}
