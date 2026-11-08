package com.soyeht.next.android.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.unit.dp

/**
 * The line glyphs of the designs (a 24-unit grid, drawn with a rounded 2-unit line), as path data. They are drawn in a
 * palette color by [IconView]; no image file carries a color of its own.
 */
enum class Icon(val path: String) {
    Apps("M3 3h7v7H3z M14 3h7v7h-7z M14 14h7v7h-7z M3 14h7v7H3z"),
    Agent("M6 8h12a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2z M12 8V4H8 M2 14h2 M20 14h2 M15 13v2 M9 13v2"),
    Editor("M10 12.5 8 15l2 2.5 M14 12.5l2 2.5-2 2.5 M14 2v4a2 2 0 0 0 2 2h4 M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7z"),
    Terminal("M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z M7 11l2-2-2-2 M11 13h4"),
    Web("M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20z M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20 M2 12h20"),
    Settings(
        "M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z M18.5 12A6.5 6.5 0 1 1 5.5 12 6.5 6.5 0 0 1 18.5 12z " +
            "M18.5 12h2.7 M16.6 16.6l1.9 1.9 M12 18.5v2.7 M7.4 16.6l-1.9 1.9 M5.5 12H2.8 M7.4 7.4 5.5 5.5 M12 5.5V2.8 M16.6 7.4l1.9-1.9",
    ),
    Folder("M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.7-.9L9.6 3.9A2 2 0 0 0 7.9 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2z"),
    More("M12 12h.01 M19 12h.01 M5 12h.01"),
    Back("m15 18-6-6 6-6"),
    Forward("m9 18 6-6-6-6"),
    Down("m6 9 6 6 6-6"),
    Up("M12 19V5 M5 12l7-7 7 7"),
    Stop("M7 7h10v10H7z"),
    Close("M18 6 6 18 M6 6l12 12"),
}

private val cache = HashMap<Icon, ImageVector>()

private fun vectorOf(icon: Icon): ImageVector = cache.getOrPut(icon) {
    val builder = ImageVector.Builder(icon.name, 24.dp, 24.dp, 24f, 24f)
    builder.addPath(
        PathParser().parsePathString(icon.path).toNodes(),
        stroke = SolidColor(Color.Black),
        strokeLineWidth = 2f,
        strokeLineCap = StrokeCap.Round,
        strokeLineJoin = StrokeJoin.Round,
    )
    builder.build()
}

/** An icon in the color of a palette role. */
@Composable
fun IconView(icon: Icon, color: Color, size: Float, modifier: Modifier = Modifier) {
    val vector = remember(icon) { vectorOf(icon) }
    Image(vector, contentDescription = null, modifier.size(size.dp), colorFilter = ColorFilter.tint(color))
}
