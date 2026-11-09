package com.soyeht.next.android.ui

import androidx.compose.foundation.*
import androidx.compose.foundation.gestures.FlingBehavior
import androidx.compose.foundation.gestures.ScrollScope
import androidx.compose.foundation.gestures.ScrollableDefaults
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.*
import com.soyeht.next.android.theme.*
import uniffi.soyeht_android.PaneDot

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

/**
 * A raised (neumorphic) surface: a light shadow up and to the left and a dark one down and to the right, from an
 * [Elevation] of the tokens, over a [fill]. Where the palette has an edge (the e-ink one) the surface draws that edge
 * and no shadow, because a screen of sixteen greys shows an edge and not a shadow. Only the pane's shell, the top bar's
 * tabs and buttons, and the rail's selection are raised; everything inside a pane is flat.
 */
@Composable
fun Modifier.raised(fill: Color, radius: Float, shadow: Shadow): Modifier {
    val roles = LocalRoles.current
    if (roles.border != null) return panel(fill, RoundedCornerShape(radius.dp))
    val palette = roles.palette
    return drawBehind {
        val corner = radius.dp.toPx()
        drawIntoCanvas { canvas ->
            val paint = android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG).apply { color = fill.toArgb() }
            val rect = android.graphics.RectF(0f, 0f, size.width, size.height)
            paint.setShadowLayer(shadow.lightBlur.dp.toPx(), -shadow.lightOffset.dp.toPx(), -shadow.lightOffset.dp.toPx(), palette.shadowLight.toInt())
            canvas.nativeCanvas.drawRoundRect(rect, corner, corner, paint)
            paint.setShadowLayer(shadow.darkBlur.dp.toPx(), shadow.darkOffset.dp.toPx(), shadow.darkOffset.dp.toPx(), palette.shadowDark.toInt())
            canvas.nativeCanvas.drawRoundRect(rect, corner, corner, paint)
        }
    }
}

/** The pane's shell: the only raised surface of a pane, a card of the palette's face. */
@Composable
fun Modifier.paneShell(): Modifier =
    raised(LocalRoles.current.face, PaneShell.cornerRadius, Shadow(PaneShell.shadowOffset, PaneShell.shadowBlur, PaneShell.shadowOffset, PaneShell.shadowBlur))

/** A one-line rule between the parts of a pane, in the palette's well (the e-ink palette's edge). */
@Composable
fun Separator(modifier: Modifier = Modifier) {
    val roles = LocalRoles.current
    Box(modifier.fillMaxWidth().height(PaneInterior.separatorWidth.dp).background(roles.border ?: roles.well))
}

/** The dot at the head of a pane's header and tab: the palette role the contract gives the pane's state. */
@Composable
fun StateDot(dot: PaneDot, modifier: Modifier = Modifier) {
    val roles = LocalRoles.current
    val color = when (dot) {
        PaneDot.SUCCESS -> roles.success
        PaneDot.ACCENT -> roles.accent
        PaneDot.WARNING -> roles.warning
        PaneDot.MUTED -> roles.muted
        PaneDot.SECONDARY -> roles.secondary
    }
    Box(modifier.size(PaneLayout.headerStateDot.dp).background(color, CircleShape))
}

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
        if (onBack != null) SquareButton(Icon.Back, onBack, "Back")
        Column(Modifier.weight(1f)) {
            Text(title, color = roles.ink, fontSize = metrics.subheading.sp, fontWeight = FontWeight.Bold, fontFamily = Faces.chrome, maxLines = 1)
            if (subtitle != null) {
                Text(subtitle, color = roles.secondary, fontSize = metrics.small.sp, maxLines = 1)
            }
        }
        actions()
    }
}

/** How a [Pill] reads: the main action takes the ink, the others the well, and a refusal the palette's red. */
enum class Tone { Primary, Neutral, Danger }

/** A tappable label: a pill with a touch target of the size the tokens set. */
@Composable
fun Pill(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    tone: Tone = Tone.Neutral,
    description: String = label,
    padding: Float = PaneInterior.pillHorizontalPadding,
    textSize: Float? = null,
) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    val fill = if (tone == Tone.Primary) roles.ink else roles.well
    val ink = when (tone) {
        Tone.Primary -> roles.face
        Tone.Neutral -> roles.ink
        Tone.Danger -> roles.danger
    }
    Box(
        modifier
            .defaultMinSize(minHeight = metrics.controlHeight.dp)
            .panel(fill, RoundedCornerShape(Radius.pill.dp))
            .clickable(onClickLabel = description, onClick = onClick)
            .padding(horizontal = padding.dp, vertical = PaneInterior.pillVerticalPadding.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = ink, fontSize = (textSize ?: metrics.paneBody).sp, fontWeight = FontWeight.Bold, fontFamily = Faces.chrome, maxLines = 1)
    }
}

/** A square raised button with a glyph: the top bar's, the phone's back and menu. */
@Composable
fun SquareButton(icon: Icon, onClick: () -> Unit, description: String, modifier: Modifier = Modifier, tint: Color = LocalRoles.current.ink) {
    val metrics = LocalMetrics.current
    val roles = LocalRoles.current
    Box(
        modifier
            .size(metrics.controlHeight.dp)
            .raised(roles.face, Radius.control, Elevation.chip)
            .clickable(onClickLabel = description, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        IconView(icon, tint, metrics.controlHeight * Glyph.topBarToggle / Layout.topBarControlHeight)
    }
}
