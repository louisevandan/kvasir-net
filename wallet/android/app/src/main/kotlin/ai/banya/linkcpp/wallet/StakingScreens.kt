package ai.banya.linkcpp.wallet

import android.os.Build
import android.provider.Settings
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.navigation.NavController
import ai.banya.linkcpp.core.NodeReward
import ai.banya.linkcpp.core.NodeRewards
import ai.banya.linkcpp.core.NodeStatus
import ai.banya.linkcpp.core.NodeStatusItem
import ai.banya.linkcpp.core.StakePosition
import ai.banya.linkcpp.core.StakingConfig
import ai.banya.linkcpp.core.StakingService
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

@Composable
fun ScreenHeader(title: String, nav: NavController, action: (@Composable () -> Unit)? = null) {
    val b = LocalBrand.current
    Row(Modifier.fillMaxWidth().padding(bottom = 10.dp), verticalAlignment = Alignment.CenterVertically) {
        Text("‹", color = b.textPrimary, fontSize = 30.sp, modifier = Modifier.clickableNoRipple { nav.popBackStack() })
        Spacer(Modifier.width(12.dp))
        Text(title, color = b.textPrimary, fontSize = 22.sp, fontWeight = FontWeight.Bold)
        Spacer(Modifier.weight(1f))
        action?.invoke()
    }
}

@Composable
private fun RowScope.StatTile(title: String, value: Double?) {
    val b = LocalBrand.current
    Column(Modifier.weight(1f), horizontalAlignment = Alignment.CenterHorizontally) {
        Text(value?.let { fmt(it) } ?: "—",
            style = TextStyle(brush = brandGradient(), fontSize = 20.sp, fontWeight = FontWeight.Bold))
        Text(title, color = b.textSecondary, fontSize = 11.sp)
    }
}

// ---------------- Staking ----------------

@Composable
fun StakingScreen(vm: WalletViewModel, nav: NavController) {
    val b = LocalBrand.current
    val s = LocalStrings.current
    val scope = rememberCoroutineScope()
    val owner = vm.address ?: ""
    val staking = remember(vm.stakingUrl) { StakingService(vm.stakingUrl) }
    var config by remember { mutableStateOf<StakingConfig?>(null) }
    var position by remember { mutableStateOf<StakePosition?>(null) }
    var rewards by remember { mutableStateOf<NodeRewards?>(null) }
    var nodeStatus by remember { mutableStateOf<NodeStatus?>(null) }
    var stakeAmt by remember { mutableStateOf("") }
    var nodeId by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var msg by remember { mutableStateOf<String?>(null) }
    var lastSig by remember { mutableStateOf<String?>(null) }
    val uri = LocalUriHandler.current

    suspend fun reload() {
        config = withContext(Dispatchers.IO) { runCatching { staking.config() }.getOrNull() }
        // Genesis discovery: adopt the gateway's advertised public URL (unless the
        // user pinned a custom one), but only if it's actually reachable — a gateway
        // advertising a not-yet-live domain must not strand the client. Changing
        // vm.stakingUrl re-triggers this effect.
        val pub = config?.publicUrl
        if (!pub.isNullOrBlank() && pub != vm.stakingUrl && !vm.hasCustomStakingUrl) {
            val reachable = withContext(Dispatchers.IO) { runCatching { StakingService(pub).config() }.isSuccess }
            if (reachable) vm.adoptGenesisUrl(pub)
        }
        position = withContext(Dispatchers.IO) { runCatching { staking.position(owner) }.getOrNull() }
        rewards = withContext(Dispatchers.IO) { runCatching { staking.nodeRewards(owner) }.getOrNull() }
        nodeStatus = withContext(Dispatchers.IO) { runCatching { staking.nodeStatus(owner) }.getOrNull() }
        msg = if (config == null) s.t("staking.serverError") else null
    }
    LaunchedEffect(vm.stakingUrl) { reload() }

    BrandBackground {
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp)) {
            ScreenHeader(s.t("staking.title"), nav)

            Row(Modifier.brandCard(14.dp).clickableNoRipple { nav.navigate("guide") },
                verticalAlignment = Alignment.CenterVertically) {
                Text(s.t("staking.howTo"), color = b.textPrimary, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.weight(1f))
                Text("›", color = b.textSecondary, fontSize = 20.sp)
            }
            Spacer(Modifier.height(14.dp))

            // staking
            Column(Modifier.brandCard()) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(s.t("staking.stakeCard"), color = b.pink, fontSize = 16.sp, fontWeight = FontWeight.Bold)
                    Spacer(Modifier.weight(1f))
                    config?.let { Text("APR ${fmt(it.aprPercent)}%", color = b.textSecondary, fontSize = 14.sp, fontWeight = FontWeight.SemiBold) }
                }
                Spacer(Modifier.height(12.dp))
                Row { StatTile(s.t("staking.staked"), position?.principal); StatTile(s.t("staking.rewards"), position?.rewards) }
                Spacer(Modifier.height(12.dp))
                OutlinedTextField(stakeAmt, { stakeAmt = it }, Modifier.fillMaxWidth(),
                    label = { Text(s.t("staking.amountLabel")) }, singleLine = true)
                // Staking is reserved for online contributing nodes — the backend
                // rejects a stake from an owner with no online node, so gate here too.
                val hasOnlineNode = (nodeStatus?.totals?.online ?: 0) > 0
                if (!hasOnlineNode) {
                    Spacer(Modifier.height(8.dp))
                    Text("⚠ " + s.t("staking.needOnline"), color = Color(0xFFE0952B), fontSize = 13.sp)
                }
                Spacer(Modifier.height(10.dp))
                PrimaryButton(if (busy) s.t("staking.processing") else s.t("staking.stake"),
                    enabled = !busy && (stakeAmt.toDoubleOrNull() ?: 0.0) > 0 && hasOnlineNode) {
                    val amt = stakeAmt.toDouble(); val cfg = config ?: return@PrimaryButton
                    busy = true; msg = null; lastSig = null
                    scope.launch {
                        try {
                            val sig = vm.sendToken(cfg.vaultOwner, amt); lastSig = sig
                            withContext(Dispatchers.IO) { staking.stake(owner, amt, sig) }
                            stakeAmt = ""; reload(); vm.refresh()
                        } catch (e: Exception) { msg = e.message } finally { busy = false }
                    }
                }
                Spacer(Modifier.height(8.dp))
                SecondaryButton(s.t("staking.unstakeAll"), enabled = !busy && (position?.principal ?: 0.0) > 0) {
                    busy = true; msg = null
                    scope.launch {
                        try {
                            val r = withContext(Dispatchers.IO) { staking.unstake(owner, null) }
                            lastSig = r.signature; reload(); vm.refresh()
                        } catch (e: Exception) { msg = e.message } finally { busy = false }
                    }
                }
            }
            Spacer(Modifier.height(14.dp))

            // node rewards
            Column(Modifier.brandCard()) {
                Text(s.t("staking.nodeRewards"), color = b.blue, fontSize = 16.sp, fontWeight = FontWeight.Bold)
                Spacer(Modifier.height(12.dp))
                Row { StatTile(s.t("staking.claimable"), rewards?.pending) }
                rewards?.nodes?.takeIf { it.isNotEmpty() }?.let { rawNodes ->
                    val nodes = consolidateRewardNodes(rawNodes)
                    Spacer(Modifier.height(8.dp))
                    nodes.forEach {
                        Row(Modifier.fillMaxWidth().padding(vertical = 3.dp)) {
                            Text(it.nodeId, color = b.textPrimary, fontSize = 13.sp, fontFamily = FontFamily.Monospace)
                            Spacer(Modifier.weight(1f))
                            Text("${fmt(it.pendingRewards)} KVR", color = b.textSecondary, fontSize = 13.sp)
                        }
                    }
                }
                Spacer(Modifier.height(10.dp))
                OutlinedTextField(nodeId, { nodeId = it }, Modifier.fillMaxWidth(),
                    label = { Text(s.t("staking.nodeIdLabel")) }, singleLine = true)
                Spacer(Modifier.height(10.dp))
                PrimaryButton(s.t("staking.claim"), enabled = !busy && (rewards?.pending ?: 0.0) > 0) {
                    busy = true; msg = null
                    scope.launch {
                        try {
                            val r = withContext(Dispatchers.IO) { staking.claimNodeRewards(owner) }
                            lastSig = r.signature; reload(); vm.refresh()
                        } catch (e: Exception) { msg = e.message } finally { busy = false }
                    }
                }
                Spacer(Modifier.height(8.dp))
                SecondaryButton(s.t("staking.registerNode"), enabled = !busy && nodeId.isNotBlank()) {
                    val id = nodeId.trim(); busy = true; msg = null
                    scope.launch {
                        try {
                            withContext(Dispatchers.IO) { staking.registerNode(id, owner) }
                            nodeId = ""; reload()
                        } catch (e: Exception) { msg = e.message } finally { busy = false }
                    }
                }
                Spacer(Modifier.height(10.dp))
                Row(Modifier.fillMaxWidth().clickableNoRipple { nav.navigate("nodemonitor") }) {
                    Text(s.t("staking.viewStatus"), color = b.blue, fontSize = 14.sp, fontWeight = FontWeight.Medium)
                    Spacer(Modifier.weight(1f))
                    Text("›", color = b.blue, fontSize = 18.sp)
                }
            }

            lastSig?.let {
                Spacer(Modifier.height(12.dp))
                Text(s.t("staking.viewLastTx"), color = b.blue, fontSize = 13.sp,
                    modifier = Modifier.clickableNoRipple { uri.openUri(vm.explorerUrl(it)) })
            }
            msg?.let { Spacer(Modifier.height(8.dp)); Text(it, color = Color.Red, fontSize = 12.sp) }
            Spacer(Modifier.height(20.dp))
        }
    }
}

// ---------------- Node monitor ----------------

@Composable
fun NodeMonitorScreen(vm: WalletViewModel, nav: NavController) {
    val b = LocalBrand.current
    val str = LocalStrings.current
    val scope = rememberCoroutineScope()
    val owner = vm.address ?: ""
    val staking = remember(vm.stakingUrl) { StakingService(vm.stakingUrl) }
    var status by remember { mutableStateOf<NodeStatus?>(null) }
    var msg by remember { mutableStateOf<String?>(null) }

    suspend fun reload() {
        status = withContext(Dispatchers.IO) { runCatching { staking.nodeStatus(owner) }.getOrNull() }
        msg = if (status == null) str.t("monitor.serverError") else null
    }
    LaunchedEffect(vm.stakingUrl) { reload() }

    BrandBackground {
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp)) {
            ScreenHeader(str.t("monitor.title"), nav) {
                Text(str.t("monitor.connectDevice"), color = b.pink, fontSize = 14.sp, fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.clickableNoRipple { nav.navigate("deviceconnect") })
            }
            HubConnectionCard(vm, b)
            Spacer(Modifier.height(10.dp))
            status?.let { s ->
                Column(Modifier.brandCard()) {
                    Row { StatTile(str.t("monitor.nodes"), s.totals.nodes.toDouble()); StatTile(str.t("monitor.online"), s.totals.online.toDouble()) }
                    Spacer(Modifier.height(12.dp))
                    Row { StatTile(str.t("monitor.rawContribUnit"), s.totals.contributedUnits); StatTile(str.t("monitor.effContribWeighted"), s.totals.effectiveUnits) }
                    Spacer(Modifier.height(12.dp))
                    Row { StatTile(str.t("monitor.lifetimeRewards"), s.totals.lifetimeRewards); StatTile(str.t("monitor.claimableLkc"), s.totals.pending) }
                }
                Spacer(Modifier.height(10.dp))
                TierLegend(b)
                Spacer(Modifier.height(14.dp))
                if (s.nodes.isEmpty()) {
                    Column(Modifier.brandCard(28.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(str.t("monitor.noDevices"), color = b.textPrimary, fontSize = 16.sp, fontWeight = FontWeight.Bold)
                        Spacer(Modifier.height(4.dp))
                        Text(str.t("monitor.noDevicesHint"), color = b.textSecondary, fontSize = 13.sp)
                    }
                } else {
                    consolidateExpertNodes(s.nodes).forEach { node ->
                        NodeCard(
                            n = node,
                            b = b,
                            onOpenSettings = { nav.navigate("nodesettings") },
                            onRemove = {
                                scope.launch {
                                    try {
                                        withContext(Dispatchers.IO) { staking.removeNode(node.nodeId, owner) }
                                        reload()
                                    } catch (e: Exception) { msg = e.message }
                                }
                            },
                        )
                        Spacer(Modifier.height(12.dp))
                    }
                }
            }
            msg?.let { Text(it, color = Color.Red, fontSize = 12.sp) }
            Spacer(Modifier.height(20.dp))
        }
    }
}

// The settlement gateway upserts a hub-qualified node (`infer-<hubKey>-<nodeId>`)
// for a phone's expert-shard contribution, separate from the phone's own
// registration node (`<nodeId>`). Fold the reward of every such work-node into
// its base node so a phone's earnings show on its own card, not a mystery second
// one. Work-nodes with no matching base (datacenter agents) are left untouched.
private fun consolidateExpertNodes(nodes: List<NodeStatusItem>): List<NodeStatusItem> {
    val ids = nodes.mapTo(HashSet()) { it.nodeId }
    val merged = LinkedHashMap<String, NodeStatusItem>()
    for (n in nodes) if (expertBaseId(n.nodeId)?.let { ids.contains(it) } != true) merged[n.nodeId] = n
    for (n in nodes) {
        val base = expertBaseId(n.nodeId) ?: continue
        val t = merged[base] ?: continue
        merged[base] = t.copy(
            contributedUnits = t.contributedUnits + n.contributedUnits,
            effectiveUnits = t.effectiveUnits + n.effectiveUnits,
            pendingRewards = t.pendingRewards + n.pendingRewards,
            claimedTotal = t.claimedTotal + n.claimedTotal,
        )
    }
    return merged.values.toList()
}

// Same fold for the claimable-rewards list (NodeReward has fewer fields).
private fun consolidateRewardNodes(nodes: List<NodeReward>): List<NodeReward> {
    val ids = nodes.mapTo(HashSet()) { it.nodeId }
    val merged = LinkedHashMap<String, NodeReward>()
    for (n in nodes) if (expertBaseId(n.nodeId)?.let { ids.contains(it) } != true) merged[n.nodeId] = n
    for (n in nodes) {
        val base = expertBaseId(n.nodeId) ?: continue
        val t = merged[base] ?: continue
        merged[base] = t.copy(
            contributedUnits = t.contributedUnits + n.contributedUnits,
            pendingRewards = t.pendingRewards + n.pendingRewards,
        )
    }
    return merged.values.toList()
}

// `infer-<hubKey>-<baseId>` -> `<baseId>` (the hubKey segment has no '-').
private fun expertBaseId(id: String): String? {
    if (!id.startsWith("infer-")) return null
    val dash = id.indexOf('-', "infer-".length)
    return if (dash >= 0) id.substring(dash + 1) else null
}

// Hub connection status: which known hubs the node auto-connects to on launch
// and whether it is currently serving one. Shown at the top of the dashboard.
@Composable
private fun HubConnectionCard(vm: WalletViewModel, b: BrandColors) {
    val str = LocalStrings.current
    val green = Color(0xFF33C06B)
    val hubs = vm.nodeHubUrls
    val serving = vm.nodeServing
    val connected = vm.nodeConnected
    val dot = if (serving) green else if (connected) b.pink else b.textSecondary
    val state = when {
        serving -> str.t("monitor.hubServing")
        connected -> str.t("monitor.hubConnectedIdle")
        else -> str.t("monitor.hubDisconnected")
    }
    Column(Modifier.fillMaxWidth().brandCard(16.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(str.t("monitor.hubTitle"), color = b.textPrimary, fontSize = 15.sp, fontWeight = FontWeight.Bold)
            Spacer(Modifier.weight(1f))
            Box(Modifier.size(8.dp).clip(CircleShape).background(dot))
            Spacer(Modifier.width(6.dp))
            Text(state, color = b.textSecondary, fontSize = 12.sp, fontWeight = FontWeight.SemiBold)
        }
        if (hubs.isEmpty()) {
            Spacer(Modifier.height(6.dp))
            Text(str.t("monitor.hubNone"), color = b.textSecondary, fontSize = 12.sp)
        } else {
            hubs.forEach { url ->
                Spacer(Modifier.height(6.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.size(6.dp).clip(CircleShape).background(if (serving) green else b.pink))
                    Spacer(Modifier.width(6.dp))
                    Text(hubHost(url), color = b.textPrimary, fontSize = 13.sp, fontFamily = FontFamily.Monospace)
                }
            }
        }
    }
}

private fun hubHost(url: String): String = runCatching { java.net.URI(url).host ?: url }.getOrDefault(url)

@Composable
private fun NodeCard(n: NodeStatusItem, b: BrandColors, onOpenSettings: () -> Unit, onRemove: () -> Unit) {
    val s = LocalStrings.current
    var showConfirm by remember { mutableStateOf(false) }
    val (statusColor, statusLabel) = when (n.status) {
        "online" -> Color(0xFF33C06B) to s.t("status.online")
        "idle" -> Color(0xFFE0952B) to s.t("status.idle")
        "registered" -> b.blue to s.t("status.registered")
        else -> Color.Gray to s.t("status.offline")
    }
    val tier = n.tier ?: "—"
    val tierColor = tierColor(tier)

    if (showConfirm) {
        AlertDialog(
            onDismissRequest = { showConfirm = false },
            title = { Text(s.t("node.remove")) },
            text = { Text(s.t("node.removeConfirm")) },
            confirmButton = {
                TextButton(onClick = { showConfirm = false; onRemove() }) {
                    Text(s.t("node.remove"), color = Color.Red)
                }
            },
            dismissButton = {
                TextButton(onClick = { showConfirm = false }) { Text(s.t("common.close")) }
            },
        )
    }

    // whole card taps through to node settings; the trash button below handles its own tap
    Column(Modifier.brandCard().clickableNoRipple { onOpenSettings() }) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(osIcon(n.os), fontSize = 20.sp)
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                Text(n.label ?: n.nodeId, color = b.textPrimary, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                Text("${osLabel(n.os, s)} · ${(n.accelerator ?: "cpu").uppercase()}", color = b.textSecondary, fontSize = 12.sp)
            }
            Box(Modifier.clip(RoundedCornerShape(50)).background(statusColor.copy(alpha = 0.15f)).padding(horizontal = 10.dp, vertical = 4.dp)) {
                Text(statusLabel, color = statusColor, fontSize = 12.sp, fontWeight = FontWeight.SemiBold)
            }
            Spacer(Modifier.width(8.dp))
            // dedicated remove (trash) button — separate click, does not trigger the row navigation
            Text("🗑", fontSize = 18.sp,
                modifier = Modifier.clickableNoRipple { showConfirm = true }.padding(4.dp))
            Spacer(Modifier.width(4.dp))
            Text("›", color = b.textSecondary, fontSize = 20.sp)
        }

        Spacer(Modifier.height(12.dp))
        // performance re-scoring panel
        Row(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp))
                .background(tierColor.copy(alpha = 0.10f)).padding(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                Modifier.size(36.dp).clip(RoundedCornerShape(50)).background(tierColor),
                contentAlignment = Alignment.Center,
            ) { Text(tier, color = Color.White, fontSize = 16.sp, fontWeight = FontWeight.ExtraBold) }
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text("${s.t("monitor.perfTier")} $tier · ×${fmt(n.perfMultiplier)}", color = tierColor, fontSize = 13.sp, fontWeight = FontWeight.Bold)
                val be = n.backend?.uppercase()?.let { "$it · " } ?: ""
                Text("$be${n.perfScore.toInt()} tok/s${n.mode?.let { " · ${modeLabel(it, s)}" } ?: ""}",
                    color = b.textSecondary, fontSize = 11.sp)
            }
        }
        Spacer(Modifier.height(8.dp))
        // contribution: raw -> effective
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text("${s.t("monitor.raw")} ${fmt(n.contributedUnits)}", color = b.textSecondary, fontSize = 12.sp)
            Text("  ×${fmt(n.perfMultiplier)}  ", color = tierColor, fontSize = 12.sp, fontWeight = FontWeight.Bold)
            Text("→ ${s.t("monitor.effective")} ${fmt(n.effectiveUnits)}", color = b.textPrimary, fontSize = 12.sp, fontWeight = FontWeight.SemiBold)
        }

        Spacer(Modifier.height(10.dp))
        Row {
            StatTile(s.t("monitor.effContrib"), n.effectiveUnits)
            StatTile(s.t("monitor.claimableShort"), n.pendingRewards)
            StatTile(s.t("monitor.claimed"), n.claimedTotal)
        }
    }
}

private fun tierColor(tier: String): Color = when (tier) {
    "S" -> Color(0xFFE0952B) // gold
    "A" -> Color(0xFF33C06B) // green
    "B" -> Color(0xFF4C8DFF) // blue
    else -> Color(0xFF9AA0A6) // gray (C / unknown)
}
private fun modeLabel(mode: String, s: Strings): String = when (mode) {
    "local_shard" -> s.t("mode.localShard"); "rpc_worker" -> s.t("mode.rpcWorker"); else -> mode
}

@Composable
private fun TierLegend(b: BrandColors) {
    val s = LocalStrings.current
    Column(Modifier.brandCard()) {
        Text(s.t("legend.title"), color = b.textPrimary, fontSize = 14.sp, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(2.dp))
        Text(s.t("legend.desc"),
            color = b.textSecondary, fontSize = 11.sp)
        Spacer(Modifier.height(10.dp))
        TierRow("S", "≥ 90 tok/s", "×1.5", b)
        TierRow("A", "60–89 tok/s", "×1.25", b)
        TierRow("B", "30–59 tok/s", "×1.0", b)
        TierRow("C", "< 30 tok/s", "×0.7", b)
    }
}

@Composable
private fun TierRow(tier: String, range: String, mult: String, b: BrandColors) {
    val c = tierColor(tier)
    Row(Modifier.fillMaxWidth().padding(vertical = 3.dp), verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(24.dp).clip(RoundedCornerShape(50)).background(c), contentAlignment = Alignment.Center) {
            Text(tier, color = Color.White, fontSize = 12.sp, fontWeight = FontWeight.Bold)
        }
        Spacer(Modifier.width(10.dp))
        Text(range, color = b.textSecondary, fontSize = 12.sp, modifier = Modifier.weight(1f))
        Text(mult, color = c, fontSize = 13.sp, fontWeight = FontWeight.Bold, fontFamily = FontFamily.Monospace)
    }
}

private fun osIcon(os: String?): String = when (os?.lowercase()) {
    "macos" -> "🖥"; "ios" -> "📱"; "android" -> "🤖"; "windows" -> "🪟"; "linux" -> "🐧"; else -> "💻"
}
private fun osLabel(os: String?, s: Strings): String = when (os?.lowercase()) {
    "macos" -> "macOS"; "ios" -> "iOS"; "android" -> "Android"; "windows" -> "Windows"; "linux" -> "Linux"; else -> s.t("os.unknown")
}

// ---------------- Device connect ----------------

@Composable
fun DeviceConnectScreen(vm: WalletViewModel, nav: NavController) {
    val b = LocalBrand.current
    val s = LocalStrings.current
    val ctx = LocalContext.current
    val clip = LocalClipboardManager.current
    val scope = rememberCoroutineScope()
    val owner = vm.address ?: ""
    val staking = remember(vm.stakingUrl) { StakingService(vm.stakingUrl) }
    val androidId = remember { Settings.Secure.getString(ctx.contentResolver, Settings.Secure.ANDROID_ID) ?: "device" }
    val nodeId = "android-${androidId.take(8)}"
    val command = "LINKCPP_SERVICE=${vm.stakingUrl} LINKCPP_OWNER=$owner node connect.js"
    var urlText by remember { mutableStateOf(vm.stakingUrl) }
    var busy by remember { mutableStateOf(false) }
    var msg by remember { mutableStateOf<String?>(null) }
    var connected by remember { mutableStateOf(false) }
    // Genesis gateway info for the settings display (reachability + facts).
    var genesisConfig by remember { mutableStateOf<StakingConfig?>(null) }
    LaunchedEffect(vm.stakingUrl) {
        genesisConfig = withContext(Dispatchers.IO) { runCatching { staking.config() }.getOrNull() }
    }

    BrandBackground {
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp)) {
            ScreenHeader(s.t("connect.title"), nav)

            // account
            Column(Modifier.brandCard(), horizontalAlignment = Alignment.CenterHorizontally) {
                Text(s.t("connect.myAccount"), color = b.pink, fontSize = 16.sp, fontWeight = FontWeight.Bold, modifier = Modifier.fillMaxWidth())
                Spacer(Modifier.height(4.dp))
                Text(s.t("connect.myAccountDesc"), color = b.textSecondary, fontSize = 12.sp, modifier = Modifier.fillMaxWidth())
                Spacer(Modifier.height(12.dp))
                if (owner.isNotEmpty()) {
                    Image(Qr.bitmap(owner), null, Modifier.size(180.dp).background(Color.White, RoundedCornerShape(14.dp)).padding(12.dp))
                    Spacer(Modifier.height(10.dp))
                    SelectionContainer { Text(owner, color = b.textPrimary, fontSize = 12.sp, fontFamily = FontFamily.Monospace) }
                    Spacer(Modifier.height(8.dp))
                    SecondaryButton(s.t("common.copyAddress")) { clip.setText(AnnotatedString(owner)) }
                }
            }
            Spacer(Modifier.height(14.dp))

            // this device
            Column(Modifier.brandCard()) {
                Text(s.t("connect.thisDevice"), color = b.blue, fontSize = 16.sp, fontWeight = FontWeight.Bold)
                Spacer(Modifier.height(6.dp))
                Text(s.t("connect.thisDeviceDesc").format(Build.MODEL), color = b.textSecondary, fontSize = 13.sp)
                Spacer(Modifier.height(10.dp))
                PrimaryButton(if (busy) s.t("connect.connecting") else s.t("connect.connectThis"), enabled = !busy) {
                    busy = true; msg = null
                    scope.launch {
                        try {
                            withContext(Dispatchers.IO) {
                                staking.registerNode(nodeId, owner, os = "android", deviceKind = "phone", accelerator = "npu", label = Build.MODEL)
                                staking.heartbeat(nodeId)
                            }
                            connected = true; msg = s.t("connect.connected").format(nodeId)
                        } catch (e: Exception) { connected = false; msg = e.message } finally { busy = false }
                    }
                }
            }
            Spacer(Modifier.height(14.dp))

            // node settings entry
            Row(Modifier.brandCard().clickableNoRipple { nav.navigate("nodesettings") },
                verticalAlignment = Alignment.CenterVertically) {
                Text(s.t("connect.nodeSettings"), color = b.textPrimary, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.weight(1f))
                Text(s.t("connect.nodeSettingsSub"), color = b.textSecondary, fontSize = 12.sp)
            }
            Spacer(Modifier.height(14.dp))

            // other device
            Column(Modifier.brandCard()) {
                Text(s.t("connect.otherDevice"), color = b.blue, fontSize = 16.sp, fontWeight = FontWeight.Bold)
                Spacer(Modifier.height(6.dp))
                Text(s.t("connect.otherDeviceDesc"), color = b.textSecondary, fontSize = 13.sp)
                Spacer(Modifier.height(8.dp))
                SelectionContainer {
                    Text(command, color = b.textPrimary, fontSize = 11.sp, fontFamily = FontFamily.Monospace,
                        modifier = Modifier.fillMaxWidth().background(b.pink.copy(alpha = 0.10f), RoundedCornerShape(10.dp)).padding(10.dp))
                }
                Spacer(Modifier.height(8.dp))
                SecondaryButton(s.t("connect.copyCommand")) { clip.setText(AnnotatedString(command)) }
            }
            Spacer(Modifier.height(14.dp))

            // settings
            Column(Modifier.brandCard()) {
                Text(s.t("connect.serverUrl"), color = b.textPrimary, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.height(6.dp))
                Text(s.t("connect.serverUrlDesc"), color = b.textSecondary, fontSize = 12.sp)
                Spacer(Modifier.height(8.dp))
                OutlinedTextField(urlText, { urlText = it }, Modifier.fillMaxWidth(),
                    singleLine = true, textStyle = TextStyle(fontFamily = FontFamily.Monospace, fontSize = 13.sp))
                Spacer(Modifier.height(8.dp))
                PrimaryButton(s.t("common.save")) { vm.updateStakingUrl(urlText) }
            }
            Spacer(Modifier.height(14.dp))

            // genesis gateway info
            GenesisInfoCard(genesisConfig, vm.stakingUrl)
            msg?.let { Spacer(Modifier.height(10.dp)); Text(it, color = if (connected) b.pink else Color.Red, fontSize = 13.sp) }
            Spacer(Modifier.height(20.dp))
        }
    }
}

// ---------------- Genesis gateway info ----------------

/** Info on the network coordination node (the active settlement/inference gateway),
 *  read from its /api/config. Reachability reflects whether that config loaded. */
@Composable
private fun GenesisInfoCard(config: StakingConfig?, activeUrl: String) {
    val b = LocalBrand.current
    val s = LocalStrings.current
    Column(Modifier.brandCard()) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(s.t("genesis.title"), color = b.textPrimary, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.weight(1f))
            Box(Modifier.size(8.dp).background(if (config == null) Color.Red else Color(0xFF34C759), CircleShape))
            Spacer(Modifier.width(5.dp))
            Text(if (config == null) s.t("genesis.unreachable") else s.t("genesis.reachable"),
                color = b.textSecondary, fontSize = 12.sp)
        }
        Spacer(Modifier.height(6.dp))
        Text(s.t("genesis.desc"), color = b.textSecondary, fontSize = 12.sp)
        Spacer(Modifier.height(10.dp))
        Text(s.t("genesis.active"), color = b.textSecondary, fontSize = 11.sp)
        SelectionContainer {
            Text(activeUrl.ifEmpty { "—" }, color = b.textPrimary, fontSize = 12.sp, fontFamily = FontFamily.Monospace)
        }
        val pub = config?.publicUrl
        if (!pub.isNullOrEmpty() && pub != activeUrl) {
            Spacer(Modifier.height(8.dp))
            Text(s.t("genesis.advertised"), color = b.textSecondary, fontSize = 11.sp)
            SelectionContainer {
                Text(pub, color = b.pink, fontSize = 12.sp, fontFamily = FontFamily.Monospace)
            }
        }
        config?.let { cfg ->
            Spacer(Modifier.height(10.dp))
            GenesisFact(s.t("genesis.cluster"), cfg.cluster, b)
            GenesisFact("APR", "${fmt(cfg.aprPercent)}%", b)
            GenesisFact(s.t("genesis.rewardPerUnit"), "${fmt(cfg.rewardPerUnit)} ${cfg.symbol}/unit", b)
            cfg.gatewayBonus?.let { if (it > 1) GenesisFact(s.t("genesis.gatewayBonus"), "+${((it - 1) * 100).toInt()}%", b) }
        }
    }
}

@Composable
private fun GenesisFact(label: String, value: String, b: BrandColors) {
    Row(Modifier.fillMaxWidth().padding(vertical = 3.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = b.textSecondary, fontSize = 12.sp)
        Spacer(Modifier.weight(1f))
        Text(value, color = b.textPrimary, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, fontFamily = FontFamily.Monospace)
    }
}

// ---------------- Guide ----------------

@Composable
fun StakingGuideScreen(nav: NavController) {
    val b = LocalBrand.current
    val s = LocalStrings.current
    BrandBackground {
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp)) {
            ScreenHeader(s.t("guide.title"), nav)
            GuideSection(s.t("guide.whatTitle"), listOf(
                s.t("guide.what1"),
                s.t("guide.what2"),
            ), b)
            GuideSection(s.t("guide.howTitle"), listOf(
                s.t("guide.how1"),
                s.t("guide.how2"),
                s.t("guide.how3"),
                s.t("guide.how4"),
                s.t("guide.how5"),
            ), b)
            GuideSection(s.t("guide.nodeTitle"), listOf(
                s.t("guide.node1"),
                s.t("guide.node2"),
                s.t("guide.node3"),
                s.t("guide.node4"),
            ), b)
            GuideSection(s.t("guide.notesTitle"), listOf(
                s.t("guide.note1"),
                s.t("guide.note2"),
                s.t("guide.note3"),
            ), b)
            Spacer(Modifier.height(20.dp))
        }
    }
}

@Composable
private fun GuideSection(title: String, lines: List<String>, b: BrandColors) {
    Column(Modifier.brandCard()) {
        Text(title, color = b.pink, fontSize = 16.sp, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(8.dp))
        lines.forEach {
            Text(it, color = b.textPrimary, fontSize = 14.sp, modifier = Modifier.padding(vertical = 3.dp))
        }
    }
    Spacer(Modifier.height(12.dp))
}
