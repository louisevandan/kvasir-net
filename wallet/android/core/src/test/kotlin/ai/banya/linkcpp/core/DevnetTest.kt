package ai.banya.linkcpp.core

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** Integration tests against Solana devnet using the real Kvasir KVR mint. */
class DevnetTest {
    private val treasuryOwner = "8uu2gDKFVtNS79yqYyztJeerEKAh4cnZGdQytCjsYNfF"

    @Test fun treasuryHoldsBulkOfSupply() {
        val (service, spec) = SolanaService.fromSharedSpec()
        val bal = service.tokenBalance(
            treasuryOwner, spec.token.mint, spec.token.symbol, spec.token.decimals)
        assertEquals("KVR", bal.symbol)
        assertEquals(6, bal.decimals)
        assertTrue(bal.amount > 999_990_000.0, "treasury should hold bulk supply, got ${bal.amount}")
    }

    @Test fun solBalanceReadable() {
        val (service, _) = SolanaService.fromSharedSpec()
        val sol = service.solBalance(treasuryOwner)
        assertEquals("SOL", sol.symbol)
        assertTrue(sol.raw >= 0)
    }
}
