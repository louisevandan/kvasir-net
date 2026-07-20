import Foundation

/// Decoded view of `wallet/shared-spec/token.devnet.json` — the single source of
/// truth (shared with the Android wallet) for the Kvasir token and network.
public struct TokenDevnetSpec: Decodable, Sendable {
    public struct Token: Decodable, Sendable {
        public let name: String
        public let symbol: String
        public let decimals: Int
        public let mint: String
    }
    public struct Treasury: Decodable, Sendable {
        public let owner: String
        public let ata: String?
    }
    public let cluster: String
    public let rpcUrl: String
    public let token: Token
    public let treasury: Treasury
}

/// Decoded view of `wallet/shared-spec/wallet-constants.json`.
public struct WalletConstants: Decodable, Sendable {
    public struct Derivation: Decodable, Sendable {
        public let scheme: String
        public let path: String
        public let coinType: Int
    }
    public struct Cluster: Decodable, Sendable {
        public let rpcUrl: String
        public let explorerCluster: String
    }
    public let derivation: Derivation
    public let activeCluster: String
    public let clusters: [String: Cluster]
    public let stakingServiceUrl: String?
}

public enum SharedSpecError: Error, CustomStringConvertible {
    case missingResource(String)
    public var description: String {
        switch self {
        case .missingResource(let name): return "shared-spec resource not found: \(name)"
        }
    }
}

/// Loads the shared-spec JSON bundled as package/app resources. These files are
/// copied from `wallet/shared-spec/` by `wallet/ios/sync-spec.sh`.
public enum SharedSpec {
    public static func loadToken(bundle: Bundle? = nil) throws -> TokenDevnetSpec {
        try load("token.devnet", as: TokenDevnetSpec.self, bundle: bundle ?? .module)
    }

    public static func loadConstants(bundle: Bundle? = nil) throws -> WalletConstants {
        try load("wallet-constants", as: WalletConstants.self, bundle: bundle ?? .module)
    }

    static func load<T: Decodable>(_ name: String, as: T.Type, bundle: Bundle) throws -> T {
        guard let url = bundle.url(forResource: name, withExtension: "json") else {
            throw SharedSpecError.missingResource("\(name).json")
        }
        let data = try Data(contentsOf: url)
        return try JSONDecoder().decode(T.self, from: data)
    }
}
