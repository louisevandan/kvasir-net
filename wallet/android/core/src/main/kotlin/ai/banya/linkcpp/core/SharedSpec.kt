package ai.banya.linkcpp.core

import kotlinx.serialization.json.Json

/** Loads the cross-platform shared spec bundled as classpath resources. */
object SharedSpec {
    private val json = Json { ignoreUnknownKeys = true; isLenient = true }

    private fun resource(name: String): String =
        SharedSpec::class.java.getResourceAsStream("/$name")
            ?.bufferedReader()?.use { it.readText() }
            ?: error("resource /$name not found on classpath")

    fun loadToken(): TokenDevnetSpec = json.decodeFromString(resource("token.devnet.json"))
    fun loadConstants(): WalletConstants = json.decodeFromString(resource("wallet-constants.json"))
}
