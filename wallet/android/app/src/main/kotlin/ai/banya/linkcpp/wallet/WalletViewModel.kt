package ai.banya.linkcpp.wallet

import android.app.Application
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import ai.banya.linkcpp.core.AssetBalance
import ai.banya.linkcpp.core.Bip39
import ai.banya.linkcpp.core.SharedSpec
import ai.banya.linkcpp.core.SolanaService
import ai.banya.linkcpp.core.TxRef
import ai.banya.linkcpp.core.WalletDeriver
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.sol4k.Keypair

class WalletViewModel(app: Application) : AndroidViewModel(app) {

    enum class Net(val id: String, val display: String) {
        DEVNET("devnet", "Devnet"), MAINNET("mainnet-beta", "Mainnet")
    }

    private val keyStore = KeyStore(app)
    private val constants = SharedSpec.loadConstants()
    private val tokenSpec = SharedSpec.loadToken()
    private val prefs = app.getSharedPreferences("linkcpp_cfg", android.content.Context.MODE_PRIVATE)

    var address by mutableStateOf<String?>(null); private set
    var sol by mutableStateOf<AssetBalance?>(null); private set
    var token by mutableStateOf<AssetBalance?>(null); private set
    var txs by mutableStateOf<List<TxRef>>(emptyList()); private set
    var loading by mutableStateOf(false); private set
    var error by mutableStateOf<String?>(null); private set
    var network by mutableStateOf(Net.DEVNET); private set
    var restoring by mutableStateOf(false); private set
    var stakingUrl by mutableStateOf(prefs.getString("stakingUrl", null) ?: (constants.stakingServiceUrl ?: "")); private set

    // node compute configuration
    var nodeBackend by mutableStateOf(prefs.getString("nodeBackend", "opencl")!!); private set   // opencl | vulkan | cpu
    var nodeMode by mutableStateOf(prefs.getString("nodeMode", "local_shard")!!); private set     // local_shard | rpc_worker
    var nodeChargingOnly by mutableStateOf(prefs.getBoolean("nodeChargingOnly", true)); private set
    // Persisted so the node auto-connects to its known hubs on the next launch:
    // once live, it resumes as soon as the wallet is restored (see activate()).
    var nodeLive by mutableStateOf(prefs.getBoolean("nodeLive", false)); private set

    // in-app language (reactive; drives LocalStrings via MainActivity)
    var language by mutableStateOf(defaultLanguage()); private set
    private val strings: Strings get() = Strings(language)

    private fun defaultLanguage(): AppLanguage {
        val saved = prefs.getString("app.language", null)
        AppLanguage.values().firstOrNull { it.code == saved }?.let { return it }
        // JVM/Android reports Indonesian as the legacy code "in"; normalize to "id".
        val device = java.util.Locale.getDefault().language.let { if (it == "in") "id" else it }
        return AppLanguage.values().firstOrNull { it.code == device } ?: AppLanguage.EN
    }

    fun switchLanguage(l: AppLanguage) {
        language = l
        prefs.edit().putString("app.language", l.code).apply()
    }

    private var keypair: Keypair? = null
    private var service = SolanaService(rpcFor(Net.DEVNET))

    val hasWallet: Boolean get() = keyStore.exists()
    val hasToken: Boolean get() = network == Net.DEVNET
    val tokenSymbol: String get() = tokenSpec.token.symbol

    fun updateStakingUrl(url: String) {
        val t = url.trim()
        stakingUrl = t
        prefs.edit().putString("stakingUrl", t).apply()
    }

    /** The shipped default (genesis gateway) from shared-spec. */
    val defaultStakingUrl: String get() = constants.stakingServiceUrl ?: ""
    /** True once the user pinned their own settlement URL — genesis discovery must
     *  not override a manual choice (e.g. a temporary LAN/IP override). */
    val hasCustomStakingUrl: Boolean get() = prefs.getString("stakingUrl", null) != null

    /** Genesis discovery: adopt the gateway's advertised publicUrl unless the user
     *  pinned a custom settlement URL. Mirrors the desktop app. */
    fun adoptGenesisUrl(publicUrl: String) {
        if (hasCustomStakingUrl) return
        val t = publicUrl.trim()
        if (t.isEmpty() || t == stakingUrl) return
        stakingUrl = t
        prefs.edit().putString("stakingUrl", t).apply()
    }

    fun updateNodeBackend(b: String) { nodeBackend = b; prefs.edit().putString("nodeBackend", b).apply() }
    fun updateNodeMode(m: String) { nodeMode = m; prefs.edit().putString("nodeMode", m).apply(); syncNodeService() }
    fun updateNodeChargingOnly(v: Boolean) { nodeChargingOnly = v; prefs.edit().putBoolean("nodeChargingOnly", v).apply() }
    fun updateNodeLive(v: Boolean) {
        nodeLive = v
        prefs.edit().putBoolean("nodeLive", v).apply()
        syncNodeService()
    }

    /// Run the node data plane (foreground service + agent) whenever live. The
    /// agent serves the hub control protocol for BOTH node modes — local_shard
    /// (ring stage via /control/proxy/stage/start) and rpc_worker (tensor
    /// executor via /control/load) — and drives the autonomous shard-demand
    /// poll, so it must run in either mode. The service survives a screen lock,
    /// so the hub can reach this phone on demand.
    private fun syncNodeService() {
        val app = getApplication<Application>()
        if (nodeLive) NodeService.start(app, address ?: "")
        else NodeService.stop(app)
    }

    /// Live status of the spawned node agent, for the UI.
    val nodeAgentRunning: Boolean get() = NodeService.agent?.running == true
    val nodeAgentEvent: String get() = NodeService.agent?.lastEvent ?: ""
    val nodeAgentPort: Int get() = NodeService.agent?.agentPort ?: 9101

    /// Hub-connection status for the node dashboard. The known hubs come from the
    /// running agent when live, or from persisted config otherwise, so the card
    /// shows which hubs the node auto-connects to even before it starts.
    val nodeConnected: Boolean get() = nodeLive && NodeService.agent?.running == true
    val nodeServing: Boolean get() = NodeService.agent?.serving == true
    val nodeHubUrls: List<String> get() =
        NodeService.agent?.knownHubUrls?.takeIf { it.isNotEmpty() } ?: configuredHubUrls()

    private fun configuredHubUrls(): List<String> = runCatching {
        val prefs = getApplication<Application>().getSharedPreferences("kvasir-node", android.content.Context.MODE_PRIVATE)
        val arr = org.json.JSONArray(prefs.getString("configuredHubs", "[]"))
        (0 until arr.length()).mapNotNull { arr.optJSONObject(it)?.optString("url")?.takeIf { u -> u.isNotEmpty() } }
    }.getOrDefault(emptyList())

    private fun rpcFor(n: Net) = constants.clusters[n.id]?.rpcUrl ?: tokenSpec.rpcUrl

    fun newMnemonic(): List<String> = Bip39.generate(12)

    /** Reveal the stored BIP39 mnemonic so the user can back it up / import the same account elsewhere. */
    fun revealMnemonic(): List<String>? = keyStore.loadMnemonic()

    fun restoreIfNeeded() {
        if (address != null) return
        val words = keyStore.loadMnemonic() ?: return
        restoring = true
        activate(words)
    }

    fun saveAndActivate(words: List<String>, onError: (String) -> Unit = {}, onSuccess: () -> Unit = {}) {
        val normalized = words.map { it.lowercase() }
        if (!Bip39.isValid(normalized)) { onError(strings.t("error.invalidMnemonic")); return }
        keyStore.saveMnemonic(normalized)
        activate(normalized)
        onSuccess()
    }

    private fun activate(words: List<String>) {
        viewModelScope.launch {
            try {
                val kp = withContext(Dispatchers.IO) { WalletDeriver.keypair(words) }
                keypair = kp
                address = kp.publicKey.toBase58()
                // Auto-connect: if the operator left the node live, resume it now
                // that the wallet (owner) is available — it reloads its known hubs
                // and re-connects to them without any manual step.
                if (nodeLive) syncNodeService()
                refresh()
            } catch (e: Exception) {
                error = e.message
            } finally {
                restoring = false
            }
        }
    }

    fun logout() {
        keyStore.clear()
        address = null; sol = null; token = null; txs = emptyList(); keypair = null
    }

    fun switchNetwork(n: Net) {
        if (n == network) return
        network = n
        service = SolanaService(rpcFor(n))
        token = null; sol = null; txs = emptyList()
        refresh()
    }

    fun refresh() {
        val addr = address ?: return
        viewModelScope.launch {
            loading = true
            try {
                val result = withContext(Dispatchers.IO) {
                    Triple(
                        service.solBalance(addr),
                        if (hasToken) service.tokenBalance(
                            addr, tokenSpec.token.mint, tokenSpec.token.symbol, tokenSpec.token.decimals
                        ) else null,
                        service.recentTransactions(addr, 50),
                    )
                }
                sol = result.first; token = result.second; txs = result.third
                error = null
            } catch (e: Exception) {
                error = e.message
            } finally {
                loading = false
            }
        }
    }

    suspend fun sendToken(to: String, amount: Double): String = withContext(Dispatchers.IO) {
        val kp = keypair ?: throw IllegalStateException("no wallet")
        service.sendToken(kp, tokenSpec.token.mint, tokenSpec.token.decimals, to, amount)
    }

    suspend fun sendSol(to: String, amount: Double): String = withContext(Dispatchers.IO) {
        val kp = keypair ?: throw IllegalStateException("no wallet")
        service.sendSol(kp, to, amount)
    }

    fun explorerUrl(sig: String): String {
        val cluster = constants.clusters[network.id]?.explorerCluster ?: network.id
        return "https://explorer.solana.com/tx/$sig?cluster=$cluster"
    }

    // MARK: prepaid-credit inference (streaming over /v1/chat/completions)

    private val credit get() = CreditService(stakingUrl)

    /** Ensure the wallet has a credit API key, self-registering + minting one on
     *  first use (both steps sign a gateway nonce with the wallet key). */
    suspend fun creditApiKey(): String = withContext(Dispatchers.IO) {
        val w = address ?: throw IllegalStateException("wallet locked")
        keyStore.loadApiKey(w)?.let { return@withContext it }
        val phrase = keyStore.loadMnemonic() ?: throw IllegalStateException("wallet locked")
        credit.register(phrase)                                   // self-whitelist (idempotent)
        val key = credit.mintApiKey(phrase, "Kvasir Android")
        keyStore.saveApiKey(w, key)
        key
    }

    /** Discard the stored credit API key and mint a new one.
     *
     *  Only the key's hash is kept by the gateway, so a key it no longer
     *  recognises cannot be repaired by asking for it back — it has to be
     *  reminted. Without this, "invalid API key" has no way out from inside the
     *  app. Credit balance is held against the wallet, not the key, so nothing
     *  is lost by reissuing. */
    suspend fun reissueCreditApiKey(): String = withContext(Dispatchers.IO) {
        val w = address ?: throw IllegalStateException("wallet locked")
        val phrase = keyStore.loadMnemonic() ?: throw IllegalStateException("wallet locked")
        keyStore.deleteApiKey(w)
        credit.register(phrase)                                   // self-whitelist (idempotent)
        val key = credit.mintApiKey(phrase, "Kvasir Android")
        keyStore.saveApiKey(w, key)
        key
    }

    /** Current credit balance, or null if there is no API key yet. */
    suspend fun creditBalance(): Double? = withContext(Dispatchers.IO) {
        val w = address ?: return@withContext null
        val key = keyStore.loadApiKey(w) ?: return@withContext null
        runCatching { credit.balance(key) }.getOrNull()
    }

    /** Top up credits: transfer KVR on-chain to the gateway vault, then prove it.
     *  Returns the new balance. */
    suspend fun topUpCredits(amount: Double, recipient: String): Double {
        val w = address ?: throw IllegalStateException("wallet locked")
        creditApiKey()                                           // must be whitelisted before deposit
        val sig = sendToken(recipient, amount)
        return withContext(Dispatchers.IO) { credit.deposit(w, amount, sig) }
    }

    /** Stream a completion over the credit-billed endpoint (collect on Dispatchers.IO). */
    fun creditStream(apiKey: String, model: String, messages: List<Pair<String, String>>) =
        credit.streamChat(apiKey, model, messages)
}
