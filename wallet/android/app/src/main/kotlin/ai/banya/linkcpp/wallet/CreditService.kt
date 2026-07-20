package ai.banya.linkcpp.wallet

import ai.banya.linkcpp.core.TokenUsage
import ai.banya.linkcpp.core.WalletDeriver
import android.util.Base64
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/**
 * Prepaid-credit gateway client: SIWS self-registration + API-key minting, KVR
 * credit deposits, balance, and **streaming** OpenAI-compatible chat.
 *
 * Streaming is why this exists: the non-streaming /api/inference route is cut off
 * by Cloudflare's fixed 100s origin timeout on slow models (M3 ~1 tok/s) => 524.
 * /v1/chat/completions with stream:true flows SSE from the first token, so the
 * connection never idles out. Auth mirrors HubAuthService (ed25519, base64 sig);
 * credits are debited per completion's usage.
 */
class CreditService(baseUrl: String) {
    private val base = baseUrl.trimEnd('/')

    sealed class ChatEvent {
        data class Token(val text: String) : ChatEvent()
        data class Usage(val usage: TokenUsage) : ChatEvent()
    }

    // MARK: SIWS onboarding

    /** Self-register the wallet into the credit whitelist (gateway must have
     *  CREDIT_OPEN_REGISTER on). Idempotent server-side. */
    fun register(mnemonic: List<String>) {
        val kp = WalletDeriver.keypair(mnemonic)
        val w = kp.publicKey.toBase58()
        val ch = postJson("/api/credits/register/challenge", JSONObject().put("wallet", w))
        val message = ch.optString("message"); val nonce = ch.optString("nonce")
        if (message.isEmpty() || nonce.isEmpty()) error("gateway issued no register challenge")
        val sig = Base64.encodeToString(kp.sign(message.toByteArray(Charsets.UTF_8)), Base64.NO_WRAP)
        postJson("/api/credits/register", JSONObject().put("wallet", w).put("nonce", nonce).put("signature", sig))
    }

    /** Mint an API key bound to the wallet. Returned once — the caller must store it. */
    fun mintApiKey(mnemonic: List<String>, label: String): String {
        val kp = WalletDeriver.keypair(mnemonic)
        val w = kp.publicKey.toBase58()
        val ch = postJson("/api/credits/challenge", JSONObject().put("wallet", w))
        val message = ch.optString("message"); val nonce = ch.optString("nonce")
        if (message.isEmpty() || nonce.isEmpty()) error("gateway issued no api-key challenge")
        val sig = Base64.encodeToString(kp.sign(message.toByteArray(Charsets.UTF_8)), Base64.NO_WRAP)
        val resp = postJson("/api/credits/apikey",
            JSONObject().put("wallet", w).put("nonce", nonce).put("signature", sig).put("label", label))
        return resp.optString("apiKey").ifEmpty { error("gateway minted no api key") }
    }

    /** Credit a prior on-chain KVR transfer (caller sends it, passes the signature).
     *  Returns the new balance. */
    fun deposit(wallet: String, amount: Double, signature: String): Double {
        val resp = postJson("/api/credits/deposit",
            JSONObject().put("wallet", wallet).put("amount", amount).put("signature", signature))
        return resp.optDouble("balance", 0.0)
    }

    /** Current credit balance for the wallet behind this API key. */
    fun balance(apiKey: String): Double {
        val conn = (URL("$base/api/credits/balance").openConnection() as HttpURLConnection).apply {
            requestMethod = "GET"; connectTimeout = 12000; readTimeout = 20000
            setRequestProperty("Authorization", "Bearer $apiKey")
        }
        try {
            val code = conn.responseCode
            val text = (if (code in 200..299) conn.inputStream else conn.errorStream)?.bufferedReader()?.use { it.readText() } ?: ""
            val obj = runCatching { JSONObject(text) }.getOrDefault(JSONObject())
            if (code !in 200..299) error(obj.optString("error", "HTTP $code"))
            return obj.optDouble("balance", 0.0)
        } finally { conn.disconnect() }
    }

    // MARK: streaming chat

    /** Stream an OpenAI-compatible completion. Emits content deltas as they arrive
     *  so the UI appends incrementally (and Cloudflare sees continuous data). Collect
     *  with `.flowOn(Dispatchers.IO)`. */
    fun streamChat(apiKey: String, model: String, messages: List<Pair<String, String>>, maxTokens: Int = 1024): Flow<ChatEvent> = flow {
        val msgs = JSONArray()
        for ((role, content) in messages) msgs.put(JSONObject().put("role", role).put("content", content))
        val body = JSONObject()
            .put("model", model).put("messages", msgs).put("stream", true).put("max_tokens", maxTokens)
        val conn = (URL("$base/v1/chat/completions").openConnection() as HttpURLConnection).apply {
            requestMethod = "POST"; doOutput = true
            connectTimeout = 15000
            readTimeout = 600000   // idle timeout between chunks; resets while data flows
            setRequestProperty("Content-Type", "application/json")
            setRequestProperty("Accept", "text/event-stream")
            setRequestProperty("Authorization", "Bearer $apiKey")
        }
        try {
            conn.outputStream.use { it.write(body.toString().toByteArray(Charsets.UTF_8)) }
            val code = conn.responseCode
            if (code !in 200..299) {
                val err = conn.errorStream?.bufferedReader()?.use { it.readText() } ?: ""
                error(errorText(err, code))
            }
            val reader = conn.inputStream.bufferedReader()
            try {
                while (true) {
                    val line = reader.readLine() ?: break
                    if (!line.startsWith("data:")) continue
                    val payload = line.substring(5).trim()
                    if (payload == "[DONE]") break
                    val obj = runCatching { JSONObject(payload) }.getOrNull() ?: continue
                    obj.optJSONArray("choices")?.optJSONObject(0)?.optJSONObject("delta")?.let { delta ->
                        // org.json's optString returns the literal "null" for a JSON
                        // null value (reasoning models send content:null while thinking),
                        // so guard on isNull before reading.
                        if (!delta.isNull("content")) {
                            val c = delta.optString("content", "")
                            if (c.isNotEmpty()) emit(ChatEvent.Token(c))
                        }
                        // reasoning_content is suppressed server-side; ignore any leak.
                    }
                    obj.optJSONObject("usage")?.let { u ->
                        emit(ChatEvent.Usage(TokenUsage(
                            promptTokens = u.optInt("prompt_tokens", 0),
                            completionTokens = u.optInt("completion_tokens", 0),
                            totalTokens = u.optInt("total_tokens", 0))))
                    }
                }
            } finally { reader.close() }
        } finally { conn.disconnect() }
    }

    // MARK: helpers

    private fun errorText(raw: String, code: Int): String {
        val obj = runCatching { JSONObject(raw) }.getOrNull() ?: return if (raw.isEmpty()) "HTTP $code" else raw
        obj.optJSONObject("error")?.optString("message")?.takeIf { it.isNotEmpty() }?.let { return it }
        obj.optString("error").takeIf { it.isNotEmpty() }?.let { return it }
        return if (raw.isEmpty()) "HTTP $code" else raw
    }

    private fun postJson(path: String, body: JSONObject): JSONObject {
        val conn = (URL(base + path).openConnection() as HttpURLConnection).apply {
            requestMethod = "POST"; connectTimeout = 12000; readTimeout = 30000; doOutput = true
            setRequestProperty("Content-Type", "application/json")
        }
        try {
            conn.outputStream.use { it.write(body.toString().toByteArray(Charsets.UTF_8)) }
            val code = conn.responseCode
            val text = (if (code in 200..299) conn.inputStream else conn.errorStream)?.bufferedReader()?.use { it.readText() } ?: ""
            val obj = runCatching { JSONObject(text) }.getOrDefault(JSONObject())
            if (code !in 200..299) error(obj.optString("error", "HTTP $code"))
            return obj
        } finally { conn.disconnect() }
    }
}
