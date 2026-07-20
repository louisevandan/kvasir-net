package ai.banya.linkcpp.core

import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

/**
 * SLIP-0010 ed25519 hierarchical key derivation. ed25519 supports hardened
 * derivation only, so every path segment is hardened. Matches the ecosystem
 * standard (ed25519-hd-key) used by Phantom / @solana/web3.js and by the iOS app.
 */
object Slip10 {
    fun derive(path: String, seed: ByteArray): ByteArray {
        var i = hmac("ed25519 seed".toByteArray(Charsets.UTF_8), seed)
        var key = i.copyOfRange(0, 32)
        var chain = i.copyOfRange(32, 64)
        for (segment in parse(path)) {
            val index = segment or 0x80000000.toInt() // hardened
            val data = ByteArray(37)
            data[0] = 0
            System.arraycopy(key, 0, data, 1, 32)
            data[33] = (index ushr 24).toByte()
            data[34] = (index ushr 16).toByte()
            data[35] = (index ushr 8).toByte()
            data[36] = index.toByte()
            i = hmac(chain, data)
            key = i.copyOfRange(0, 32)
            chain = i.copyOfRange(32, 64)
        }
        return key
    }

    private fun parse(path: String): List<Int> =
        path.removePrefix("m/").split("/").map { it.removeSuffix("'").toInt() }

    private fun hmac(key: ByteArray, data: ByteArray): ByteArray {
        val mac = Mac.getInstance("HmacSHA512")
        mac.init(SecretKeySpec(key, "HmacSHA512"))
        return mac.doFinal(data)
    }
}
