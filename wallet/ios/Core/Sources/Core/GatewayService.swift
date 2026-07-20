import Foundation

// MARK: - Models (mirror the gateway JSON contract)

public struct PayModel: Decodable, Sendable, Identifiable {
    public let id: String
    public let name: String
}

public struct PayModels: Decodable, Sendable {
    public let recipient: String
    public let mint: String
    public let symbol: String
    public let models: [PayModel]
}

public struct PaymentQuote: Decodable, Sendable {
    public let requestId: String
    public let model: String
    public let priceToken: Double
    public let recipient: String
    public let mint: String
    public let symbol: String
    public let estimated: Bool?
    public let estPromptTokens: Int?
    public let estCompletionTokens: Int?
    public let estTotalTokens: Int?
}

/// Actual token usage reported after an inference run.
public struct TokenUsage: Codable, Sendable {
    public let promptTokens: Int
    public let completionTokens: Int
    public let totalTokens: Int
    public let costToken: Double?
}

public struct InferenceResult: Decodable, Sendable {
    public let requestId: String
    public let paid: Bool
    public let signature: String?
    public let model: String?
    public let priceToken: Double?
    public let result: String
    public let usage: TokenUsage?
}

/// HTTP client for the Kvasir inference-payment gateway (Phase 2).
public actor GatewayService {
    private let baseString: String
    private let session: URLSession

    public init?(baseURL: String) {
        let trimmed = baseURL.hasSuffix("/") ? String(baseURL.dropLast()) : baseURL
        guard URL(string: trimmed) != nil, !trimmed.isEmpty else { return nil }
        self.baseString = trimmed
        self.session = URLSession(configuration: .ephemeral)
    }

    public func models() async throws -> PayModels { try await get("/api/pay/models") }

    public func quote(model: String, prompt: String) async throws -> PaymentQuote {
        try await post("/api/pay/quote", ["model": model, "prompt": prompt])
    }

    public func infer(requestId: String, signature: String) async throws -> InferenceResult {
        try await post("/api/inference", ["requestId": requestId, "signature": signature])
    }

    // MARK: transport
    private func makeURL(_ path: String) throws -> URL {
        guard let u = URL(string: baseString + path) else { throw StakingError.badURL }
        return u
    }
    private func get<T: Decodable>(_ path: String) async throws -> T {
        var req = URLRequest(url: try makeURL(path))
        req.timeoutInterval = 20
        return try await send(req)
    }
    private func post<T: Decodable>(_ path: String, _ body: [String: Any]) async throws -> T {
        var req = URLRequest(url: try makeURL(path))
        req.httpMethod = "POST"
        // Inference can run for minutes on very large / paging-backed models
        // (e.g. a 428B MoE served from disk), so allow up to 3 minutes before the
        // client gives up. Fast POSTs (quote, etc.) still return immediately; this
        // is only the ceiling, matched by the gateway's own inference timeout.
        req.timeoutInterval = 180
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        req.httpBody = try JSONSerialization.data(withJSONObject: body)
        return try await send(req)
    }
    private func send<T: Decodable>(_ req: URLRequest) async throws -> T {
        let (data, resp) = try await session.data(for: req)
        let code = (resp as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(code) else {
            let msg = (try? JSONDecoder().decode([String: String].self, from: data))?["error"]
                ?? String(data: data, encoding: .utf8) ?? ""
            throw StakingError.http(code, msg)
        }
        return try JSONDecoder().decode(T.self, from: data)
    }
}
