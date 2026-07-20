package ai.banya.linkcpp.wallet

import android.content.Context
import android.content.ContextWrapper
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricPrompt
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import androidx.navigation.NavController

private fun Context.findFragmentActivity(): FragmentActivity? {
    var ctx: Context? = this
    while (ctx is ContextWrapper) {
        if (ctx is FragmentActivity) return ctx
        ctx = ctx.baseContext
    }
    return null
}

@Composable
fun ExportPhraseScreen(vm: WalletViewModel, nav: NavController) {
    val b = LocalBrand.current
    val s = LocalStrings.current
    val clip = LocalClipboardManager.current
    val context = LocalContext.current
    val activity = remember(context) { context.findFragmentActivity() }
    var words by remember { mutableStateOf<List<String>?>(null) }
    var err by remember { mutableStateOf<String?>(null) }

    fun doReveal() {
        val w = vm.revealMnemonic()
        if (w.isNullOrEmpty()) err = s.t("error.invalidMnemonic") else { words = w; err = null }
    }

    // Gate the reveal behind device auth (biometric / device credential) when available.
    fun authAndReveal() {
        err = null
        val act = activity
        val allowed = BiometricManager.Authenticators.BIOMETRIC_WEAK or
            BiometricManager.Authenticators.DEVICE_CREDENTIAL
        if (act == null || BiometricManager.from(act).canAuthenticate(allowed) != BiometricManager.BIOMETRIC_SUCCESS) {
            doReveal() // no enrolled auth on this device — fall back to the on-screen warning gate
            return
        }
        val prompt = BiometricPrompt(
            act,
            ContextCompat.getMainExecutor(act),
            object : BiometricPrompt.AuthenticationCallback() {
                override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) = doReveal()
                override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                    if (errorCode != BiometricPrompt.ERROR_USER_CANCELED &&
                        errorCode != BiometricPrompt.ERROR_NEGATIVE_BUTTON
                    ) err = errString.toString()
                }
            },
        )
        val info = BiometricPrompt.PromptInfo.Builder()
            .setTitle(s.t("export.title"))
            .setSubtitle(s.t("export.reveal"))
            .setAllowedAuthenticators(allowed)
            .build()
        prompt.authenticate(info)
    }

    BrandBackground {
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp)) {
            ScreenHeader(s.t("export.title"), nav)

            Column(Modifier.brandCard()) {
                Text(s.t("export.desc"), color = b.textSecondary, fontSize = 13.sp)
                Spacer(Modifier.height(14.dp))

                val current = words
                if (current == null) {
                    Box(
                        Modifier.fillMaxWidth().padding(vertical = 4.dp)
                            .background(b.pink.copy(alpha = 0.12f), RoundedCornerShape(16.dp))
                            .clickableNoRipple { authAndReveal() }.padding(vertical = 15.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(s.t("export.reveal"), color = b.pink, fontWeight = FontWeight.SemiBold, fontSize = 16.sp)
                    }
                } else {
                    Text(s.t("export.warn"), color = Color(0xFFD9534F), fontSize = 12.sp, fontWeight = FontWeight.Medium)
                    Spacer(Modifier.height(12.dp))
                    SelectionContainer {
                        Column(Modifier.fillMaxWidth()) {
                            current.chunked(3).forEachIndexed { r, row ->
                                Row(Modifier.fillMaxWidth().padding(vertical = 5.dp)) {
                                    row.forEachIndexed { i, w ->
                                        Row(
                                            Modifier.weight(1f).padding(horizontal = 4.dp)
                                                .background(b.pink.copy(alpha = 0.08f), RoundedCornerShape(10.dp))
                                                .padding(horizontal = 10.dp, vertical = 8.dp),
                                            verticalAlignment = Alignment.CenterVertically,
                                        ) {
                                            Text(
                                                "${r * 3 + i + 1}", color = b.pink, fontSize = 12.sp,
                                                fontWeight = FontWeight.Bold, modifier = Modifier.width(20.dp),
                                            )
                                            Text(w, color = b.textPrimary, fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Spacer(Modifier.height(16.dp))
                    PrimaryButton(s.t("export.copy")) { clip.setText(AnnotatedString(current.joinToString(" "))) }
                    Spacer(Modifier.height(10.dp))
                    SecondaryButton(s.t("export.hide")) { words = null }
                }

                err?.let { Spacer(Modifier.height(10.dp)); Text(it, color = Color.Red, fontSize = 12.sp) }
            }
            Spacer(Modifier.height(20.dp))
        }
    }
}
