package ai.banya.linkcpp.wallet

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Language
import androidx.compose.material.icons.filled.Public
import androidx.compose.material.icons.filled.Key
import androidx.compose.material.icons.filled.Logout
import androidx.compose.material.icons.filled.Check
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
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
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.navigation.NavController
import ai.banya.linkcpp.core.TxRef
import kotlinx.coroutines.launch
import java.math.BigDecimal
import java.math.RoundingMode

fun fmt(v: Double): String =
    BigDecimal(v).setScale(6, RoundingMode.HALF_UP).stripTrailingZeros().toPlainString()

fun shorten(s: String, head: Int = 6, tail: Int = 6): String =
    if (s.length > head + tail + 1) "${s.take(head)}…${s.takeLast(tail)}" else s

// ---------------- Onboarding ----------------

@Composable
fun WelcomeScreen(nav: NavController) {
    val b = LocalBrand.current
    val s = LocalStrings.current
    BrandBackground {
        Column(
            Modifier.fillMaxSize().padding(28.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Spacer(Modifier.weight(1f))
            Image(
                painterResource(R.mipmap.ic_launcher), null,
                Modifier.size(120.dp).clip(RoundedCornerShape(28.dp)),
            )
            Spacer(Modifier.height(16.dp))
            Title("Kvasir Wallet")
            Spacer(Modifier.height(6.dp))
            Text(s.t("welcome.subtitle"), color = b.textSecondary, fontSize = 14.sp)
            Spacer(Modifier.weight(1f))
            PrimaryButton(s.t("welcome.create")) { nav.navigate("create") }
            Spacer(Modifier.height(12.dp))
            SecondaryButton(s.t("welcome.restore")) { nav.navigate("import") }
        }
    }
}

@Composable
fun CreateScreen(vm: WalletViewModel, nav: NavController) {
    val b = LocalBrand.current
    val s = LocalStrings.current
    val words = remember { vm.newMnemonic() }
    var saved by remember { mutableStateOf(false) }
    var err by remember { mutableStateOf<String?>(null) }
    BrandBackground {
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp)) {
            Text(s.t("create.title"), color = b.textPrimary, fontSize = 20.sp, fontWeight = FontWeight.Bold)
            Spacer(Modifier.height(6.dp))
            Text(s.t("create.desc"),
                color = b.textSecondary, fontSize = 13.sp)
            Spacer(Modifier.height(16.dp))
            Column(Modifier.brandCard(16.dp)) {
                words.chunked(3).forEachIndexed { r, row ->
                    Row(Modifier.fillMaxWidth().padding(vertical = 5.dp)) {
                        row.forEachIndexed { i, w ->
                            Box(Modifier.weight(1f).padding(horizontal = 4.dp)) {
                                Row {
                                    Text("${r * 3 + i + 1} ", color = b.pink, fontSize = 12.sp, fontWeight = FontWeight.Bold)
                                    Text(w, color = b.textPrimary, fontSize = 14.sp)
                                }
                            }
                        }
                    }
                }
            }
            Spacer(Modifier.height(16.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Checkbox(saved, { saved = it })
                Text(s.t("create.saved"), color = b.textPrimary, fontSize = 14.sp)
            }
            err?.let { Spacer(Modifier.height(8.dp)); Text(it, color = Color.Red, fontSize = 13.sp) }
            Spacer(Modifier.height(16.dp))
            PrimaryButton(s.t("create.start"), enabled = saved) {
                vm.saveAndActivate(words, onError = { err = it }) {
                    nav.navigate("home") { popUpTo("welcome") { inclusive = true } }
                }
            }
        }
    }
}

@Composable
fun ImportScreen(vm: WalletViewModel, nav: NavController) {
    val b = LocalBrand.current
    val s = LocalStrings.current
    var text by remember { mutableStateOf("") }
    var err by remember { mutableStateOf<String?>(null) }
    BrandBackground {
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp)) {
            Text(s.t("import.title"), color = b.textPrimary, fontSize = 20.sp, fontWeight = FontWeight.Bold)
            Spacer(Modifier.height(6.dp))
            Text(s.t("import.desc"), color = b.textSecondary, fontSize = 13.sp)
            Spacer(Modifier.height(16.dp))
            OutlinedTextField(
                value = text, onValueChange = { text = it },
                modifier = Modifier.fillMaxWidth().height(140.dp),
                textStyle = TextStyle(fontFamily = FontFamily.Monospace),
            )
            err?.let { Spacer(Modifier.height(8.dp)); Text(it, color = Color.Red, fontSize = 13.sp) }
            Spacer(Modifier.height(16.dp))
            PrimaryButton(s.t("import.action"), enabled = text.isNotBlank()) {
                val words = text.trim().split(Regex("\\s+")).filter { it.isNotEmpty() }
                vm.saveAndActivate(words, onError = { err = it }) {
                    nav.navigate("home") { popUpTo("welcome") { inclusive = true } }
                }
            }
        }
    }
}

// ---------------- Home ----------------

@Composable
fun HomeScreen(vm: WalletViewModel, nav: NavController) {
    val b = LocalBrand.current
    val s = LocalStrings.current
    val clip = LocalClipboardManager.current
    val uri = LocalUriHandler.current
    var menu by remember { mutableStateOf(false) }

    BrandBackground {
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(s.t("home.title"), color = b.textPrimary, fontSize = 24.sp, fontWeight = FontWeight.Bold)
                Spacer(Modifier.weight(1f))
                Box {
                    Icon(Icons.Default.MoreVert, "menu", tint = b.textPrimary,
                        modifier = Modifier.clip(CircleShape).size(28.dp)
                            .clickableNoRipple { menu = true })
                    DropdownMenu(menu, { menu = false }) {
                        DropdownMenuItem(
                            text = { Text("Devnet") },
                            leadingIcon = { Icon(Icons.Default.Public, null, tint = if (vm.network == WalletViewModel.Net.DEVNET) b.pink else b.textSecondary) },
                            onClick = { vm.switchNetwork(WalletViewModel.Net.DEVNET); menu = false })
                        DropdownMenuItem(
                            text = { Text("Mainnet") },
                            leadingIcon = { Icon(Icons.Default.Public, null, tint = if (vm.network == WalletViewModel.Net.MAINNET) b.pink else b.textSecondary) },
                            onClick = { vm.switchNetwork(WalletViewModel.Net.MAINNET); menu = false })
                        DropdownMenuItem(
                            text = { Text(s.t("menu.refresh")) },
                            leadingIcon = { Icon(Icons.Default.Refresh, null, tint = b.textSecondary) },
                            onClick = { vm.refresh(); menu = false })
                        DropdownMenuItem(
                            text = { Text(s.t("export.title")) },
                            leadingIcon = { Icon(Icons.Default.Key, null, tint = b.textSecondary) },
                            onClick = { menu = false; nav.navigate("export") })
                        DropdownMenuItem(
                            text = { Text(s.t("menu.delete")) },
                            leadingIcon = { Icon(Icons.Default.Logout, null, tint = b.textSecondary) },
                            onClick = { vm.logout(); menu = false; nav.navigate("welcome") { popUpTo("home") { inclusive = true } } })
                        HorizontalDivider()
                        DropdownMenuItem(
                            text = { Text("${s.t("menu.language")}: ${vm.language.display}", fontWeight = FontWeight.SemiBold, color = b.textSecondary) },
                            leadingIcon = { Icon(Icons.Default.Language, null, tint = b.textSecondary) },
                            onClick = {},
                            enabled = false,
                        )
                        AppLanguage.values().forEach { lang ->
                            DropdownMenuItem(
                                text = { Text(lang.display, fontWeight = if (vm.language == lang) FontWeight.Bold else FontWeight.Normal) },
                                leadingIcon = { if (vm.language == lang) Icon(Icons.Default.Check, null, tint = b.pink) else Spacer(Modifier.size(24.dp)) },
                                onClick = { vm.switchLanguage(lang); menu = false },
                            )
                        }
                    }
                }
            }
            Spacer(Modifier.height(14.dp))

            // balance hero
            Column(Modifier.brandCard(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Box(Modifier.background(b.pink.copy(alpha = 0.12f), RoundedCornerShape(50)).padding(horizontal = 12.dp, vertical = 5.dp)) {
                    Text(vm.network.display, color = b.textPrimary, fontSize = 12.sp, fontWeight = FontWeight.SemiBold)
                }
                Spacer(Modifier.height(10.dp))
                Text(if (vm.hasToken) "${vm.tokenSymbol} ${s.t("home.balance")}" else "SOL ${s.t("home.balance")}", color = b.textSecondary, fontSize = 14.sp)
                val amount = if (vm.hasToken) vm.token?.amount else vm.sol?.amount
                Text(amount?.let { fmt(it) } ?: "—",
                    style = TextStyle(brush = brandGradient(), fontSize = 44.sp, fontWeight = FontWeight.ExtraBold))
                if (vm.hasToken) {
                    Text("${vm.sol?.amount?.let { fmt(it) } ?: "0"} SOL", color = b.textSecondary, fontSize = 14.sp)
                } else {
                    Text(s.t("home.notIssued").format(vm.tokenSymbol), color = b.textSecondary, fontSize = 12.sp)
                }
                vm.address?.let { addr ->
                    Spacer(Modifier.height(10.dp))
                    SelectionContainer {
                        Text(addr, color = b.textSecondary, fontSize = 11.sp,
                            fontFamily = FontFamily.Monospace, textAlign = TextAlign.Center)
                    }
                    Spacer(Modifier.height(6.dp))
                    Row(Modifier.clickableNoRipple { clip.setText(AnnotatedString(addr)) },
                        verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.Default.ContentCopy, null, tint = b.pink, modifier = Modifier.size(16.dp))
                        Text(" ${s.t("common.copyAddress")}", color = b.pink, fontSize = 13.sp)
                    }
                }
                if (vm.loading) { Spacer(Modifier.height(8.dp)); Text(s.t("home.loading"), color = b.textSecondary, fontSize = 12.sp) }
                vm.error?.let { Spacer(Modifier.height(6.dp)); Text(it, color = Color.Red, fontSize = 11.sp) }
            }

            Spacer(Modifier.height(14.dp))
            Row(Modifier.fillMaxWidth()) {
                Box(Modifier.weight(1f)) { SecondaryButton(s.t("home.receive")) { nav.navigate("receive") } }
                Spacer(Modifier.width(12.dp))
                Box(Modifier.weight(1f)) { PrimaryButton(s.t("home.send")) { nav.navigate("send") } }
            }

            if (vm.hasToken) {
                Spacer(Modifier.height(14.dp))
                EntryCard("🔒", s.t("home.staking.title"), s.t("home.staking.sub")) { nav.navigate("staking") }
                Spacer(Modifier.height(12.dp))
                EntryCard("✨", s.t("home.inference.title"), s.t("home.inference.sub")) { nav.navigate("inference") }
            }

            // node entries — always available (running a node earns tokens, doesn't require holding them)
            Spacer(Modifier.height(if (vm.hasToken) 12.dp else 14.dp))
            EntryCard("📱", s.t("home.nodeSettings.title"), s.t("home.nodeSettings.sub")) { nav.navigate("nodesettings") }
            Spacer(Modifier.height(12.dp))
            EntryCard("🧠", s.t("models.title"), s.t("models.sub")) { nav.navigate("models") }
            Spacer(Modifier.height(12.dp))
            EntryCard("📊", s.t("home.nodeMonitor.title"), s.t("home.nodeMonitor.sub")) { nav.navigate("nodemonitor") }

            Spacer(Modifier.height(14.dp))
            Column(Modifier.brandCard()) {
                Text(s.t("home.txHistory"), color = b.textPrimary, fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.height(8.dp))
                if (vm.txs.isEmpty()) {
                    Text(s.t("home.noTx"), color = b.textSecondary, fontSize = 13.sp)
                } else {
                    // Home shows only the latest 10; "더보기" opens the full history.
                    vm.txs.take(10).forEach { tx -> TxRow(tx, b) { uri.openUri(vm.explorerUrl(tx.signature)) } }
                    if (vm.txs.size > 10) {
                        Spacer(Modifier.height(6.dp))
                        Row(Modifier.fillMaxWidth().clickableNoRipple { nav.navigate("history") },
                            horizontalArrangement = Arrangement.Center, verticalAlignment = Alignment.CenterVertically) {
                            Text(s.t("home.more"), color = b.pink, fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
                            Text(" ›", color = b.pink, fontSize = 16.sp)
                        }
                    }
                }
            }
        }
    }
}

// ---------------- Transaction history (full) ----------------

@Composable
fun TransactionHistoryScreen(vm: WalletViewModel, nav: NavController) {
    val b = LocalBrand.current
    val s = LocalStrings.current
    val uri = LocalUriHandler.current
    BrandBackground {
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp)) {
            ScreenHeader(s.t("history.title"), nav)
            Spacer(Modifier.height(14.dp))
            Column(Modifier.brandCard()) {
                if (vm.txs.isEmpty()) {
                    Text(s.t("home.noTx"), color = b.textSecondary, fontSize = 13.sp)
                } else {
                    vm.txs.forEach { tx -> TxRow(tx, b) { uri.openUri(vm.explorerUrl(tx.signature)) } }
                }
            }
            Spacer(Modifier.height(20.dp))
        }
    }
}

@Composable
private fun TxRow(tx: TxRef, b: BrandColors, onOpen: () -> Unit) {
    Row(Modifier.fillMaxWidth().padding(vertical = 6.dp).clickableNoRipple { onOpen() },
        verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(30.dp).clip(CircleShape)
            .background((if (tx.failed) Color.Red else b.pink).copy(alpha = 0.15f)),
            contentAlignment = Alignment.Center) {
            Text(if (tx.failed) "✕" else "✓", color = if (tx.failed) Color.Red else b.pink, fontSize = 13.sp)
        }
        Spacer(Modifier.width(10.dp))
        Text(shorten(tx.signature), color = b.textPrimary, fontSize = 13.sp, fontFamily = FontFamily.Monospace)
        Spacer(Modifier.weight(1f))
        Text("↗", color = b.blue, fontSize = 16.sp)
    }
}

// ---------------- Receive ----------------

@Composable
fun ReceiveScreen(vm: WalletViewModel, nav: NavController) {
    val b = LocalBrand.current
    val s = LocalStrings.current
    val clip = LocalClipboardManager.current
    BrandBackground {
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp),
            horizontalAlignment = Alignment.CenterHorizontally) {
            Text(s.t("receive.title"), color = b.textPrimary, fontSize = 20.sp, fontWeight = FontWeight.Bold)
            Spacer(Modifier.height(16.dp))
            vm.address?.let { addr ->
                Column(Modifier.brandCard(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    Image(Qr.bitmap(addr), null, Modifier.size(220.dp)
                        .background(Color.White, RoundedCornerShape(16.dp)).padding(12.dp))
                    Spacer(Modifier.height(12.dp))
                    SelectionContainer {
                        Text(addr, color = b.textPrimary, fontSize = 13.sp,
                            fontFamily = FontFamily.Monospace, textAlign = TextAlign.Center)
                    }
                }
                Spacer(Modifier.height(16.dp))
                PrimaryButton(s.t("common.copyAddress")) { clip.setText(AnnotatedString(addr)) }
            }
            Spacer(Modifier.height(12.dp))
            SecondaryButton(s.t("common.close")) { nav.popBackStack() }
        }
    }
}

// ---------------- Send ----------------

@Composable
fun SendScreen(vm: WalletViewModel, nav: NavController) {
    val b = LocalBrand.current
    val s = LocalStrings.current
    val scope = rememberCoroutineScope()
    var isToken by remember { mutableStateOf(vm.hasToken) }
    var to by remember { mutableStateOf("") }
    var amount by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var result by remember { mutableStateOf<String?>(null) }
    var err by remember { mutableStateOf<String?>(null) }
    val uri = LocalUriHandler.current

    BrandBackground {
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp)) {
            Text(s.t("send.title"), color = b.textPrimary, fontSize = 20.sp, fontWeight = FontWeight.Bold)
            Spacer(Modifier.height(14.dp))
            Column(Modifier.brandCard()) {
                Text(s.t("send.asset"), color = b.textSecondary, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.height(8.dp))
                if (vm.hasToken) {
                    Row {
                        AssetChip(vm.tokenSymbol, isToken) { isToken = true }
                        Spacer(Modifier.width(8.dp))
                        AssetChip("SOL", !isToken) { isToken = false }
                    }
                } else {
                    Text("SOL · ${vm.network.display}", color = b.textPrimary, fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
                }
            }
            Spacer(Modifier.height(12.dp))
            Column(Modifier.brandCard()) {
                Text(s.t("send.recipient"), color = b.textSecondary, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                OutlinedTextField(to, { to = it }, Modifier.fillMaxWidth(),
                    textStyle = TextStyle(fontFamily = FontFamily.Monospace, fontSize = 13.sp), singleLine = true)
            }
            Spacer(Modifier.height(12.dp))
            Column(Modifier.brandCard()) {
                Text(s.t("send.amount"), color = b.textSecondary, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                OutlinedTextField(amount, { amount = it }, Modifier.fillMaxWidth(),
                    keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(keyboardType = KeyboardType.Decimal),
                    singleLine = true)
            }
            result?.let {
                Spacer(Modifier.height(12.dp))
                Column(Modifier.brandCard()) {
                    Text(s.t("send.done"), color = b.pink, fontWeight = FontWeight.SemiBold)
                    Text(shorten(it), color = b.textSecondary, fontSize = 12.sp, fontFamily = FontFamily.Monospace)
                    Text(s.t("send.viewExplorer"), color = b.blue, fontSize = 13.sp,
                        modifier = Modifier.clickableNoRipple { uri.openUri(vm.explorerUrl(it)) })
                }
            }
            err?.let { Spacer(Modifier.height(8.dp)); Text(it, color = Color.Red, fontSize = 13.sp) }
            Spacer(Modifier.height(16.dp))
            PrimaryButton(if (busy) s.t("send.sending") else s.t("send.title"),
                enabled = !busy && to.isNotBlank() && (amount.toDoubleOrNull() ?: 0.0) > 0) {
                busy = true; err = null; result = null
                val amt = amount.toDouble()
                val dest = to.trim()
                scope.launch {
                    try {
                        result = if (isToken) vm.sendToken(dest, amt) else vm.sendSol(dest, amt)
                        vm.refresh()
                    } catch (e: Exception) { err = e.message } finally { busy = false }
                }
            }
            Spacer(Modifier.height(12.dp))
            SecondaryButton(s.t("common.close")) { nav.popBackStack() }
        }
    }
}

@Composable
fun EntryCard(icon: String, title: String, subtitle: String, onClick: () -> Unit) {
    val b = LocalBrand.current
    Row(Modifier.brandCard().clickableNoRipple { onClick() }, verticalAlignment = Alignment.CenterVertically) {
        Text(icon, fontSize = 22.sp)
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(title, color = b.textPrimary, fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
            Text(subtitle, color = b.textSecondary, fontSize = 12.sp)
        }
        Text("›", color = b.textSecondary, fontSize = 20.sp)
    }
}

@Composable
private fun AssetChip(label: String, selected: Boolean, onClick: () -> Unit) {
    val b = LocalBrand.current
    Box(
        Modifier.clip(RoundedCornerShape(10.dp))
            .background(if (selected) b.pink else b.pink.copy(alpha = 0.12f))
            .clickableNoRipple { onClick() }.padding(horizontal = 18.dp, vertical = 8.dp),
    ) {
        Text(label, color = if (selected) Color.White else b.pink, fontWeight = FontWeight.SemiBold)
    }
}
