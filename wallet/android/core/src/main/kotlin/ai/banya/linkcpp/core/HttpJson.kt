package ai.banya.linkcpp.core

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.net.HttpURLConnection
import java.net.URL

/**
 * Minimal JSON-over-HTTP helper shared by the staking + gateway clients.
 * Uses HttpURLConnection so it runs on both the JVM and Android (java.net.http
 * is not available on Android).
 */
internal class HttpJson(baseUrl: String) {
    private val base = baseUrl.trimEnd('/')
    val json = Json { ignoreUnknownKeys = true; isLenient = true }

    fun getRaw(path: String): String = request("GET", path, null)
    fun postRaw(path: String, body: String): String = request("POST", path, body)

    private fun request(method: String, path: String, body: String?): String {
        val conn = (URL(base + path).openConnection() as HttpURLConnection).apply {
            requestMethod = method
            connectTimeout = 15000
            // Inference can run for minutes on very large / paging-backed models
            // (e.g. a 428B MoE served from disk); allow up to 3 minutes to match
            // the gateway's inference timeout before the client gives up.
            readTimeout = 180000
            if (body != null) {
                doOutput = true
                setRequestProperty("Content-Type", "application/json")
            }
        }
        try {
            if (body != null) conn.outputStream.use { it.write(body.toByteArray(Charsets.UTF_8)) }
            val code = conn.responseCode
            val stream = if (code in 200..299) conn.inputStream else (conn.errorStream ?: conn.inputStream)
            val text = stream.bufferedReader().use { it.readText() }
            if (code !in 200..299) {
                val msg = runCatching {
                    json.parseToJsonElement(text).jsonObject["error"]?.jsonPrimitive?.content
                }.getOrNull()
                throw RuntimeException("HTTP $code: ${msg ?: text}")
            }
            return text
        } finally {
            conn.disconnect()
        }
    }
}
