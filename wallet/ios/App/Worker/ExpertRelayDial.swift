import Foundation
import Network

/// Worker-mode expert-relay bridge: pipe the local `kvasir_expert` serve port to
/// the hub's /api/expert-relay WebSocket (443), so a NAT phone reaches the
/// backbone over one long-lived stream. iOS port of expert-relay-dial.py
/// (--mode worker): connect the hub WS AND 127.0.0.1:localPort, then pipe.
/// Uses `URLSessionWebSocketTask` (native RFC 6455) + `NWConnection` (TCP client).
final class ExpertRelayDial {
    private let wsURL: URL
    private let localPort: Int
    private let session = URLSession(configuration: .ephemeral)
    private let queue = DispatchQueue(label: "kvasir.expert-relay")
    private var conn: NWConnection?
    private var ws: URLSessionWebSocketTask?
    private var stopped = false

    init(hubBase: String, session: String, token: String, localPort: Int) {
        var base = hubBase
        while base.hasSuffix("/") { base.removeLast() }
        if base.hasPrefix("https") { base = "wss" + base.dropFirst("https".count) }
        else if base.hasPrefix("http") { base = "ws" + base.dropFirst("http".count) }
        var comps = URLComponents(string: base + "/api/expert-relay")!
        var items = [URLQueryItem(name: "session", value: session)]
        if !token.isEmpty { items.append(URLQueryItem(name: "token", value: token)) }
        comps.queryItems = items
        self.wsURL = comps.url!
        self.localPort = localPort
    }

    func start() {
        guard let port = NWEndpoint.Port(rawValue: UInt16(localPort)) else { return }
        let params = NWParameters.tcp
        (params.defaultProtocolStack.transportProtocol as? NWProtocolTCP.Options)?.noDelay = true
        let c = NWConnection(host: "127.0.0.1", port: port, using: params)
        let w = session.webSocketTask(with: wsURL)
        conn = c; ws = w
        c.start(queue: queue); w.resume()
        pumpTCPtoWS(c, w)
        pumpWStoTCP(w, c)
    }

    func stop() {
        stopped = true
        conn?.cancel()
        ws?.cancel(with: .normalClosure, reason: nil)
    }

    private func closeBoth() {
        conn?.cancel()
        ws?.cancel(with: .normalClosure, reason: nil)
    }

    private func pumpTCPtoWS(_ conn: NWConnection, _ ws: URLSessionWebSocketTask) {
        conn.receive(minimumIncompleteLength: 1, maximumLength: 65536) { [weak self] data, _, isComplete, err in
            guard let self, !self.stopped else { return }
            if let data, !data.isEmpty {
                ws.send(.data(data)) { e in
                    if e != nil { self.closeBoth() } else { self.pumpTCPtoWS(conn, ws) }
                }
            } else if isComplete || err != nil {
                self.closeBoth()
            } else {
                self.pumpTCPtoWS(conn, ws)
            }
        }
    }

    private func pumpWStoTCP(_ ws: URLSessionWebSocketTask, _ conn: NWConnection) {
        ws.receive { [weak self] result in
            guard let self, !self.stopped else { return }
            switch result {
            case .failure:
                self.closeBoth()
            case .success(let message):
                let payload: Data?
                switch message {
                case .data(let d): payload = d
                case .string(let s): payload = s.data(using: .utf8)
                @unknown default: payload = nil
                }
                if let payload, !payload.isEmpty {
                    conn.send(content: payload, completion: .contentProcessed { e in
                        if e != nil { self.closeBoth() } else { self.pumpWStoTCP(ws, conn) }
                    })
                } else {
                    self.pumpWStoTCP(ws, conn)
                }
            }
        }
    }
}
