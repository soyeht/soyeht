package com.soyeht.next.android.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.soyeht.next.android.theme.LocalMetrics
import com.soyeht.next.android.theme.LocalRoles
import com.soyeht.next.android.theme.UsageRing
import uniffi.soyeht_android.GaugeBand
import uniffi.soyeht_android.UsageGauge

/**
 * The usage ring of a pane's header: a track, the arc of the used fraction in the color of its band, and the
 * percentage in the middle. What the arc, its band and its opacities are is the core's (the thresholds of
 * `x-clientPolicy`); this draws them. The track and the arc share one path, inset by half the widest line, so
 * neither is cut at the edge of the box.
 */
@Composable
fun UsageRingView(gauge: UsageGauge, modifier: Modifier = Modifier) {
    val roles = LocalRoles.current
    // The ring's fading is opacity, which on e-ink is a grey the page nearly hides: it draws at full ink.
    val faded = !LocalMetrics.current.eink
    val arc = when (gauge.band) {
        GaugeBand.SUCCESS -> roles.success
        GaugeBand.WARNING -> roles.warning
        GaugeBand.DANGER -> roles.danger
        null -> roles.muted
    }
    Box(
        modifier
            .size(UsageRing.diameter.dp)
            .alpha(if (faded) gauge.ringOpacity else 1f)
            .semantics { contentDescription = "${gauge.provider} usage ${gauge.percentage}" },
        contentAlignment = Alignment.Center,
    ) {
        Canvas(Modifier.size(UsageRing.diameter.dp)) {
            val track = UsageRing.trackWidth.dp.toPx()
            val width = UsageRing.arcWidth.dp.toPx()
            val inset = maxOf(track, width) / 2
            val side = size.minDimension - 2 * inset
            val corner = Offset(inset, inset)
            drawArc(roles.well, 0f, 360f, false, corner, Size(side, side), style = Stroke(track))
            gauge.fraction?.let { fraction ->
                drawArc(
                    arc.copy(alpha = if (faded) gauge.arcOpacity else 1f),
                    -90f,
                    (fraction * 360.0).toFloat(),
                    false,
                    corner,
                    Size(side, side),
                    style = Stroke(width, cap = StrokeCap.Round),
                )
            }
        }
        Text(gauge.percentage, color = roles.ink, fontSize = UsageRing.percentageSize.sp)
    }
}
