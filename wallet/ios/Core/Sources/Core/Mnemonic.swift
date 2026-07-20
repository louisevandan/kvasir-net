import Foundation
import SolanaSwift

/// BIP39 mnemonic generation and validation for the Kvasir wallet.
/// Wraps SolanaSwift's `Mnemonic` so the rest of the app has a small, stable API.
public enum WalletMnemonic {
    /// Generate a new BIP39 mnemonic. `wordCount` must be 12 or 24.
    public static func generate(wordCount: Int = 12) -> [String] {
        let strength = wordCount == 24 ? 256 : 128
        return Mnemonic(strength: strength, wordlist: Wordlists.english).phrase
    }

    /// True if `phrase` is a valid BIP39 mnemonic (word list + checksum).
    public static func isValid(_ phrase: [String]) -> Bool {
        let normalized = phrase.map { $0.lowercased() }
        return (try? Mnemonic(phrase: normalized)) != nil
    }

    /// Normalize user input into a word array (lowercased, whitespace-split).
    public static func words(from text: String) -> [String] {
        text.lowercased()
            .split(whereSeparator: { $0.isWhitespace })
            .map(String.init)
    }
}
