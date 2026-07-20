package ai.banya.linkcpp.wallet

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
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
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.navigation.NavController
import android.content.Context
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.encodeToString
import kotlinx.serialization.decodeFromString
import ai.banya.linkcpp.core.GatewayService
import ai.banya.linkcpp.core.PayModel
import ai.banya.linkcpp.core.TokenUsage
import com.mikepenz.markdown.m3.Markdown
import com.mikepenz.markdown.m3.markdownColor
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.flowOn
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** One chat message in the inference conversation. */
@Serializable
data class ChatMessage(
    val role: Role,
    val content: String = "",
    val thinking: Boolean = false,
    val usage: TokenUsage? = null,
    val error: Boolean = false,
    // Display name of the model that produced this reply (run across the network)
    // — shown so the requester knows which model answered.
    val model: String? = null,
) {
    @Serializable
    enum class Role { USER, ASSISTANT }
}

private fun replaceLast(list: List<ChatMessage>, msg: ChatMessage): List<ChatMessage> =
    if (list.isEmpty()) listOf(msg) else list.dropLast(1) + msg

// The conversation is saved locally so it survives app relaunches. Transient
// "thinking" placeholders and error bubbles are dropped; the tail is capped.
private const val CHAT_PREFS = "inference"
private const val CHAT_KEY = "history"
private const val CHAT_MAX = 100
private val chatJson = Json { ignoreUnknownKeys = true; encodeDefaults = true }

private fun loadChat(ctx: Context): List<ChatMessage> = runCatching {
    ctx.getSharedPreferences(CHAT_PREFS, Context.MODE_PRIVATE).getString(CHAT_KEY, null)
        ?.let { chatJson.decodeFromString<List<ChatMessage>>(it) }
        ?.filter { !it.thinking && !it.error }
}.getOrNull() ?: emptyList()

private fun saveChat(ctx: Context, msgs: List<ChatMessage>) {
    runCatching {
        val keep = msgs.filter { !it.thinking && !it.error }.takeLast(CHAT_MAX)
        ctx.getSharedPreferences(CHAT_PREFS, Context.MODE_PRIVATE).edit()
            .putString(CHAT_KEY, chatJson.encodeToString(keep)).apply()
    }
}

private fun clearChat(ctx: Context) {
    ctx.getSharedPreferences(CHAT_PREFS, Context.MODE_PRIVATE).edit().remove(CHAT_KEY).apply()
}

@Composable
fun InferenceScreen(vm: WalletViewModel, nav: NavController) {
    val s = LocalStrings.current
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val gateway = remember(vm.stakingUrl) { GatewayService(vm.stakingUrl) }
    var models by remember { mutableStateOf<List<PayModel>>(emptyList()) }
    var selected by remember { mutableStateOf("") }
    var input by remember { mutableStateOf("") }
    // Load the saved conversation; persisted on every change below.
    var messages by remember { mutableStateOf(loadChat(ctx)) }
    var busy by remember { mutableStateOf(false) }
    val listState = rememberLazyListState()
    // Prepaid-credit state: networked inference streams over /v1/chat/completions
    // (credit-debited) so slow models stream past Cloudflare's 100s timeout.
    var creditBalance by remember { mutableStateOf<Double?>(null) }
    var depositRecipient by remember { mutableStateOf("") }
    var showTopUp by remember { mutableStateOf(false) }

    LaunchedEffect(vm.stakingUrl) {
        val pm = withContext(Dispatchers.IO) { runCatching { gateway.models() }.getOrNull() }
        models = pm?.models ?: emptyList()
        depositRecipient = pm?.recipient ?: ""
        if (selected.isEmpty()) selected = models.firstOrNull()?.id ?: ""
        creditBalance = vm.creditBalance()
    }

    // auto-scroll to the newest message
    LaunchedEffect(messages.size) {
        if (messages.isNotEmpty()) listState.animateScrollToItem(messages.size - 1)
    }
    // persist the conversation on every change (drops thinking/error, caps length)
    LaunchedEffect(messages) { saveChat(ctx, messages) }

    val send: () -> Unit = send@{
        val prompt = input.trim()
        if (prompt.isEmpty() || busy || selected.isEmpty()) return@send
        input = ""
        busy = true
        // Resolve the selected model's display name so the reply can show it.
        val modelName = models.firstOrNull { it.id == selected }?.name ?: selected
        messages = messages +
            ChatMessage(ChatMessage.Role.USER, prompt) +
            ChatMessage(ChatMessage.Role.ASSISTANT, thinking = true, model = modelName)
        // The conversation sent to the model: prior turns + this prompt (drop the
        // placeholder/errors, cap the tail so context stays bounded).
        val convo = messages.filter { !it.thinking && !it.error && it.content.isNotBlank() }
            .takeLast(20).map { it.role.name.lowercase() to it.content }
        scope.launch {
            try {
                val key = vm.creditApiKey()
                var acc = ""
                vm.creditStream(key, selected, convo)
                    .flowOn(Dispatchers.IO)
                    .collect { ev ->
                        when (ev) {
                            is CreditService.ChatEvent.Token -> {
                                acc += ev.text
                                messages = replaceLast(messages, ChatMessage(ChatMessage.Role.ASSISTANT, acc, model = modelName))
                            }
                            is CreditService.ChatEvent.Usage ->
                                messages = replaceLast(messages, ChatMessage(ChatMessage.Role.ASSISTANT, acc, usage = ev.usage, model = modelName))
                        }
                    }
                creditBalance = vm.creditBalance()
                vm.refresh()
            } catch (e: Exception) {
                messages = replaceLast(
                    messages,
                    ChatMessage(ChatMessage.Role.ASSISTANT, e.message ?: s.t("inf.gatewayError"), error = true),
                )
            } finally {
                busy = false
            }
        }
    }

    BrandBackground {
        Column(Modifier.fillMaxSize().padding(horizontal = 20.dp)) {
            Spacer(Modifier.height(20.dp))
            ScreenHeader(s.t("inf.title"), nav) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        creditBalance?.let { "💳 %.2f".format(it) } ?: "💳 ${s.t("inf.credits")}",
                        fontSize = 13.sp, fontWeight = FontWeight.SemiBold, color = LocalBrand.current.pink,
                        modifier = Modifier.clickableNoRipple { showTopUp = true },
                    )
                    if (messages.isNotEmpty()) {
                        Spacer(Modifier.width(14.dp))
                        Text("🗑", fontSize = 18.sp,
                            modifier = Modifier.clickableNoRipple { messages = emptyList(); clearChat(ctx) })
                    }
                }
            }

            LazyColumn(
                state = listState,
                modifier = Modifier.weight(1f).fillMaxWidth(),
                verticalArrangement = Arrangement.spacedBy(14.dp),
                contentPadding = PaddingValues(vertical = 8.dp),
            ) {
                if (messages.isEmpty()) {
                    item { EmptyChat() }
                }
                items(messages.size) { i -> ChatBubble(messages[i]) }
            }

            Composer(
                models = models,
                selected = selected,
                onSelect = { selected = it },
                input = input,
                onInput = { input = it },
                busy = busy,
                onSend = send,
            )
            Spacer(Modifier.height(12.dp))
        }
    }

    if (showTopUp) {
        TopUpDialog(
            balance = creditBalance,
            recipient = depositRecipient,
            onDismiss = { showTopUp = false },
            onTopUp = { amount ->
                runCatching { vm.topUpCredits(amount, depositRecipient) }.fold(
                    onSuccess = { creditBalance = it; vm.refresh(); s.t("inf.topUpOk") },
                    onFailure = { "${s.t("inf.topUpFailed")}: ${it.message}" },
                )
            },
        )
    }
}

/** Credit top-up: transfer KVR on-chain to the gateway vault and credit it to the
 *  prepaid balance that networked (streaming) inference debits. */
@Composable
private fun TopUpDialog(
    balance: Double?,
    recipient: String,
    onDismiss: () -> Unit,
    onTopUp: suspend (Double) -> String,
) {
    val s = LocalStrings.current
    val b = LocalBrand.current
    val scope = rememberCoroutineScope()
    var amount by remember { mutableStateOf("10") }
    var busy by remember { mutableStateOf(false) }
    var msg by remember { mutableStateOf<String?>(null) }
    androidx.compose.material3.AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(s.t("inf.topUpTitle"), fontWeight = FontWeight.Bold) },
        text = {
            Column {
                Text("${s.t("inf.currentBalance")}: ${balance?.let { "%.4f KVR".format(it) } ?: "—"}",
                    color = b.textPrimary, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.height(8.dp))
                Text(s.t("inf.topUpNote"), fontSize = 12.sp, color = b.textSecondary)
                Spacer(Modifier.height(10.dp))
                OutlinedTextField(
                    value = amount, onValueChange = { amount = it }, singleLine = true,
                    label = { Text(s.t("inf.topUpAmount")) },
                )
                msg?.let { Spacer(Modifier.height(8.dp)); Text(it, fontSize = 12.sp, color = b.textSecondary) }
            }
        },
        confirmButton = {
            androidx.compose.material3.TextButton(
                enabled = !busy && recipient.isNotEmpty(),
                onClick = {
                    val amt = amount.toDoubleOrNull() ?: 0.0
                    if (amt <= 0.0) { msg = s.t("inf.topUpInvalid"); return@TextButton }
                    scope.launch { busy = true; msg = onTopUp(amt); busy = false }
                },
            ) { Text(if (busy) "…" else s.t("inf.topUpConfirm"), color = b.pink, fontWeight = FontWeight.Bold) }
        },
        dismissButton = {
            androidx.compose.material3.TextButton(onClick = onDismiss) { Text(s.t("common.close"), color = b.textSecondary) }
        },
    )
}

@Composable
private fun EmptyChat() {
    val b = LocalBrand.current
    val s = LocalStrings.current
    Column(
        Modifier.fillMaxWidth().padding(top = 60.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("✨", fontSize = 40.sp)
        Spacer(Modifier.height(8.dp))
        Text(s.t("inf.title"), color = b.textSecondary, fontSize = 16.sp, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(4.dp))
        Text(s.t("inf.actualNote"), color = b.textSecondary, fontSize = 12.sp)
    }
}

@Composable
private fun ChatBubble(m: ChatMessage) {
    val b = LocalBrand.current
    val s = LocalStrings.current
    if (m.role == ChatMessage.Role.USER) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
            Box(
                Modifier.widthIn(max = 300.dp).clip(RoundedCornerShape(18.dp))
                    .background(brandGradient()).padding(horizontal = 14.dp, vertical = 10.dp),
            ) {
                Text(m.content, color = Color.White, fontSize = 14.sp)
            }
        }
    } else {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.Top) {
            Text("✨", fontSize = 18.sp, modifier = Modifier.padding(top = 4.dp, end = 8.dp))
            Column(
                Modifier.weight(1f)
                    .background(b.card, RoundedCornerShape(18.dp))
                    .border(1.dp, b.stroke, RoundedCornerShape(18.dp))
                    .padding(14.dp),
            ) {
                if (!m.error && !m.model.isNullOrEmpty()) {
                    Text(m.model, color = b.pink, fontSize = 11.sp, fontWeight = FontWeight.SemiBold)
                    Spacer(Modifier.height(4.dp))
                }
                when {
                    m.thinking -> ThinkingIndicator()
                    m.error -> Text(m.content, color = Color.Red, fontSize = 14.sp)
                    else -> Markdown(
                        content = m.content,
                        modifier = Modifier.fillMaxWidth(),
                        colors = markdownColor(text = b.textPrimary, linkText = b.blue),
                    )
                }
                m.usage?.let { u ->
                    Spacer(Modifier.height(8.dp))
                    val cost = u.costToken?.let { " · ${fmt(it)} KVR" } ?: ""
                    Text(
                        "${s.t("inf.actualTokens")}: ${u.totalTokens} tok$cost",
                        color = b.textSecondary, fontSize = 11.sp,
                    )
                }
            }
        }
    }
}

/** Subtle pulsing "thinking…" indicator (three dots animated out of phase). */
@Composable
private fun ThinkingIndicator() {
    val b = LocalBrand.current
    val s = LocalStrings.current
    val transition = rememberInfiniteTransition(label = "thinking")
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(s.t("inf.thinking"), color = b.textSecondary, fontSize = 14.sp)
        Spacer(Modifier.width(8.dp))
        listOf(0, 160, 320).forEach { delay ->
            val alpha by transition.animateFloat(
                initialValue = 0.25f,
                targetValue = 1f,
                animationSpec = infiniteRepeatable(
                    animation = tween(durationMillis = 620, delayMillis = delay),
                    repeatMode = RepeatMode.Reverse,
                ),
                label = "dot$delay",
            )
            Box(
                Modifier.padding(horizontal = 2.dp).size(6.dp).clip(CircleShape)
                    .background(b.pink.copy(alpha = alpha)),
            )
        }
    }
}

@Composable
private fun Composer(
    models: List<PayModel>,
    selected: String,
    onSelect: (String) -> Unit,
    input: String,
    onInput: (String) -> Unit,
    busy: Boolean,
    onSend: () -> Unit,
) {
    val b = LocalBrand.current
    val s = LocalStrings.current
    val focus = LocalFocusManager.current
    Column(Modifier.fillMaxWidth()) {
        if (models.isNotEmpty()) {
            var expanded by remember { mutableStateOf(false) }
            val selectedName = models.firstOrNull { it.id == selected }?.name ?: models.first().name
            Box(Modifier.fillMaxWidth()) {
                Row(
                    Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp))
                        .border(1.dp, b.stroke, RoundedCornerShape(12.dp))
                        .clickableNoRipple { expanded = true }
                        .padding(horizontal = 14.dp, vertical = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(selectedName, color = b.textPrimary, fontSize = 14.sp, modifier = Modifier.weight(1f))
                    Icon(Icons.Default.ArrowDropDown, contentDescription = null, tint = b.textSecondary)
                }
                DropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
                    models.forEach { m ->
                        DropdownMenuItem(text = { Text(m.name) }, onClick = { onSelect(m.id); expanded = false })
                    }
                }
            }
            Spacer(Modifier.height(10.dp))
        }
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.Bottom) {
            OutlinedTextField(
                value = input,
                onValueChange = onInput,
                modifier = Modifier.weight(1f).heightIn(min = 56.dp, max = 140.dp),
                placeholder = { Text(s.t("inf.chatHint"), fontSize = 14.sp) },
                maxLines = 5,
            )
            Spacer(Modifier.width(8.dp))
            val enabled = !busy && input.isNotBlank() && selected.isNotEmpty()
            val bg = if (enabled) brandGradient() else Brush.linearGradient(listOf(b.stroke, b.stroke))
            Box(
                Modifier.height(56.dp).widthIn(min = 84.dp).clip(RoundedCornerShape(16.dp))
                    .background(bg).clickableNoRipple { if (enabled) { focus.clearFocus(); onSend() } }
                    .padding(horizontal = 18.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    if (busy) "…" else s.t("inf.payRun"),
                    color = Color.White, fontWeight = FontWeight.SemiBold, fontSize = 15.sp,
                )
            }
        }
    }
}

@Composable
private fun ModelChip(label: String, selected: Boolean, onClick: () -> Unit) {
    val b = LocalBrand.current
    Box(
        Modifier.clip(RoundedCornerShape(10.dp))
            .background(if (selected) b.pink else b.pink.copy(alpha = 0.12f))
            .clickableNoRipple { onClick() }.padding(horizontal = 16.dp, vertical = 8.dp),
    ) {
        Text(label, color = if (selected) Color.White else b.pink, fontWeight = FontWeight.SemiBold, fontSize = 14.sp)
    }
}
