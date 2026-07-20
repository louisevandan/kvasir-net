package ai.banya.linkcpp.wallet

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

data class BrandColors(
    val pink: Color, val blue: Color,
    val bgTop: Color, val bgBottom: Color,
    val card: Color, val stroke: Color,
    val textPrimary: Color, val textSecondary: Color,
)

private val LightBrand = BrandColors(
    pink = Color(0xFFEC6FA6), blue = Color(0xFF6FA8E6),
    bgTop = Color(0xFFFDEFF5), bgBottom = Color(0xFFE9F2FE),
    card = Color(0xFFFFFFFF), stroke = Color(0xFFF1E2EA),
    textPrimary = Color(0xFF2C2533), textSecondary = Color(0xFF938A9C),
)
private val DarkBrand = BrandColors(
    pink = Color(0xFFF490BA), blue = Color(0xFF9AC3F5),
    bgTop = Color(0xFF1B1522), bgBottom = Color(0xFF121722),
    card = Color(0xFF241E2E), stroke = Color(0xFF352C40),
    textPrimary = Color(0xFFF3ECF6), textSecondary = Color(0xFFACA1B8),
)

val LocalBrand = staticCompositionLocalOf { LightBrand }

@Composable
fun LinkcppTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val brand = if (dark) DarkBrand else LightBrand
    val scheme = if (dark) darkColorScheme(primary = brand.pink) else lightColorScheme(primary = brand.pink)
    CompositionLocalProvider(LocalBrand provides brand) {
        MaterialTheme(colorScheme = scheme, content = content)
    }
}

@Composable fun brandGradient(): Brush {
    val b = LocalBrand.current
    return Brush.linearGradient(listOf(b.pink, b.blue))
}

@Composable
fun BrandBackground(content: @Composable BoxScope.() -> Unit) {
    val b = LocalBrand.current
    Box(Modifier.fillMaxSize().background(Brush.verticalGradient(listOf(b.bgTop, b.bgBottom)))) {
        // gradient is full-bleed; content respects the status/navigation bars (like iOS safe area)
        Box(Modifier.fillMaxSize().systemBarsPadding(), content = content)
    }
}

@Composable
fun Modifier.brandCard(padding: Dp = 20.dp): Modifier {
    val b = LocalBrand.current
    return this
        .fillMaxWidth()
        .background(b.card, RoundedCornerShape(24.dp))
        .border(1.dp, b.stroke, RoundedCornerShape(24.dp))
        .padding(padding)
}

@Composable
fun PrimaryButton(text: String, enabled: Boolean = true, onClick: () -> Unit) {
    val b = LocalBrand.current
    val bg = if (enabled) brandGradient() else Brush.linearGradient(listOf(b.stroke, b.stroke))
    Box(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(bg)
            .clickable(enabled = enabled) { onClick() }.padding(vertical = 15.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(text, color = Color.White, fontWeight = FontWeight.SemiBold, fontSize = 16.sp)
    }
}

@Composable
fun SecondaryButton(text: String, enabled: Boolean = true, onClick: () -> Unit) {
    val b = LocalBrand.current
    Box(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp))
            .background(b.pink.copy(alpha = 0.12f))
            .clickable(enabled = enabled) { onClick() }.padding(vertical = 15.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(text, color = b.pink, fontWeight = FontWeight.Medium, fontSize = 16.sp)
    }
}

@Composable
fun Modifier.clickableNoRipple(onClick: () -> Unit): Modifier {
    val source = remember { MutableInteractionSource() }
    return this.clickable(interactionSource = source, indication = null, onClick = onClick)
}

@Composable
fun Title(text: String) {
    Text(text, color = LocalBrand.current.textPrimary, fontSize = 28.sp, fontWeight = FontWeight.Bold, textAlign = TextAlign.Center)
}
