import Foundation
import Network

/// Relays a ring stage's raw TCP stream to the coordinator through the hub over
/// a WebSocket (443), so a NAT node — or a hub behind Cloudflare, which proxies
/// only 80/443 — needs no publicly reachable ring port. The local ring stage
/// dials 127.0.0.1:proxyPort; each connection is bridged to a WebSocket at
/// {hub}/api/ring-relay, which the hub joins to the coordinator's ring listener.
///
/// Mirrors `wallet/android/.../RingRelay.kt`, but uses `URLSessionWebSocketTask`
/// (native RFC 6455: handshake, masking, ping/pong handled by the OS) instead of
/// a hand-rolled frame codec, and `NWListener`/`NWConnection` for the local TCP
/// proxy.
final class RingRelay {
    private let wsURL: URL
    private let controllerId: String
    private let token: String
    private let log: (String) -> Void
    private var listener: NWListener?
    private let queue = DispatchQueue(label: "kvasir.ring-relay")
    private let session = URLSession(configuration: .ephemeral)

    init(hubBase: String, controllerId: String, token: String, log: @escaping (String) -> Void) {
        var base = hubBase
        while base.hasSuffix("/") { base.removeLast() }
        // http->ws, https->wss
        if base.hasPrefix("https") { base = "wss" + base.dropFirst("https".count) }
        else if base.hasPrefix("http") { base = "ws" + base.dropFirst("http".count) }
        var comps = URLComponents(string: base + "/api/ring-relay")!
        var items = [URLQueryItem(name: "controller_id", value: controllerId)]
        if !token.isEmpty { items.append(URLQueryItem(name: "token", value: token)) }
        comps.queryItems = items
        self.wsURL = comps.url!
        self.controllerId = controllerId
        self.token = token
        self.log = log
    }

    /// Start the local TCP proxy; returns the 127.0.0.1 port the stage should dial.
    func start() throws -> Int {
        let params = NWParameters.tcp
        params.requiredLocalEndpoint = NWEndpoint.hostPort(host: "127.0.0.1", port: .any)
        (params.defaultProtocolStack.transportProtocol as? NWProtocolTCP.Options)?.noDelay = true
        let l = try NWListener(using: params)
        l.newConnectionHandler = { [weak self] conn in self?.bridge(conn) }
        var boundPort: Int = 0
        let sem = DispatchSemaphore(value: 0)
        l.stateUpdateHandler = { state in
            if case .ready = state { boundPort = Int(l.port?.rawValue ?? 0); sem.signal() }
            if case .failed = state { sem.signal() }
        }
        l.start(queue: queue)
        _ = sem.wait(timeout: .now() + 5)
        listener = l
        guard boundPort > 0 else { throw NSError(domain: "RingRelay", code: 1) }
        log("ring relay proxy on 127.0.0.1:\(boundPort) -> \(wsURL.absoluteString)")
        return boundPort
    }

    func stop() { listener?.cancel(); listener = nil }

    // One stage TCP connection <-> one WebSocket to the hub.
    private func bridge(_ conn: NWConnection) {
        let ws = session.webSocketTask(with: wsURL)
        conn.start(queue: queue)
        ws.resume()
        let closeBoth: () -> Void = {
            conn.cancel()
            ws.cancel(with: .normalClosure, reason: nil)
        }
        pumpWStoTCP(ws, conn, closeBoth)   // WebSocket -> stage TCP
        pumpTCPtoWS(conn, ws, closeBoth)   // stage TCP -> WebSocket
    }

    private func pumpTCPtoWS(_ conn: NWConnection, _ ws: URLSessionWebSocketTask, _ close: @escaping () -> Void) {
        conn.receive(minimumIncompleteLength: 1, maximumLength: 65536) { [weak self] data, _, isComplete, err in
            if let data, !data.isEmpty {
                ws.send(.data(data)) { sendErr in
                    if sendErr != nil { close() } else { self?.pumpTCPtoWS(conn, ws, close) }
                }
            } else if isComplete || err != nil {
                close()
            } else {
                self?.pumpTCPtoWS(conn, ws, close)
            }
        }
    }

    private func pumpWStoTCP(_ ws: URLSessionWebSocketTask, _ conn: NWConnection, _ close: @escaping () -> Void) {
        ws.receive { [weak self] result in
            switch result {
            case .failure:
                close()
            case .success(let message):
                let payload: Data?
                switch message {
                case .data(let d): payload = d
                case .string(let s): payload = s.data(using: .utf8)
                @unknown default: payload = nil
                }
                if let payload, !payload.isEmpty {
                    conn.send(content: payload, completion: .contentProcessed { sendErr in
                        if sendErr != nil { close() } else { self?.pumpWStoTCP(ws, conn, close) }
                    })
                } else {
                    self?.pumpWStoTCP(ws, conn, close)
                }
            }
        }
    }
}
