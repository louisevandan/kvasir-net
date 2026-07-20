package ai.banya.linkcpp.core

import org.sol4k.Keypair

/**
 * Derives the Solana keypair from a mnemonic at the Kvasir path m/44'/501'/0'/0'.
 * BIP39 seed -> SLIP-0010 ed25519 -> sol4k Keypair. This reproduces the iOS
 * derivation exactly (guarded by the shared cross-impl vector test).
 */
object WalletDeriver {
    const val derivationPath = "m/44'/501'/0'/0'"

    fun keypair(phrase: List<String>): Keypair {
        val normalized = phrase.map { it.lowercase() }
        require(Bip39.isValid(normalized)) { "invalid BIP39 mnemonic" }
        val seed = Bip39.toSeed(normalized)
        return Keypair.fromSecretKey(Slip10.derive(derivationPath, seed))
    }

    fun address(phrase: List<String>): String = keypair(phrase).publicKey.toBase58()
}
