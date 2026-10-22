package fr.apex.maint.ui.theme

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// Valeurs de static/styles.css, alpha conservé : les cartes laissent voir la lueur du fond.
@Immutable
data class ApexTokens(
    val bg: Color, val glass: Color, val glass2: Color, val line: Color, val line2: Color,
    val text: Color, val muted: Color, val accent: Color, val accent2: Color, val violet: Color,
    val run: Color, val warn: Color, val crit: Color, val glow: Color, val bar: Color,
    val nav: Color, val navSolid: Color, val field: Color, val selected: Color, val avatar: Color,
    val onAccent: Color, val backdrop: Color, val okText: Color, val okBg: Color, val okLine: Color,
    val koText: Color, val koBg: Color, val koLine: Color,
    val stop: Color = Color(0xFF64748B),
)

val DarkTokens = ApexTokens(
    bg = Color(0xFF070D1A), glass = Color(0xFF121E36).copy(alpha = .78f),
    glass2 = Color(0xFF1C2C4C).copy(alpha = .9f),
    line = Color(0xFF78A0DC).copy(alpha = .14f), line2 = Color(0xFF78A0DC).copy(alpha = .28f),
    text = Color(0xFFE8EEF8), muted = Color(0xFF8EA2C0), accent = Color(0xFF38BDF8),
    accent2 = Color(0xFF0EA5E9), violet = Color(0xFF818CF8), run = Color(0xFF22C55E),
    warn = Color(0xFFF59E0B), crit = Color(0xFFEF4444), glow = Color(0xFF13284D),
    bar = Color(0xFF091020).copy(alpha = .8f), nav = Color(0xFF091020).copy(alpha = .65f),
    navSolid = Color(0xFF091020).copy(alpha = .97f), field = Color(0xFF080E1C).copy(alpha = .8f),
    selected = Color(0xFF38BDF8).copy(alpha = .14f), avatar = Color(0xFF1E3A64),
    onAccent = Color(0xFF061020), backdrop = Color(0xFF050911).copy(alpha = .6f),
    okText = Color(0xFF86EFAC), okBg = Color(0xFF22C55E).copy(alpha = .1f),
    okLine = Color(0xFF22C55E).copy(alpha = .35f), koText = Color(0xFFFCA5A5),
    koBg = Color(0xFFEF4444).copy(alpha = .14f), koLine = Color(0xFFEF4444).copy(alpha = .45f),
)
val LightTokens = ApexTokens(
    bg = Color(0xFFEEF2F8), glass = Color.White.copy(alpha = .82f), glass2 = Color.White,
    line = Color(0xFF1E3C6E).copy(alpha = .13f), line2 = Color(0xFF1E3C6E).copy(alpha = .26f),
    text = Color(0xFF0F1B2E), muted = Color(0xFF56667F), accent = Color(0xFF0284C7),
    accent2 = Color(0xFF0284C7), violet = Color(0xFF6366F1), run = Color(0xFF16A34A),
    warn = Color(0xFFD97706), crit = Color(0xFFDC2626), glow = Color(0xFFD6E6FB),
    bar = Color.White.copy(alpha = .85f), nav = Color(0xFFF8FAFD).copy(alpha = .9f),
    navSolid = Color(0xFFF8FAFD), field = Color.White, selected = Color(0xFF0284C7).copy(alpha = .12f),
    avatar = Color(0xFFDBEAFE), onAccent = Color.White, backdrop = Color(0xFF0F1B2E).copy(alpha = .35f),
    okText = Color(0xFF15803D), okBg = Color(0xFF16A34A).copy(alpha = .1f),
    okLine = Color(0xFF16A34A).copy(alpha = .35f), koText = Color(0xFFB91C1C),
    koBg = Color(0xFFDC2626).copy(alpha = .08f), koLine = Color(0xFFDC2626).copy(alpha = .35f),
)
val LocalApexTokens = staticCompositionLocalOf { DarkTokens }

object ApexLayout {
    val unit = 4.dp
    val gap = 8.dp
    val itemGap = 12.dp
    val padding = 16.dp
    val touch = 48.dp
    val formWidth = 680.dp
    val radius = 10.dp
    val fieldRadius = 7.dp
    val badgeRadius = 12.dp
    val railWidth = 112.dp
    const val railBreakpoint = 600
    const val detailBreakpoint = 840
}

private fun style(size: Int, line: Int, weight: FontWeight = FontWeight.Normal) = TextStyle(
    fontFamily = FontFamily.SansSerif, fontSize = size.sp, lineHeight = line.sp, fontWeight = weight,
)
val ApexTypography = Typography(
    displayLarge = style(40, 48, FontWeight.Bold), displayMedium = style(36, 44, FontWeight.Bold),
    displaySmall = style(32, 40, FontWeight.Bold), headlineLarge = style(28, 36, FontWeight.Bold),
    headlineMedium = style(26, 34, FontWeight.Bold), headlineSmall = style(24, 32, FontWeight.Bold),
    titleLarge = style(20, 28, FontWeight.SemiBold), titleMedium = style(16, 24, FontWeight.SemiBold),
    titleSmall = style(14, 20, FontWeight.SemiBold), bodyLarge = style(16, 24),
    bodyMedium = style(14, 20), bodySmall = style(12, 16), labelLarge = style(14, 20, FontWeight.Medium),
    labelMedium = style(12, 16, FontWeight.SemiBold), labelSmall = style(12, 16, FontWeight.Medium),
)

private fun ApexTokens.scheme(dark: Boolean): ColorScheme {
    val base = if (dark) darkColorScheme() else lightColorScheme()
    return base.copy(
        primary = accent, onPrimary = onAccent, primaryContainer = selected, onPrimaryContainer = text,
        secondary = violet, onSecondary = onAccent, secondaryContainer = selected, onSecondaryContainer = text,
        tertiary = run, onTertiary = bg, tertiaryContainer = okBg, onTertiaryContainer = okText,
        background = bg, onBackground = text, surface = glass2, onSurface = text,
        surfaceVariant = glass, onSurfaceVariant = muted, surfaceTint = Color.Transparent,
        surfaceBright = glass2, surfaceDim = bg, surfaceContainerLowest = bg,
        surfaceContainerLow = glass, surfaceContainer = glass2, surfaceContainerHigh = glass2,
        surfaceContainerHighest = field, outline = line2, outlineVariant = line,
        error = crit, onError = onAccent, errorContainer = koBg, onErrorContainer = koText,
        inverseSurface = if (dark) LightTokens.navSolid else DarkTokens.navSolid,
        inverseOnSurface = if (dark) LightTokens.text else DarkTokens.text,
        inversePrimary = if (dark) LightTokens.accent else DarkTokens.accent, scrim = backdrop,
        primaryFixed = LightTokens.selected, primaryFixedDim = DarkTokens.selected,
        onPrimaryFixed = LightTokens.text, onPrimaryFixedVariant = LightTokens.muted,
        secondaryFixed = LightTokens.glow, secondaryFixedDim = DarkTokens.glow,
        onSecondaryFixed = LightTokens.text, onSecondaryFixedVariant = LightTokens.muted,
        tertiaryFixed = LightTokens.okBg, tertiaryFixedDim = DarkTokens.okBg,
        onTertiaryFixed = LightTokens.okText, onTertiaryFixedVariant = LightTokens.text,
    )
}

@Composable fun ApexTheme(dark: Boolean, content: @Composable () -> Unit) {
    val tokens = if (dark) DarkTokens else LightTokens
    CompositionLocalProvider(LocalApexTokens provides tokens) {
        MaterialTheme(colorScheme = tokens.scheme(dark), typography = ApexTypography,
            shapes = Shapes(extraSmall = RoundedCornerShape(ApexLayout.fieldRadius),
                small = RoundedCornerShape(8.dp), medium = RoundedCornerShape(ApexLayout.radius),
                large = RoundedCornerShape(ApexLayout.radius), extraLarge = RoundedCornerShape(ApexLayout.radius))) {
            Box(Modifier.fillMaxSize().background(tokens.bg).drawWithCache {
                val glow = Brush.radialGradient(colors = listOf(tokens.glow, Color.Transparent),
                    center = Offset(size.width * .7f, -size.height * .1f),
                    radius = maxOf(size.width, size.height) * .85f)
                onDrawBehind { drawRect(glow) }
            }) {
                // Les Scaffold transparents héritent de cette couleur : sans elle, le texte resterait noir.
                CompositionLocalProvider(LocalContentColor provides tokens.text, content = content)
            }
        }
    }
}

@Immutable data class BadgeColors(val text: Color, val background: Color, val border: Color)
fun ApexTokens.badge(value: String): BadgeColors = when (value) {
    "completed", "operational", "running", "FONCTIONNEL" -> BadgeColors(okText, okBg, okLine)
    "critical", "out_of_service", "stopped" -> BadgeColors(koText, koBg, koLine)
    "degraded" -> BadgeColors(warn, Color(0xFFF59E0B).copy(alpha = .15f), Color(0xFFF59E0B).copy(alpha = .35f))
    "in_progress", "high", "DEGRADE" -> BadgeColors(warn,
        Color(0xFFF59E0B).copy(alpha = .15f), Color(0xFFF59E0B).copy(alpha = .4f))
    "ARRET" -> if (bg == LightTokens.bg) BadgeColors(crit, crit.copy(alpha = .12f), crit.copy(alpha = .4f))
        else BadgeColors(Color(0xFFF87171), crit.copy(alpha = .2f), crit.copy(alpha = .55f))
    "cancelled" -> BadgeColors(muted, Color(0xFF94A3B8).copy(alpha = .16f), Color(0xFF94A3B8).copy(alpha = .35f))
    "low", "unknown" -> BadgeColors(muted, Color(0xFF94A3B8).copy(alpha = .12f), Color(0xFF94A3B8).copy(alpha = .3f))
    else -> BadgeColors(accent, Color(0xFF38BDF8).copy(alpha = if (value == "medium") .12f else .15f),
        Color(0xFF38BDF8).copy(alpha = if (value == "medium") .3f else .4f))
}
