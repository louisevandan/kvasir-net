package ai.banya.linkcpp.wallet

import android.os.Build
import android.provider.Settings
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import kotlinx.coroutines.launch
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.navigation.NavController
import ai.banya.linkcpp.core.StakingService
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext

private val AMBER = Color(0xFFE0952B)
private val GREEN = Color(0xFF33C06B)

@Composable
fun NodeSettingsScreen(vm: WalletViewModel, nav: NavController) {
    val b = LocalBrand.current
    val s = LocalStrings.current
    val ctx = LocalContext.current
    val profile = nodeProfile(vm.nodeBackend, vm.nodeMode, s)
    var stats by remember { mutableStateOf(DeviceStats()) }

    LaunchedEffect(vm.nodeLive, vm.stakingUrl) {
        if (!vm.nodeLive) return@LaunchedEffect
        val owner = vm.address ?: return@LaunchedEffect
        val svc = StakingService(vm.stakingUrl)
        val accel = if (vm.nodeBackend == "cpu") "cpu" else "gpu"
        val nodeId = "android-${(Settings.Secure.getString(ctx.contentResolver, Settings.Secure.ANDROID_ID) ?: "dev").take(8)}"
        NodeTelemetry.read(ctx) // prime CPU delta
        runCatching {
            withContext(Dispatchers.IO) {
                svc.registerNode(nodeId, owner, os = "android", deviceKind = "phone",
                    accelerator = accel, label = "${Build.MODEL} · ${vm.nodeMode}",
                    perfScore = profile.tokPerSec.toDouble(), backend = vm.nodeBackend, mode = vm.nodeMode)
            }
        }
        while (vm.nodeLive) {
            stats = NodeTelemetry.read(ctx)
            runCatching { withContext(Dispatchers.IO) { svc.heartbeat(nodeId) } }
            delay(1500)
        }
    }

    BrandBackground {
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp)) {
            ScreenHeader(s.t("ns.title"), nav)

            // backend
            Column(Modifier.brandCard()) {
                Text(s.t("ns.backend"), color = b.textPrimary, fontSize = 16.sp, fontWeight = FontWeight.Bold)
                Spacer(Modifier.height(4.dp))
                Text(s.t("ns.backendDesc"), color = b.textSecondary, fontSize = 12.sp)
                Spacer(Modifier.height(10.dp))
                Row {
                    ChoiceChip("GPU · OpenCL", vm.nodeBackend == "opencl") { vm.updateNodeBackend("opencl") }
                    Spacer(Modifier.width(8.dp))
                    ChoiceChip("GPU · Vulkan", vm.nodeBackend == "vulkan") { vm.updateNodeBackend("vulkan") }
                    Spacer(Modifier.width(8.dp))
                    ChoiceChip("CPU", vm.nodeBackend == "cpu") { vm.updateNodeBackend("cpu") }
                }
            }
            Spacer(Modifier.height(12.dp))

            // mode
            Column(Modifier.brandCard()) {
                Text(s.t("ns.mode"), color = b.textPrimary, fontSize = 16.sp, fontWeight = FontWeight.Bold)
                Spacer(Modifier.height(10.dp))
                ModeRow(s.t("ns.localShard"), s.t("ns.localShardDesc"),
                    vm.nodeMode == "local_shard") { vm.updateNodeMode("local_shard") }
                Spacer(Modifier.height(8.dp))
                ModeRow(s.t("ns.rpcWorker"), s.t("ns.rpcWorkerDesc"),
                    vm.nodeMode == "rpc_worker") { vm.updateNodeMode("rpc_worker") }
                if (vm.nodeLive && vm.nodeMode == "rpc_worker") {
                    Spacer(Modifier.height(8.dp))
                    Text(
                        if (vm.nodeAgentRunning) "RPC worker listening · agent :${vm.nodeAgentPort}"
                        else vm.nodeAgentEvent.ifEmpty { "starting…" },
                        color = if (vm.nodeAgentRunning) GREEN else b.textSecondary,
                        fontSize = 11.sp, fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                    )
                }
            }
            Spacer(Modifier.height(12.dp))

            // infographic
            Column(Modifier.brandCard()) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(s.t("ns.resImpact"), color = b.textPrimary, fontSize = 16.sp, fontWeight = FontWeight.Bold)
                    Spacer(Modifier.weight(1f))
                    Column(horizontalAlignment = Alignment.End) {
                        Text("${profile.tokPerSec}", style = androidx.compose.ui.text.TextStyle(brush = brandGradient(), fontSize = 24.sp, fontWeight = FontWeight.ExtraBold))
                        Text("tok/s (0.5B Q8)", color = b.textSecondary, fontSize = 10.sp)
                    }
                }
                Text(profile.computeUnit, color = b.blue, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.height(10.dp))
                MetricBar(s.t("ns.memImpact"), profile.memImpact, "${(profile.memImpact * 100).toInt()}%", b.blue)
                MetricBar(s.t("ns.thermal"), profile.thermal, thermalWord(profile.thermal, s), AMBER)
                MetricBar(s.t("ns.performance"), profile.performance, "${(profile.performance * 100).toInt()}%", GREEN)
                Spacer(Modifier.height(6.dp))
                Text("※ ${profile.note}", color = b.textSecondary, fontSize = 11.sp)
            }
            Spacer(Modifier.height(12.dp))

            // policy + live
            Column(Modifier.brandCard()) {
                ToggleRow(s.t("ns.chargingOnly"), vm.nodeChargingOnly) { vm.updateNodeChargingOnly(it) }
                Spacer(Modifier.height(8.dp))
                ToggleRow(s.t("ns.runLive"), vm.nodeLive) { vm.updateNodeLive(it) }
            }

            Spacer(Modifier.height(12.dp))
            HubConnectCard(vm)

            if (vm.nodeLive) {
                Spacer(Modifier.height(12.dp))
                Column(Modifier.brandCard()) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.size(9.dp).clip(RoundedCornerShape(50)).background(GREEN))
                        Spacer(Modifier.width(8.dp))
                        Text(s.t("ns.liveGauges"), color = b.textPrimary, fontSize = 16.sp, fontWeight = FontWeight.Bold)
                        Spacer(Modifier.weight(1f))
                        Text(if (stats.charging) s.t("ns.charging") else s.t("ns.onBattery"), color = b.textSecondary, fontSize = 12.sp)
                    }
                    Spacer(Modifier.height(12.dp))
                    val ramFrac = if (stats.ramTotalMb > 0) stats.ramUsedMb.toFloat() / stats.ramTotalMb else 0f
                    Gauge("RAM", ramFrac, "%.1f / %.1f GB".format(stats.ramUsedMb / 1024f, stats.ramTotalMb / 1024f), b.blue, true)
                    Gauge(s.t("ns.cpuLoad"), stats.cpuLoad, "${(stats.cpuLoad * 100).toInt()}%", GREEN, true)
                    val tempFrac = ((stats.batteryTempC - 20f) / 30f).coerceIn(0f, 1f)
                    Gauge(s.t("ns.temp"), tempFrac, "%.1f°C".format(stats.batteryTempC), AMBER, true)
                    Gauge("GPU (${vm.nodeBackend})", profile.performance, s.t("ns.estimated"), b.pink, false)
                    Gauge("NPU", 0f, s.t("ns.unused"), b.textSecondary, false)
                    Spacer(Modifier.height(4.dp))
                    Text(s.t("ns.gaugeNote"), color = b.textSecondary, fontSize = 10.sp)
                }
            }
            Spacer(Modifier.height(12.dp))
            ApiKeyCard(vm)
            Spacer(Modifier.height(24.dp))
        }
    }
}

/**
 * Reissue the credit API key.
 *
 * The key is stored on this device and the gateway keeps only its hash, so a key
 * it no longer recognises cannot be repaired by asking for it back — it has to
 * be reminted. Without this, "invalid API key" has no way out from inside the
 * app. Credit balance is held against the wallet, not the key.
 */
@Composable
private fun ApiKeyCard(vm: WalletViewModel) {
    val b = LocalBrand.current
    val s = Strings(vm.language)
    val scope = rememberCoroutineScope()
    var busy by remember { mutableStateOf(false) }
    var status by remember { mutableStateOf("") }

    Column(Modifier.brandCard()) {
        Text(s.t("ns.apiKey"), color = b.textPrimary, fontSize = 16.sp, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(4.dp))
        Text(s.t("ns.apiKeyDesc"), color = b.textSecondary, fontSize = 12.sp)
        Spacer(Modifier.height(10.dp))
        Box(
            Modifier.fillMaxWidth()
                .clip(RoundedCornerShape(10.dp))
                .background(if (busy) b.pink.copy(alpha = 0.4f) else b.pink)
                .clickableNoRipple {
                    if (busy) return@clickableNoRipple
                    busy = true; status = s.t("ns.apiKeyWorking")
                    scope.launch {
                        val r = runCatching { vm.reissueCreditApiKey() }
                        status = r.fold({ s.t("ns.apiKeyOk") }, { "${s.t("ns.apiKeyFailed")}: ${it.message}" })
                        busy = false
                    }
                }
                .padding(vertical = 12.dp), contentAlignment = Alignment.Center,
        ) { Text(if (busy) s.t("ns.apiKeyWorking") else s.t("ns.apiKeyReissue"), color = Color.White, fontWeight = FontWeight.SemiBold, fontSize = 13.sp) }

        if (status.isNotEmpty()) {
            Spacer(Modifier.height(8.dp))
            Text(status, color = if (status == s.t("ns.apiKeyOk")) GREEN else b.textSecondary, fontSize = 11.sp)
        }
    }
}

private fun thermalWord(v: Float, s: Strings): String = when {
    v >= 0.75f -> s.t("thermal.high"); v >= 0.5f -> s.t("thermal.medium"); else -> s.t("thermal.low")
}

/**
 * Connect this node to a remote (auth-gated) hub: sign in with the wallet
 * (SIWS) + OTP, then hand the resulting bearer token to the running node agent
 * so it polls that hub's shard-demand market. The LAN hub is discovered
 * automatically; this is for public hubs the node can only reach outbound.
 */
@Composable
private fun HubConnectCard(vm: WalletViewModel) {
    val b = LocalBrand.current
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    var url by remember { mutableStateOf("https://hub.kvasir-ai.net") }
    var busy by remember { mutableStateOf(false) }
    var status by remember { mutableStateOf("") }
    val hubs = remember { mutableStateOf(NodeService.agent?.knownHubList() ?: emptyList()) }

    Column(Modifier.brandCard()) {
        Text("허브 연결", color = b.textPrimary, fontSize = 16.sp, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(4.dp))
        Text("원격 허브에 지갑 서명으로 노드 토큰을 발급받아 폴링 등록 (OTP 불필요)", color = b.textSecondary, fontSize = 12.sp)
        Spacer(Modifier.height(10.dp))
        androidx.compose.material3.OutlinedTextField(
            value = url, onValueChange = { url = it }, singleLine = true,
            label = { Text("허브 URL", fontSize = 12.sp) },
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(10.dp))
        Box(
            Modifier.fillMaxWidth()
                .clip(RoundedCornerShape(10.dp))
                .background(if (busy) b.pink.copy(alpha = 0.4f) else b.pink)
                .clickableNoRipple {
                    if (busy) return@clickableNoRipple
                    val words = vm.revealMnemonic()
                    if (words == null) { status = "지갑 잠금 해제 필요"; return@clickableNoRipple }
                    if (NodeService.agent == null) { status = "노드를 먼저 켜세요 (라이브)"; return@clickableNoRipple }
                    busy = true; status = "지갑 서명 중…"
                    scope.launch {
                        val r = withContext(Dispatchers.IO) { runCatching { HubAuthService(url, words).nodeToken() } }
                        r.onSuccess {
                            NodeService.agent?.registerHub(url, it)
                            hubs.value = NodeService.agent?.knownHubList() ?: emptyList()
                            status = "연결됨 · 노드 토큰 등록"
                        }.onFailure { status = "실패: ${it.message}" }
                        busy = false
                    }
                }
                .padding(vertical = 12.dp), contentAlignment = Alignment.Center,
        ) { Text(if (busy) "연결 중…" else "지갑으로 연결", color = Color.White, fontWeight = FontWeight.SemiBold, fontSize = 13.sp) }

        if (status.isNotEmpty()) {
            Spacer(Modifier.height(8.dp))
            Text(status, color = if (status.startsWith("연결됨")) GREEN else b.textSecondary,
                fontSize = 11.sp, fontFamily = FontFamily.Monospace)
        }
        if (hubs.value.isNotEmpty()) {
            Spacer(Modifier.height(8.dp))
            hubs.value.forEach { Text("• $it", color = b.textSecondary, fontSize = 11.sp, fontFamily = FontFamily.Monospace) }
        }
    }
}

@Composable
private fun ChoiceChip(label: String, selected: Boolean, onClick: () -> Unit) {
    val b = LocalBrand.current
    Box(
        Modifier.clip(RoundedCornerShape(10.dp))
            .background(if (selected) b.pink else b.pink.copy(alpha = 0.12f))
            .clickableNoRipple { onClick() }.padding(horizontal = 12.dp, vertical = 8.dp),
    ) {
        Text(label, color = if (selected) Color.White else b.pink, fontWeight = FontWeight.SemiBold, fontSize = 12.sp)
    }
}

@Composable
private fun ModeRow(title: String, desc: String, selected: Boolean, onClick: () -> Unit) {
    val b = LocalBrand.current
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp))
            .background(if (selected) b.pink.copy(alpha = 0.10f) else Color.Transparent)
            .clickableNoRipple { onClick() }.padding(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(20.dp).clip(RoundedCornerShape(50))
            .background(if (selected) b.pink else b.stroke), contentAlignment = Alignment.Center) {
            if (selected) Text("✓", color = Color.White, fontSize = 12.sp)
        }
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(title, color = b.textPrimary, fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
            Text(desc, color = b.textSecondary, fontSize = 11.sp)
        }
    }
}

@Composable
private fun ToggleRow(label: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    val b = LocalBrand.current
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = b.textPrimary, fontSize = 14.sp, modifier = Modifier.weight(1f))
        Switch(checked = checked, onCheckedChange = onChange,
            colors = SwitchDefaults.colors(checkedTrackColor = b.pink))
    }
}

@Composable
private fun MetricBar(label: String, value: Float, valueText: String, color: Color) {
    val b = LocalBrand.current
    Column(Modifier.padding(vertical = 5.dp)) {
        Row {
            Text(label, color = b.textSecondary, fontSize = 12.sp)
            Spacer(Modifier.weight(1f))
            Text(valueText, color = b.textPrimary, fontSize = 12.sp, fontWeight = FontWeight.SemiBold)
        }
        Spacer(Modifier.height(4.dp))
        Box(Modifier.fillMaxWidth().height(8.dp).clip(RoundedCornerShape(4.dp)).background(b.stroke)) {
            Box(Modifier.fillMaxWidth(value.coerceIn(0f, 1f)).fillMaxHeight()
                .clip(RoundedCornerShape(4.dp)).background(color))
        }
    }
}

@Composable
private fun Gauge(label: String, value: Float, valueText: String, color: Color, live: Boolean) {
    val b = LocalBrand.current
    val s = LocalStrings.current
    Column(Modifier.padding(vertical = 6.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(label, color = b.textPrimary, fontSize = 13.sp, fontWeight = FontWeight.Medium)
            if (!live) {
                Spacer(Modifier.width(6.dp))
                Text(s.t("ns.estimated"), color = b.textSecondary, fontSize = 9.sp,
                    modifier = Modifier.background(b.stroke, RoundedCornerShape(4.dp)).padding(horizontal = 4.dp, vertical = 1.dp))
            }
            Spacer(Modifier.weight(1f))
            Text(valueText, color = color, fontSize = 13.sp, fontWeight = FontWeight.Bold, fontFamily = FontFamily.Monospace)
        }
        Spacer(Modifier.height(5.dp))
        Box(Modifier.fillMaxWidth().height(10.dp).clip(RoundedCornerShape(5.dp)).background(b.stroke)) {
            Box(Modifier.fillMaxWidth(value.coerceIn(0f, 1f)).fillMaxHeight()
                .clip(RoundedCornerShape(5.dp)).background(color))
        }
    }
}
