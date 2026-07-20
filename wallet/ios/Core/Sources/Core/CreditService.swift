import Foundation
import TweetNacl
import SolanaSwift

/// Prepaid-credit gateway client: SIWS self-registration + API-key minting, KVR
/// credit deposits, balance, and **streaming** OpenAI-compatible chat.
///
/// Streaming is why this path exists: the non-streaming `/api/inference` route
/// gets cut off by Cloudflare's fixed 100s origin timeout on slow models (M3 at
/// ~1 tok/s), returning 524. `/v1/chat/completions` with `stream: true` flows SSE
/// chunks from the first token, so Cloudflare never idles the connection out.
///
/// Auth mirrors HubAuthService (controller/siws.py-style ed25519): the wallet signs
/// a server nonce, base64 signature. Credits are debited per completion's usage.
public actor CreditService {
    private let baseString: String
    private let session = URLSession(configuration: .ephemeral)

    public init(baseUrl: String) {
        var b = baseUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        while b.hasSuffix("/") { b.removeLast() }
        self.baseString = b
    }

    private struct Challenge: Decodable { let nonce: String?; let message: String? }
    private struct RegisterResp: Decodable { let ok: Bool?; let whitelisted: Bool? }
    private struct ApiKeyResp: Decodable { let apiKey: String? }
    private struct DepositResp: Decodable { let ok: Bool?; let balance: Double? }

    // MARK: SIWS onboarding

    /// Self-register the wallet into the credit whitelist (gateway must have
    /// CREDIT_OPEN_REGISTER on). Idempotent server-side.
    public func register(mnemonic: [String]) async throws {
        let (kp, wallet) = try await keypair(mnemonic)
        let ch: Challenge = try await post("/api/credits/register/challenge", ["wallet": wallet])
        guard let message = ch.message, let nonce = ch.nonce, !message.isEmpty, !nonce.isEmpty else {
            throw CreditError.message("gateway issued no register challenge")
        }
        let sig = try NaclSign.signDetached(message: Data(message.utf8), secretKey: kp.secretKey)
        let _: RegisterResp = try await post("/api/credits/register",
            ["wallet": wallet, "nonce": nonce, "signature": sig.base64EncodedString()])
    }

    /// Mint an API key bound to the wallet. Returned once — the caller must store it.
    public func mintApiKey(mnemonic: [String], label: String) async throws -> String {
        let (kp, wallet) = try await keypair(mnemonic)
        let ch: Challenge = try await post("/api/credits/challenge", ["wallet": wallet])
        guard let message = ch.message, let nonce = ch.nonce, !message.isEmpty, !nonce.isEmpty else {
            throw CreditError.message("gateway issued no api-key challenge")
        }
        let sig = try NaclSign.signDetached(message: Data(message.utf8), secretKey: kp.secretKey)
        let resp: ApiKeyResp = try await post("/api/credits/apikey",
            ["wallet": wallet, "nonce": nonce, "signature": sig.base64EncodedString(), "label": label])
        guard let key = resp.apiKey, !key.isEmpty else { throw CreditError.message("gateway minted no api key") }
        return key
    }

    /// Credit a prior on-chain KVR transfer (caller sends the transfer, passes its
    /// signature). Returns the new balance.
    public func deposit(wallet: String, amount: Double, signature: String) async throws -> Double {
        let resp: DepositResp = try await post("/api/credits/deposit",
            ["wallet": wallet, "amount": amount, "signature": signature])
        return resp.balance ?? 0
    }

    /// Current credit balance for the wallet behind this API key.
    public func balance(apiKey: String) async throws -> CreditBalance {
        try await get("/api/credits/balance", apiKey: apiKey)
    }

    // MARK: streaming chat

    /// One SSE event from the chat stream.
    public enum StreamEvent: Sendable {
        case token(String)          // an answer content delta
        case reasoning(String)      // a thinking delta (usually suppressed by the gateway)
        case usage(TokenUsage)      // final usage chunk
    }

    /// Stream an OpenAI-compatible completion. Yields content deltas as they arrive
    /// so the UI can append incrementally (and Cloudflare sees continuous data).
    public nonisolated func streamChat(
        apiKey: String, model: String, messages: [[String: String]], maxTokens: Int = 1024
    ) -> AsyncThrowingStream<StreamEvent, Error> {
        AsyncThrowingStream { continuation in
            let task = Task {
                do {
                    let cfg = URLSessionConfiguration.ephemeral
                    cfg.timeoutIntervalForRequest = 600     // idle timeout resets while chunks flow
                    cfg.timeoutIntervalForResource = 1200
                    let stream = URLSession(configuration: cfg)
                    guard let url = URL(string: await baseString + "/v1/chat/completions") else {
                        throw CreditError.message("bad gateway URL")
                    }
                    var req = URLRequest(url: url)
                    req.httpMethod = "POST"
                    req.setValue("application/json", forHTTPHeaderField: "Content-Type")
                    req.setValue("text/event-stream", forHTTPHeaderField: "Accept")
                    req.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
                    // enable_thinking=false is injected server-side; the app need not send it.
                    let body: [String: Any] = ["model": model, "messages": messages, "stream": true, "max_tokens": maxTokens]
                    req.httpBody = try JSONSerialization.data(withJSONObject: body)

                    let (bytes, resp) = try await stream.bytes(for: req)
                    let code = (resp as? HTTPURLResponse)?.statusCode ?? 0
                    guard (200..<300).contains(code) else {
                        // Non-2xx: drain the (JSON) error body for a useful message.
                        var raw = ""
                        for try await line in bytes.lines { raw += line }
                        throw CreditError.message(Self.errorText(raw, code: code))
                    }
                    for try await line in bytes.lines {
                        guard line.hasPrefix("data:") else { continue }
                        let payload = line.dropFirst(5).trimmingCharacters(in: .whitespaces)
                        if payload == "[DONE]" { break }
                        guard let data = payload.data(using: .utf8),
                              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { continue }
                        if let choices = obj["choices"] as? [[String: Any]], let delta = choices.first?["delta"] as? [String: Any] {
                            if let c = delta["content"] as? String, !c.isEmpty { continuation.yield(.token(c)) }
                            if let r = delta["reasoning_content"] as? String, !r.isEmpty { continuation.yield(.reasoning(r)) }
                        }
                        if let u = obj["usage"] as? [String: Any] {
                            continuation.yield(.usage(TokenUsage(
                                promptTokens: (u["prompt_tokens"] as? Int) ?? 0,
                                completionTokens: (u["completion_tokens"] as? Int) ?? 0,
                                totalTokens: (u["total_tokens"] as? Int) ?? 0,
                                costToken: nil)))
                        }
                    }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }

    private static func errorText(_ raw: String, code: Int) -> String {
        // OpenAI error shape {"error":{"message":...}} or {"error":"..."}.
        if let d = raw.data(using: .utf8), let o = try? JSONSerialization.jsonObject(with: d) as? [String: Any] {
            if let e = o["error"] as? [String: Any], let m = e["message"] as? String { return m }
            if let e = o["error"] as? String { return e }
        }
        return raw.isEmpty ? "HTTP \(code)" : raw
    }

    // MARK: helpers

    private func keypair(_ mnemonic: [String]) async throws -> (KeyPair, String) {
        let kp = try await WalletDeriver.keyPair(phrase: mnemonic.map { $0.lowercased() })
        return (kp, kp.publicKey.base58EncodedString)
    }

    private func post<T: Decodable>(_ path: String, _ body: [String: Any]) async throws -> T {
        guard let url = URL(string: baseString + path) else { throw CreditError.message("bad gateway URL") }
        var req = URLRequest(url: url)
        req.httpMethod = "POST"
        req.timeoutInterval = 30
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        req.httpBody = try JSONSerialization.data(withJSONObject: body)
        return try await send(req)
    }

    private func get<T: Decodable>(_ path: String, apiKey: String) async throws -> T {
        guard let url = URL(string: baseString + path) else { throw CreditError.message("bad gateway URL") }
        var req = URLRequest(url: url)
        req.timeoutInterval = 20
        req.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        return try await send(req)
    }

    private func send<T: Decodable>(_ req: URLRequest) async throws -> T {
        let (data, resp) = try await session.data(for: req)
        let code = (resp as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(code) else {
            let msg = (try? JSONDecoder().decode([String: String].self, from: data))?["error"]
                ?? String(data: data, encoding: .utf8) ?? "HTTP \(code)"
            throw CreditError.message(msg)
        }
        return try JSONDecoder().decode(T.self, from: data)
    }
}

public struct CreditBalance: Decodable, Sendable {
    public let wallet: String
    public let balance: Double
    public let spent: Double
    public let symbol: String
}

public enum CreditError: Error, CustomStringConvertible {
    case message(String)
    public var description: String { if case let .message(m) = self { return m }; return "credit error" }
}
