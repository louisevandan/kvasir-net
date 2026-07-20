package ai.banya.linkcpp.core

import org.sol4k.Connection
import org.sol4k.api.Commitment
import org.sol4k.Keypair
import org.sol4k.PublicKey
import org.sol4k.TransactionMessage
import org.sol4k.VersionedTransaction
import org.sol4k.instruction.CreateAssociatedTokenAccountInstruction
import org.sol4k.instruction.Instruction
import org.sol4k.instruction.SplTransferInstruction
import org.sol4k.instruction.TransferInstruction
import kotlin.math.pow
import kotlin.math.roundToLong

/** Reads balances/history and sends SOL/SPL via sol4k (mirrors iOS SolanaService). */
class SolanaService(rpcUrl: String) {
    private val conn = Connection(rpcUrl, Commitment.CONFIRMED)

    fun solBalance(owner: String): AssetBalance {
        val lamports = conn.getBalance(PublicKey(owner)).toLong()
        return AssetBalance("SOL", lamports / 1e9, lamports, 9)
    }

    fun tokenBalance(owner: String, mint: String, symbol: String, decimals: Int): AssetBalance {
        val account = ata(PublicKey(owner), PublicKey(mint))
        return try {
            val raw = conn.getTokenAccountBalance(account).amount.toLong()
            AssetBalance(symbol, raw / 10.0.pow(decimals), raw, decimals)
        } catch (e: Exception) {
            AssetBalance(symbol, 0.0, 0, decimals) // no token account yet
        }
    }

    fun recentTransactions(owner: String, limit: Int = 20): List<TxRef> =
        conn.getSignaturesForAddress(PublicKey(owner), limit)
            .map { TxRef(it.signature, it.blockTime, it.isError) }

    fun sendSol(from: Keypair, to: String, sol: Double): String {
        val lamports = (sol * 1e9).roundToLong()
        return submit(from, listOf(TransferInstruction(from.publicKey, PublicKey(to), lamports)))
    }

    fun sendToken(from: Keypair, mint: String, decimals: Int, to: String, amount: Double): String {
        val mintPk = PublicKey(mint)
        val toOwner = PublicKey(to)
        val fromAta = ata(from.publicKey, mintPk)
        val toAta = ata(toOwner, mintPk)
        val ixs = ArrayList<Instruction>()
        if (conn.getAccountInfo(toAta) == null) {
            ixs.add(CreateAssociatedTokenAccountInstruction(from.publicKey, toAta, toOwner, mintPk))
        }
        val raw = (amount * 10.0.pow(decimals)).roundToLong()
        ixs.add(SplTransferInstruction(fromAta, toAta, mintPk, from.publicKey, raw, decimals))
        return submit(from, ixs)
    }

    private fun ata(owner: PublicKey, mint: PublicKey): PublicKey =
        PublicKey.findProgramDerivedAddress(owner, mint).publicKey

    // Public devnet RPC load-balances across nodes, so a freshly fetched blockhash
    // is occasionally "not found" during preflight. Refetch and retry a few times.
    private fun submit(payer: Keypair, ixs: List<Instruction>): String {
        var lastError: Exception? = null
        repeat(4) { attempt ->
            try {
                // sol4k's sendTransaction runs preflight at FINALIZED, so use a
                // finalized blockhash to avoid "Blockhash not found".
                val blockhash = conn.getLatestBlockhash(Commitment.FINALIZED)
                val tx = VersionedTransaction(TransactionMessage.newMessage(payer.publicKey, blockhash, ixs))
                tx.sign(payer)
                return conn.sendTransaction(tx)
            } catch (e: Exception) {
                lastError = e
                if (attempt < 3) Thread.sleep(600)
            }
        }
        throw lastError ?: RuntimeException("transaction failed")
    }

    companion object {
        fun fromSharedSpec(): Pair<SolanaService, TokenDevnetSpec> {
            val spec = SharedSpec.loadToken()
            return SolanaService(spec.rpcUrl) to spec
        }
    }
}
