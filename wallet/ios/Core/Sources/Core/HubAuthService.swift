import Foundation
import TweetNacl

/// Sign-In-With-Solana against a Kvasir hub to obtain the bearer token an
/// autonomous node uses to poll/enroll on an auth-gated (public, remote) hub.
///
/// Mirrors `wallet/android/.../HubAuthService.kt`. Flow (controller/siws.py,
/// controller/hub.py):
///   1. POST /api/auth/challenge {wallet}          -> {nonce, message}
///   2. sign the message bytes with the wallet key -> base64 ed25519 signature
///   3. POST /api/auth/node-token {wallet,nonce,signature} -> {node_token}
/// The node token is scoped to participation, so the hub skips 2FA (which a
/// mobile wallet has no UI for). Returns the bearer token the node polls with.
public actor HubAuthService {
    private let baseString: String
    private let mnemonic: [String]
    private let session = URLSession(configuration: .ephemeral)

    public init(baseUrl: String, mnemonic: [String]) {
        var b = baseUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        while b.hasSuffix("/") { b.removeLast() }
        self.baseString = b
        self.mnemonic = mnemonic.map { $0.lowercased() }
    }

    private struct Challenge: Decodable { let nonce: String?; let message: String? }
    private struct TokenResp: Decodable { let node_token: String? }

    /// Mint a long-lived NODE token with a wallet signature alone — no OTP.
    public func nodeToken() async throws -> String {
        let kp = try await WalletDeriver.keyPair(phrase: mnemonic)
        let wallet = kp.publicKey.base58EncodedString

        let ch: Challenge = try await post("/api/auth/challenge", ["wallet": wallet])
        guard let message = ch.message, let nonce = ch.nonce,
              !message.isEmpty, !nonce.isEmpty else {
            throw HubAuthError.message("hub did not issue a challenge")
        }
        let sig = try NaclSign.signDetached(message: Data(message.utf8), secretKey: kp.secretKey)
        let sigB64 = sig.base64EncodedString()

        let resp: TokenResp = try await post("/api/auth/node-token",
            ["wallet": wallet, "nonce": nonce, "signature": sigB64])
        guard let token = resp.node_token, !token.isEmpty else {
            throw HubAuthError.message("hub issued no node token")
        }
        return token
    }

    // MARK: transport (mirrors StakingService)
    private func post<T: Decodable>(_ path: String, _ body: [String: Any]) async throws -> T {
        guard let url = URL(string: baseString + path) else { throw HubAuthError.message("bad hub URL") }
        var req = URLRequest(url: url)
        req.httpMethod = "POST"
        req.timeoutInterval = 20
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        req.httpBody = try JSONSerialization.data(withJSONObject: body)
        let (data, resp) = try await session.data(for: req)
        let code = (resp as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(code) else {
            let msg = (try? JSONDecoder().decode([String: String].self, from: data))?["error"]
                ?? String(data: data, encoding: .utf8) ?? "HTTP \(code)"
            throw HubAuthError.message(msg)
        }
        return try JSONDecoder().decode(T.self, from: data)
    }
}

public enum HubAuthError: Error, CustomStringConvertible {
    case message(String)
    public var description: String { if case let .message(m) = self { return m }; return "hub auth error" }
}
