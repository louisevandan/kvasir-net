package ai.banya.linkcpp.wallet

import android.content.Context
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey

/** Non-custodial secret storage: the mnemonic is encrypted at rest (Android Keystore). */
class KeyStore(context: Context) {
    private val prefs = run {
        val master = MasterKey.Builder(context)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build()
        EncryptedSharedPreferences.create(
            context,
            "linkcpp_wallet",
            master,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM,
        )
    }

    fun saveMnemonic(words: List<String>) {
        prefs.edit().putString(KEY, words.joinToString(" ")).apply()
    }

    fun loadMnemonic(): List<String>? =
        prefs.getString(KEY, null)?.trim()?.split(Regex("\\s+"))?.filter { it.isNotEmpty() }

    fun exists(): Boolean = prefs.contains(KEY)
    fun clear() { prefs.edit().remove(KEY).apply() }

    // Gateway credit API key (bearer credential for the wallet's prepaid balance),
    // encrypted at rest and keyed by wallet address. Re-mintable, so losing it is
    // recoverable via a fresh SIWS mint.
    fun saveApiKey(wallet: String, key: String) { prefs.edit().putString("apikey:$wallet", key).apply() }
    fun loadApiKey(wallet: String): String? = prefs.getString("apikey:$wallet", null)?.takeIf { it.isNotEmpty() }
    fun deleteApiKey(wallet: String) { prefs.edit().remove("apikey:$wallet").apply() }

    private companion object { const val KEY = "mnemonic" }
}
