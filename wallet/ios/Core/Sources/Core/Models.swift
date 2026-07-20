import Foundation

/// A balance for one asset (SOL or an SPL token), in both raw base units and
/// human-readable amount.
public struct AssetBalance: Sendable, Equatable {
    public let symbol: String
    public let raw: UInt64
    public let decimals: Int

    public init(symbol: String, raw: UInt64, decimals: Int) {
        self.symbol = symbol
        self.raw = raw
        self.decimals = decimals
    }

    /// Human-readable amount (raw / 10^decimals).
    public var amount: Double {
        Double(raw) / pow(10.0, Double(decimals))
    }
}

/// A reference to a past transaction touching an address.
public struct TxRef: Sendable, Equatable {
    public let signature: String
    public let slot: UInt64?
    public let failed: Bool
    public let blockTime: Int64?

    public init(signature: String, slot: UInt64?, failed: Bool, blockTime: Int64?) {
        self.signature = signature
        self.slot = slot
        self.failed = failed
        self.blockTime = blockTime
    }
}
