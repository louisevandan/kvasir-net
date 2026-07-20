import Foundation

// MARK: - Models (mirror the staking-service JSON contract)

public struct PerfTier: Decodable, Sendable, Identifiable {
    public let tier: String
    public let minTps: Double
    public let mult: Double
    public var id: String { tier }
}

public struct StakingConfig: Decodable, Sendable {
    public let cluster: String
    public let mint: String
    public let decimals: Int
    public let vault: String
    public let vaultOwner: String
    public let symbol: String
    public let aprPercent: Double
    public let rewardPerUnit: Double
    public let perfTiers: [PerfTier]?
    // Genesis discovery: the canonical URL the gateway advertises for itself.
    // Clients still on the shipped default auto-adopt it; a null value means the
    // gateway isn't publicly addressable yet.
    public let publicUrl: String?
    public let gatewayBonus: Double?
    public let rpcUrl: String?
}

public struct StakePosition: Decodable, Sendable {
    public let owner: String
    public let principal: Double
    public let rewards: Double
    public let total: Double
    public let aprPercent: Double
    public let stakedAt: Double?
}

public struct UnstakeResult: Decodable, Sendable {
    public let signature: String
    public let principalReturned: Double
    public let rewardsPaid: Double
    public let total: Double
}

public struct NodeReward: Decodable, Sendable, Identifiable {
    public let nodeId: String
    // var so the app can fold a hub-qualified expert-work node's reward into its
    // base phone node before display (see the claimable-rewards list).
    public var contributedUnits: Double
    public var pendingRewards: Double
    public var id: String { nodeId }
}

public struct NodeRewards: Decodable, Sendable {
    public let owner: String
    public let pending: Double
    public let rewardPerUnit: Double
    public let nodes: [NodeReward]
}

public struct ClaimResult: Decodable, Sendable {
    public let signature: String
    public let claimed: Double
}

public struct NodeRemoveResult: Decodable, Sendable {
    public let removed: String?   // server returns the removed nodeId string
    public let nodeId: String?
}

public struct NodeRegisterResult: Decodable, Sendable {
    public let nodeId: String
    public let owner: String
    public let os: String?
    public let deviceKind: String?
    public let accelerator: String?
    public let label: String?
    public let perfScore: Double?
    public let backend: String?
    public let mode: String?
    public let tier: String?
    public let perfMultiplier: Double?
}

public struct HeartbeatResult: Decodable, Sendable {
    public let nodeId: String
    public let lastReport: Double?
}

public struct NodeStatusItem: Decodable, Sendable, Identifiable {
    public let nodeId: String
    public let status: String   // online | idle | offline | registered
    public let os: String?      // macos | ios | android | windows | linux | unknown
    public let deviceKind: String?
    public let accelerator: String? // cpu | gpu | npu
    public let label: String?
    public let perfScore: Double?
    public let backend: String?
    public let mode: String?
    public let tier: String?    // S | A | B | C
    public let perfMultiplier: Double?
    // Reward fields are var so the app can fold a hub-qualified expert-work node's
    // earnings into its base phone node before display (see NodeMonitor).
    public var contributedUnits: Double
    public var effectiveUnits: Double?
    public var pendingRewards: Double
    public var claimedTotal: Double
    public let registeredAt: Double?
    public let lastReport: Double?
    public var id: String { nodeId }
}

public struct NodeStatusTotals: Decodable, Sendable {
    public let nodes: Int
    public let online: Int
    public let contributedUnits: Double
    public let effectiveUnits: Double?
    public let pending: Double
    public let claimedTotal: Double
    public let lifetimeRewards: Double
}

public struct NodeStatus: Decodable, Sendable {
    public let owner: String
    public let totals: NodeStatusTotals
    public let nodes: [NodeStatusItem]
}

public enum StakingError: Error, CustomStringConvertible {
    case badURL
    case http(Int, String)
    public var description: String {
        switch self {
        case .badURL: return "invalid staking service URL"
        case .http(let code, let msg): return "staking service error \(code): \(msg)"
        }
    }
}

/// HTTP client for the Kvasir off-chain staking + node-rewards settlement service.
public actor StakingService {
    private let baseString: String
    private let session: URLSession

    public init?(baseURL: String) {
        let trimmed = baseURL.hasSuffix("/") ? String(baseURL.dropLast()) : baseURL
        guard URL(string: trimmed) != nil, !trimmed.isEmpty else { return nil }
        self.baseString = trimmed
        self.session = URLSession(configuration: .ephemeral)
    }

    // MARK: staking
    public func config() async throws -> StakingConfig { try await get("/api/config") }
    public func position(owner: String) async throws -> StakePosition { try await get("/api/positions/\(owner)") }
    public func stake(owner: String, amount: Double, signature: String) async throws -> StakePosition {
        try await post("/api/stake", ["owner": owner, "amount": amount, "signature": signature])
    }
    public func unstake(owner: String, amount: Double? = nil) async throws -> UnstakeResult {
        var body: [String: Any] = ["owner": owner]
        if let amount { body["amount"] = amount }
        return try await post("/api/unstake", body)
    }

    // MARK: node rewards
    public func registerNode(
        nodeId: String, owner: String,
        os: String? = nil, deviceKind: String? = nil, accelerator: String? = nil, label: String? = nil,
        perfScore: Double? = nil, backend: String? = nil, mode: String? = nil
    ) async throws -> NodeRegisterResult {
        var body: [String: Any] = ["nodeId": nodeId, "owner": owner]
        if let os { body["os"] = os }
        if let deviceKind { body["deviceKind"] = deviceKind }
        if let accelerator { body["accelerator"] = accelerator }
        if let label { body["label"] = label }
        if let perfScore { body["perfScore"] = perfScore }
        if let backend { body["backend"] = backend }
        if let mode { body["mode"] = mode }
        return try await post("/api/node/register", body)
    }
    @discardableResult
    public func heartbeat(nodeId: String) async throws -> HeartbeatResult {
        try await post("/api/node/heartbeat", ["nodeId": nodeId])
    }
    public func nodeRewards(owner: String) async throws -> NodeRewards { try await get("/api/node/rewards/\(owner)") }
    public func nodeStatus(owner: String) async throws -> NodeStatus { try await get("/api/node/status/\(owner)") }
    public func claimNodeRewards(owner: String) async throws -> ClaimResult {
        try await post("/api/node/claim", ["owner": owner])
    }
    @discardableResult
    public func removeNode(nodeId: String, owner: String) async throws -> NodeRemoveResult {
        try await post("/api/node/remove", ["nodeId": nodeId, "owner": owner])
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
        req.timeoutInterval = 60
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
