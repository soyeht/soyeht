package com.soyeht.next.android.ui

import androidx.compose.foundation.*
import androidx.compose.foundation.gestures.FlingBehavior
import androidx.compose.foundation.gestures.ScrollScope
import androidx.compose.foundation.gestures.ScrollableDefaults
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.*
import com.soyeht.next.android.theme.*

/**
 * A filled surface. On e-ink the fill differs from the page by too few greys to show an edge, so the
 * surface also draws the palette's border; elsewhere the fill alone is the surface.
 */
@Composable
fun Modifier.panel(fill: Color, shape: Shape): Modifier {
    val border = LocalRoles.current.border
    val width = LocalMetrics.current.borderWidth
    val filled = background(fill, shape)
    return if (border != null) filled.border(width.dp, border, shape) else filled
}

/** The corners of a modal. */
val dialogShape = RoundedCornerShape(Radius.card.dp)

/**
 * The edge of a modal. Material draws a bare card, and on e-ink a white card on a white page has no outline;
 * the modal takes the border of the palette, which the other palettes do not have.
 */
@Composable
fun dialogModifier(): Modifier {
    val border = LocalRoles.current.border ?: return Modifier
    return Modifier.border(LocalMetrics.current.borderWidth.dp, border, dialogShape)
}

/** A fling that stops where the finger lifts: on e-ink a coasting list is a stream of refreshes. */
private object NoFling : FlingBehavior {
    override suspend fun ScrollScope.performFling(initialVelocity: Float) = initialVelocity
}

/** The fling behavior of the mode: none on e-ink, the platform's otherwise. */
@Composable
fun flingBehavior(): FlingBehavior =
    if (LocalMetrics.current.eink) NoFling else ScrollableDefaults.flingBehavior()

/** The bar over a screen: a back button when there is somewhere to go back to, the title, and actions. */
@Composable
fun TopBar(title: String, subtitle: String? = null, onBack: (() -> Unit)? = null, actions: @Composable () -> Unit = {}) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    Row(
        Modifier.fillMaxWidth().padding(horizontal = PaneInterior.phoneInset.dp, vertical = PaneInterior.compactSpacing.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(PaneInterior.compactSpacing.dp),
    ) {
        if (onBack != null) {
            Pill("‹", onBack, description = "Back")
        }
        Column(Modifier.weight(1f)) {
            Text(title, color = roles.ink, fontSize = metrics.subheading.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
            if (subtitle != null) {
                Text(subtitle, color = roles.secondary, fontSize = metrics.small.sp, maxLines = 1)
            }
        }
        actions()
    }
}

/** A flat, tappable label with a touch target of the size the tokens set. */
@Composable
fun Pill(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    primary: Boolean = false,
    description: String = label,
    padding: Float = PaneInterior.pillHorizontalPadding,
) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    Box(
        modifier
            .defaultMinSize(minWidth = metrics.touchTarget.dp, minHeight = metrics.touchTarget.dp)
            .panel(if (primary) roles.accent else roles.well, RoundedCornerShape(Radius.control.dp))
            .clickable(onClickLabel = description, onClick = onClick)
            .padding(horizontal = padding.dp, vertical = PaneInterior.pillVerticalPadding.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = if (primary) roles.onAccent else roles.ink, fontSize = metrics.body.sp)
    }
}

/** The mode's own switch: e-ink on or off, kept across launches. */
@Composable
fun EinkSwitch(on: Boolean, toggle: () -> Unit) {
    Pill(if (on) "E-ink: on" else "E-ink: off", toggle, description = "Switch the e-ink mode")
}
