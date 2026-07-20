package ai.banya.linkcpp.core

import java.security.MessageDigest
import java.security.SecureRandom
import javax.crypto.SecretKeyFactory
import javax.crypto.spec.PBEKeySpec

/** BIP39 mnemonic generation, validation, and seed derivation (self-contained). */
object Bip39 {
    val words: List<String> by lazy {
        (Bip39::class.java.getResourceAsStream("/bip39-english.txt")
            ?: error("bip39 wordlist missing")).bufferedReader().readLines()
            .map { it.trim() }.filter { it.isNotEmpty() }
    }

    fun generate(wordCount: Int = 12): List<String> {
        require(wordCount == 12 || wordCount == 24) { "wordCount must be 12 or 24" }
        val entropy = ByteArray(if (wordCount == 12) 16 else 32)
        SecureRandom().nextBytes(entropy)
        return fromEntropy(entropy)
    }

    fun fromEntropy(entropy: ByteArray): List<String> {
        val csBits = entropy.size * 8 / 32
        val bits = StringBuilder()
        for (b in entropy) bits.append(bitsOf(b))
        val hashBits = StringBuilder()
        for (b in sha256(entropy)) hashBits.append(bitsOf(b))
        bits.append(hashBits.substring(0, csBits))
        val out = ArrayList<String>()
        var i = 0
        while (i < bits.length) { out.add(words[bits.substring(i, i + 11).toInt(2)]); i += 11 }
        return out
    }

    fun isValid(mnemonic: List<String>): Boolean {
        if (mnemonic.size !in intArrayOf(12, 15, 18, 21, 24)) return false
        val idx = mnemonic.map { words.indexOf(it) }
        if (idx.any { it < 0 }) return false
        val bits = StringBuilder()
        for (n in idx) bits.append(n.toString(2).padStart(11, '0'))
        val entBits = mnemonic.size * 11 * 32 / 33
        val csBits = mnemonic.size * 11 - entBits
        val entropy = bytesOf(bits.substring(0, entBits))
        val hashBits = StringBuilder()
        for (b in sha256(entropy)) hashBits.append(bitsOf(b))
        return bits.substring(entBits) == hashBits.substring(0, csBits)
    }

    /** BIP39 seed: PBKDF2-HMAC-SHA512 over the mnemonic, 2048 iterations, 512-bit. */
    fun toSeed(mnemonic: List<String>, passphrase: String = ""): ByteArray {
        val spec = PBEKeySpec(
            mnemonic.joinToString(" ").toCharArray(),
            "mnemonic$passphrase".toByteArray(Charsets.UTF_8),
            2048, 512,
        )
        return SecretKeyFactory.getInstance("PBKDF2WithHmacSHA512").generateSecret(spec).encoded
    }

    fun words(from: String): List<String> =
        from.trim().split(Regex("\\s+")).filter { it.isNotEmpty() }

    private fun bitsOf(b: Byte): String = (b.toInt() and 0xff).toString(2).padStart(8, '0')
    private fun bytesOf(bits: String): ByteArray {
        val out = ByteArray(bits.length / 8)
        for (i in out.indices) out[i] = bits.substring(i * 8, i * 8 + 8).toInt(2).toByte()
        return out
    }
    private fun sha256(data: ByteArray): ByteArray = MessageDigest.getInstance("SHA-256").digest(data)
}
