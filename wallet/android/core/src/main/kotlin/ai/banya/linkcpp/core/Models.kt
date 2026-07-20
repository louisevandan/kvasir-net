package ai.banya.linkcpp.core

import kotlinx.serialization.Serializable

// ---- shared-spec/token.devnet.json ----
@Serializable
data class TokenInfo(val name: String? = null, val symbol: String, val decimals: Int, val mint: String)

@Serializable
data class Treasury(val owner: String, val ata: String)

@Serializable
data class TokenDevnetSpec(
    val cluster: String,
    val rpcUrl: String,
    val token: TokenInfo,
    val treasury: Treasury,
)

// ---- shared-spec/wallet-constants.json ----
@Serializable
data class Derivation(val scheme: String, val path: String, val coinType: Int)

@Serializable
data class Cluster(val rpcUrl: String, val explorerCluster: String)

@Serializable
data class WalletConstants(
    val derivation: Derivation,
    val activeCluster: String,
    val clusters: Map<String, Cluster>,
    val stakingServiceUrl: String? = null,
)

// ---- runtime models ----
data class AssetBalance(val symbol: String, val amount: Double, val raw: Long, val decimals: Int)

data class TxRef(val signature: String, val blockTime: Long?, val failed: Boolean)
