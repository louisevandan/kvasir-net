package ai.banya.linkcpp.core

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

// ---- models (mirror the staking-service JSON contract) ----
@Serializable
data class PerfTier(val tier: String, val minTps: Double, val mult: Double)

@Serializable
data class StakingConfig(
    val cluster: String, val mint: String, val decimals: Int,
    val vault: String, val vaultOwner: String, val symbol: String,
    val aprPercent: Double, val rewardPerUnit: Double,
    val perfTiers: List<PerfTier> = emptyList(),
    // Genesis discovery: the canonical URL the gateway advertises for itself.
    // Clients on the shipped default auto-adopt it; null means not yet public.
    val gatewayBonus: Double? = null,
    val publicUrl: String? = null,
    val rpcUrl: String? = null,
)

@Serializable
data class StakePosition(
    val owner: String, val principal: Double, val rewards: Double,
    val total: Double, val aprPercent: Double, val stakedAt: Double? = null,
)

@Serializable
data class UnstakeResult(
    val signature: String, val principalReturned: Double,
    val rewardsPaid: Double, val total: Double,
)

@Serializable
data class NodeReward(val nodeId: String, val contributedUnits: Double, val pendingRewards: Double)

@Serializable
data class NodeRewards(
    val owner: String, val pending: Double, val rewardPerUnit: Double, val nodes: List<NodeReward>,
)

@Serializable
data class ClaimResult(val signature: String, val claimed: Double)

@Serializable
data class NodeRegisterResult(
    val nodeId: String, val owner: String, val os: String? = null,
    val deviceKind: String? = null, val accelerator: String? = null, val label: String? = null,
    val perfScore: Double = 0.0, val backend: String? = null, val mode: String? = null,
    val tier: String? = null, val perfMultiplier: Double = 1.0,
)

@Serializable
data class NodeStatusItem(
    val nodeId: String, val status: String, val os: String? = null,
    val deviceKind: String? = null, val accelerator: String? = null, val label: String? = null,
    val perfScore: Double = 0.0, val backend: String? = null, val mode: String? = null,
    val tier: String? = null, val perfMultiplier: Double = 1.0,
    val contributedUnits: Double, val effectiveUnits: Double = 0.0,
    val pendingRewards: Double, val claimedTotal: Double,
    val registeredAt: Double? = null, val lastReport: Double? = null,
)

@Serializable
data class NodeStatusTotals(
    val nodes: Int, val online: Int, val contributedUnits: Double,
    val effectiveUnits: Double = 0.0,
    val pending: Double, val claimedTotal: Double, val lifetimeRewards: Double,
)

@Serializable
data class NodeStatus(val owner: String, val totals: NodeStatusTotals, val nodes: List<NodeStatusItem>)

/** HTTP client for the off-chain staking + node-rewards settlement service. */
class StakingService(baseUrl: String) {
    private val http = HttpJson(baseUrl)
    private inline fun <reified T> get(path: String): T = http.json.decodeFromString(http.getRaw(path))
    private inline fun <reified T> post(path: String, body: String): T = http.json.decodeFromString(http.postRaw(path, body))

    fun config(): StakingConfig = get("/api/config")
    fun position(owner: String): StakePosition = get("/api/positions/$owner")

    fun stake(owner: String, amount: Double, signature: String): StakePosition =
        post("/api/stake", buildJsonObject {
            put("owner", owner); put("amount", amount); put("signature", signature)
        }.toString())

    fun unstake(owner: String, amount: Double? = null): UnstakeResult =
        post("/api/unstake", buildJsonObject {
            put("owner", owner); if (amount != null) put("amount", amount)
        }.toString())

    fun registerNode(
        nodeId: String, owner: String, os: String? = null,
        deviceKind: String? = null, accelerator: String? = null, label: String? = null,
        perfScore: Double? = null, backend: String? = null, mode: String? = null,
    ): NodeRegisterResult = post("/api/node/register", buildJsonObject {
        put("nodeId", nodeId); put("owner", owner)
        os?.let { put("os", it) }; deviceKind?.let { put("deviceKind", it) }
        accelerator?.let { put("accelerator", it) }; label?.let { put("label", it) }
        perfScore?.let { put("perfScore", it) }; backend?.let { put("backend", it) }
        mode?.let { put("mode", it) }
    }.toString())

    fun nodeRewards(owner: String): NodeRewards = get("/api/node/rewards/$owner")
    fun nodeStatus(owner: String): NodeStatus = get("/api/node/status/$owner")
    fun claimNodeRewards(owner: String): ClaimResult =
        post("/api/node/claim", buildJsonObject { put("owner", owner) }.toString())

    fun removeNode(nodeId: String, owner: String): String =
        http.postRaw("/api/node/remove", buildJsonObject {
            put("nodeId", nodeId); put("owner", owner)
        }.toString())

    fun heartbeat(nodeId: String) {
        http.postRaw("/api/node/heartbeat", buildJsonObject { put("nodeId", nodeId) }.toString())
    }
}
