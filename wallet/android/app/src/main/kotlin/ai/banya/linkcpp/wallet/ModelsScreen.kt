package ai.banya.linkcpp.wallet

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.navigation.NavController
import java.io.File

/** A GGUF the hub has staged onto this device (filesDir/models). */
private data class LocalModel(val name: String, val sizeBytes: Long) {
    val display: String get() = name.removeSuffix(".gguf")
    val sizeLabel: String get() = when {
        sizeBytes >= 1L shl 30 -> "%.1f GB".format(sizeBytes.toDouble() / (1L shl 30))
        sizeBytes >= 1L shl 20 -> "%d MB".format(sizeBytes / (1L shl 20))
        else -> "%d KB".format(sizeBytes / (1L shl 10))
    }
}

/**
 * Manage models the hub has downloaded to this phone: list each GGUF with its
 * size and delete ones no longer needed. Mirrors the iOS/desktop models page.
 */
@androidx.compose.runtime.Composable
fun ModelsScreen(vm: WalletViewModel, nav: NavController) {
    val b = LocalBrand.current
    val s = LocalStrings.current
    val ctx = LocalContext.current
    val dir = remember { File(ctx.filesDir, "models").apply { mkdirs() } }
    var models by remember { mutableStateOf(listModels(dir)) }
    var pendingDelete by remember { mutableStateOf<String?>(null) }

    BrandBackground {
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(20.dp)) {
            ScreenHeader(s.t("models.title"), nav)

            Column(Modifier.brandCard()) {
                Text(s.t("models.storedTitle"), color = b.textPrimary, fontSize = 16.sp, fontWeight = FontWeight.Bold)
                Spacer(Modifier.height(4.dp))
                Text(s.t("models.storedDesc"), color = b.textSecondary, fontSize = 12.sp)
                Spacer(Modifier.height(8.dp))
                Text(dir.absolutePath, color = b.textSecondary, fontSize = 10.sp, fontFamily = FontFamily.Monospace)
            }
            Spacer(Modifier.height(12.dp))

            if (models.isEmpty()) {
                Column(Modifier.brandCard()) {
                    Text(s.t("models.emptyHint"), color = b.textSecondary, fontSize = 13.sp)
                }
            } else {
                models.forEach { m ->
                    Row(Modifier.brandCard(), verticalAlignment = Alignment.CenterVertically) {
                        Text("🧠", fontSize = 20.sp)
                        Spacer(Modifier.width(12.dp))
                        Column(Modifier.weight(1f)) {
                            Text(m.display, color = b.textPrimary, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
                            Text(m.sizeLabel, color = b.textSecondary, fontSize = 11.sp)
                        }
                        if (pendingDelete == m.name) {
                            Text(s.t("models.delete"), color = b.pink, fontSize = 13.sp, fontWeight = FontWeight.Bold,
                                modifier = Modifier.clickableNoRipple {
                                    File(dir, m.name).delete(); models = listModels(dir); pendingDelete = null
                                }.padding(horizontal = 8.dp))
                            Text(s.t("common.cancel"), color = b.textSecondary, fontSize = 13.sp,
                                modifier = Modifier.clickableNoRipple { pendingDelete = null }.padding(horizontal = 4.dp))
                        } else {
                            Text("🗑", fontSize = 18.sp, modifier = Modifier.clickableNoRipple { pendingDelete = m.name }.padding(4.dp))
                        }
                    }
                    Spacer(Modifier.height(8.dp))
                }
            }
        }
    }
}

private fun listModels(dir: File): List<LocalModel> =
    (dir.listFiles { f -> f.name.endsWith(".gguf") } ?: emptyArray())
        .map { LocalModel(it.name, it.length()) }
        .sortedBy { it.name }
