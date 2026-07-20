package ai.banya.linkcpp.core

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** Offline: shared-spec, BIP39, and the cross-implementation derivation vector. */
class WalletCoreTest {

    @Test fun sharedSpecLoadsRealMint() {
        val spec = SharedSpec.loadToken()
        assertEquals("KVR", spec.token.symbol)
        assertEquals(6, spec.token.decimals)
        assertEquals("DhtWwpWBXcL7WoWAgWk8f5ZHVNxAkHESmcdiQNma1msL", spec.token.mint)
        assertEquals("devnet", spec.cluster)
    }

    @Test fun constantsDerivationPath() {
        val c = SharedSpec.loadConstants()
        assertEquals("m/44'/501'/0'/0'", c.derivation.path)
        assertEquals(501, c.derivation.coinType)
        assertEquals("devnet", c.activeCluster)
    }

    @Test fun mnemonicGenerateLengths() {
        assertEquals(12, Bip39.generate(12).size)
        assertEquals(24, Bip39.generate(24).size)
    }

    @Test fun mnemonicValidation() {
        val valid = "employ very include battle midnight usual broom school hollow manual embody slam"
        assertTrue(Bip39.isValid(Bip39.words(valid)))
        assertFalse(Bip39.isValid(Bip39.words(
            "length else orbit dinner cannon dinosaur miss deal reward embody debate axis")))
        assertFalse(Bip39.isValid(listOf("hello", "world")))
    }

    /**
     * The derivation contract: this exact phrase must derive this exact address,
     * identical to the iOS wallet and the ecosystem standard (Phantom / web3.js).
     */
    @Test fun derivationVectorMatchesEcosystem() {
        val phrase = Bip39.words(
            "employ very include battle midnight usual broom school hollow manual embody slam")
        assertEquals("EZFPUWABKyJKrZ5Ubbu5kdbraSkdCdLu9KPDeU3qgqEG", WalletDeriver.address(phrase))
    }

    @Test fun generatedMnemonicIsValidAndDerivable() {
        val m = Bip39.generate(12)
        assertTrue(Bip39.isValid(m))
        assertTrue(WalletDeriver.address(m).length in 32..44)
    }
}
