package com.soyeht.next.android.theme

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.IndicationNodeFactory
import androidx.compose.foundation.LocalIndication
import androidx.compose.foundation.LocalOverscrollConfiguration
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.LocalRippleConfiguration
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.node.DelegatableNode
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.sp

/** The palette roles as Compose colors. Views read these and never write a color of their own. */
class Roles(val palette: Palette) {
    val screen = Color(palette.screen)
    val canvas = Color(palette.canvas)
    val face = Color(palette.face)
    val hover = Color(palette.hover)
    val well = Color(palette.well)
    val ink = Color(palette.ink)
    val secondary = Color(palette.secondary)
    val muted = Color(palette.muted)
    val accent = Color(palette.accent)
    val onAccent = Color(palette.onAccent)
    val selection = Color(palette.selection)

    /** The edge of a card, well, pill or modal, and dimmed text's ink: set by the e-ink palette only. */
    val border = palette.border?.let { Color(it) }
    val dim = palette.dim?.let { Color(it) }

    /** The terminal's ground: on the e-ink page, with an edge, so its colors keep their contrast; else the well. */
    val terminalWell = if (border != null) screen else well

    // The bands of a gauge are the palette's own red, green and yellow (the ANSI slots 1, 2 and 3).
    val danger = Color(palette.ansi[1])
    val success = Color(palette.ansi[2])
    val warning = Color(palette.ansi[3])
}

val LocalRoles = staticCompositionLocalOf { Roles(Palettes.milk) }

/**
 * The first palette of the token file is the default, as on the other clients. In e-ink mode the e-ink
 * palette and sizes replace it, and nothing animates: no ripple, no pressed indication, no overscroll glow.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)
@Composable
fun SoyehtTheme(eink: Boolean = false, content: @Composable () -> Unit) {
    val roles = Roles(if (eink) Palettes.eink else Palettes.all.first().second)
    val scheme = lightColorScheme(
        primary = roles.accent,
        onPrimary = roles.onAccent,
        background = roles.screen,
        onBackground = roles.ink,
        surface = roles.face,
        onSurface = roles.ink,
        surfaceVariant = roles.well,
        onSurfaceVariant = roles.secondary,
    )
    CompositionLocalProvider(
        LocalRoles provides roles,
        LocalMetrics provides if (eink) Metrics.eink else Metrics.standard,
    ) {
        MaterialTheme(colorScheme = scheme, typography = if (eink) einkTypography() else Typography()) {
            if (eink) {
                CompositionLocalProvider(
                    LocalIndication provides NoIndication,
                    LocalRippleConfiguration provides null,
                    LocalOverscrollConfiguration provides null,
                    content = content,
                )
            } else {
                content()
            }
        }
    }
}

/** The text a Material component sets without a size of its own (a dialog, a menu, a button) takes the e-ink sizes. */
private fun einkTypography() = Typography(
    bodyLarge = TextStyle(fontSize = Eink.bodyTextSize.sp),
    bodyMedium = TextStyle(fontSize = Eink.bodyTextSize.sp),
    bodySmall = TextStyle(fontSize = Eink.captionTextSize.sp),
    labelLarge = TextStyle(fontSize = Eink.bodyTextSize.sp),
    titleLarge = TextStyle(fontSize = Eink.headingTextSize.sp),
    titleMedium = TextStyle(fontSize = Eink.subheadingTextSize.sp),
    headlineSmall = TextStyle(fontSize = Eink.headingTextSize.sp),
)

/** No pressed or hover effect: on e-ink every frame of a ripple is a refresh. */
private object NoIndication : IndicationNodeFactory {
    override fun create(interactionSource: InteractionSource): DelegatableNode = object : Modifier.Node() {}

    override fun equals(other: Any?) = other === this

    override fun hashCode() = System.identityHashCode(this)
}
