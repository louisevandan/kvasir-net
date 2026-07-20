package ai.banya.linkcpp.core

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

@Serializable
data class PayModel(val id: String, val name: String)

@Serializable
data class PayModels(val recipient: String, val mint: String, val symbol: String, val models: List<PayModel>)

@Serializable
data class PaymentQuote(
    val requestId: String, val model: String, val priceToken: Double,
    val recipient: String, val mint: String, val symbol: String,
    val estimated: Boolean = false,
    val estPromptTokens: Int? = null, val estCompletionTokens: Int? = null, val estTotalTokens: Int? = null,
)

/** Actual token usage reported after an inference run. */
@Serializable
data class TokenUsage(
    val promptTokens: Int, val completionTokens: Int, val totalTokens: Int, val costToken: Double? = null,
)

@Serializable
data class InferenceResult(
    val requestId: String, val paid: Boolean, val signature: String? = null,
    val model: String? = null, val priceToken: Double? = null, val result: String,
    val usage: TokenUsage? = null,
)

/** HTTP client for the Kvasir inference-payment gateway (Phase 2). */
class GatewayService(baseUrl: String) {
    private val http = HttpJson(baseUrl)
    private inline fun <reified T> get(path: String): T = http.json.decodeFromString(http.getRaw(path))
    private inline fun <reified T> post(path: String, body: String): T = http.json.decodeFromString(http.postRaw(path, body))

    fun models(): PayModels = get("/api/pay/models")

    fun quote(model: String, prompt: String): PaymentQuote =
        post("/api/pay/quote", buildJsonObject { put("model", model); put("prompt", prompt) }.toString())

    fun infer(requestId: String, signature: String): InferenceResult =
        post("/api/inference", buildJsonObject { put("requestId", requestId); put("signature", signature) }.toString())
}
