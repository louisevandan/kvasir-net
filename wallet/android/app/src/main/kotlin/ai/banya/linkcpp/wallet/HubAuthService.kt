package ai.banya.linkcpp.wallet

import ai.banya.linkcpp.core.WalletDeriver
import android.util.Base64
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/**
 * Sign-In With Solana against a Kvasir hub, to obtain the bearer token an
 * autonomous node uses to poll/enroll on an auth-gated (public, remote) hub.
 *
 * Flow (see controller/siws.py, controller/hub.py):
 *   1. POST /api/auth/challenge {wallet}         -> {nonce, message}
 *   2. sign message bytes with the wallet key    -> base64 signature (ed25519)
 *   3. POST /api/auth/verify {wallet,nonce,sig}  -> session cookie, OR
 *                                                   {twofa_required, pre_auth}
 *   4. POST /api/auth/2fa/login {pre_auth,code}  -> session cookie
 * The session cookie value (linkcpp_session) doubles as the Authorization
 * bearer token the hub accepts (_authed_wallet).
 */
class HubAuthService(baseUrl: String, private val mnemonic: List<String>) {
    private val base = baseUrl.trimEnd('/')
    private val sessionCookie = "linkcpp_session"

    private val keypair get() = WalletDeriver.keypair(mnemonic)
    val wallet: String get() = keypair.publicKey.toBase58()

    /**
     * Mint a long-lived NODE token with a wallet signature alone — no OTP.
     * The node token is scoped to participation, so the hub skips 2FA (which a
     * mobile wallet has no UI for). Returns the bearer token the node polls with.
     */
    fun nodeToken(): String {
        val kp = keypair
        val w = kp.publicKey.toBase58()
        val ch = postJson("/api/auth/challenge", JSONObject().put("wallet", w))
        val message = ch.optString("message"); val nonce = ch.optString("nonce")
        if (message.isEmpty() || nonce.isEmpty()) error("hub did not issue a challenge")
        val sigB64 = Base64.encodeToString(kp.sign(message.toByteArray(Charsets.UTF_8)), Base64.NO_WRAP)
        val resp = postJson("/api/auth/node-token",
            JSONObject().put("wallet", w).put("nonce", nonce).put("signature", sigB64))
        return resp.optString("node_token").ifEmpty { error("hub issued no node token") }
    }

    private fun postJson(path: String, body: JSONObject): JSONObject {
        val conn = (URL(base + path).openConnection() as HttpURLConnection).apply {
            requestMethod = "POST"; connectTimeout = 12000; readTimeout = 20000; doOutput = true
            setRequestProperty("Content-Type", "application/json")
        }
        try {
            conn.outputStream.use { it.write(body.toString().toByteArray(Charsets.UTF_8)) }
            val code = conn.responseCode
            val text = (if (code in 200..299) conn.inputStream else conn.errorStream)
                ?.bufferedReader()?.use { it.readText() } ?: ""
            val obj = runCatching { JSONObject(text) }.getOrDefault(JSONObject())
            if (code !in 200..299) error(obj.optString("error", "HTTP $code"))
            return obj
        } finally {
            conn.disconnect()
        }
    }
}
