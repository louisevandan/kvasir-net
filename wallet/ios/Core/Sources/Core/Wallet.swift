import Foundation
import SolanaSwift

public enum WalletError: Error, CustomStringConvertible {
    case invalidMnemonic
    public var description: String {
        switch self {
        case .invalidMnemonic: return "invalid BIP39 mnemonic"
        }
    }
}

/// Derives the Solana keypair from a mnemonic at the Kvasir derivation path
/// `m/44'/501'/0'/0'` (SolanaSwift `DerivablePath.default` = bip44Change, walletIndex 0).
/// This path is the shared-spec contract; it matches the ecosystem-standard
/// (bip39 + ed25519-hd-key) derivation used by Phantom and @solana/web3.js.
public enum WalletDeriver {
    public static let derivationPath = "m/44'/501'/0'/0'"

    public static func keyPair(phrase: [String], network: Network = .devnet) async throws -> KeyPair {
        let normalized = phrase.map { $0.lowercased() }
        guard WalletMnemonic.isValid(normalized) else { throw WalletError.invalidMnemonic }
        return try await KeyPair(phrase: normalized, network: network, derivablePath: .default)
    }

    /// Convenience: derive and return the base58 address only.
    public static func address(phrase: [String], network: Network = .devnet) async throws -> String {
        try await keyPair(phrase: phrase, network: network).publicKey.base58EncodedString
    }
}
