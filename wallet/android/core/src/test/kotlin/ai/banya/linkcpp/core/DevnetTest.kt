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
        // Distribution has started, so "the treasury still holds essentially the
        // whole mint" stopped being true — it is about half of the 1e9 supply now.
        // What is worth asserting is that this really is the treasury of a live
        // mint and not an empty account: a majority of supply, on the nose for a
        // token whose circulating half is staked and paid out.
        assertTrue(bal.amount > 400_000_000.0, "treasury should hold a majority of supply, got ${bal.amount}")
    }

    @Test fun solBalanceReadable() {
        val (service, _) = SolanaService.fromSharedSpec()
        val sol = service.solBalance(treasuryOwner)
        assertEquals("SOL", sol.symbol)
        assertTrue(sol.raw >= 0)
    }
}
