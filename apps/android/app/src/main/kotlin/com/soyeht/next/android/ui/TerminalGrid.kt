package com.soyeht.next.android.ui

import android.graphics.Paint
import android.graphics.Typeface
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.soyeht.next.android.theme.LocalMetrics
import com.soyeht.next.android.theme.LocalRoles
import com.soyeht.next.android.theme.Palette
import com.soyeht.next.android.theme.legible
import com.soyeht.next.android.theme.resolve
import kotlin.math.ceil
import uniffi.soyeht_android.TermColor
import uniffi.soyeht_android.TermCursor
import uniffi.soyeht_android.TermRun
import uniffi.soyeht_android.TerminalFrame

/** The runs of one row, equal when the runs are: a row that did not change is not drawn again. */
@Immutable
private class RowRuns(val runs: List<TermRun>) {
    override fun equals(other: Any?) = other is RowRuns && other.runs == runs

    override fun hashCode() = runs.hashCode()
}

/** Everything a row is painted with, so that a change of size or theme redraws every row and nothing else does. */
@Immutable
private data class GridStyle(
    val textSize: Float,
    val cellWidth: Float,
    val cellHeight: Float,
    val baseline: Float,
    val palette: Palette,
    val ink: Color,
    val page: Color,
    val ground: Color,
    val accent: Color,
    val onAccent: Color,
    val dimOpacity: Float,
    /** The ink of dimmed text where it is a lighter step and not an alpha; null elsewhere. */
    val dimInk: Color?,
    /** True colors are moved until they read on their ground. */
    val legible: Boolean,
)

/** The monospace family in each style, so that bold and italic are the font's own. */
private val faces = (0..3).map { style -> Typeface.create(Typeface.MONOSPACE, style) }

private fun paintOf(size: Float) = Paint(Paint.ANTI_ALIAS_FLAG).apply {
    typeface = Typeface.MONOSPACE
    textSize = size
}

/**
 * The grid the computer has, painted run by run. The phone never resizes it (ADR 0013): on e-ink the font grows
 * until the columns fill the pane's width, and a grid still wider than the pane scrolls sideways. Each row is a layer
 * of its own that is drawn again only when its runs change, so a line of output marks one row of the screen as
 * changed and not the whole text block.
 */
@Composable
fun TerminalGrid(frame: TerminalFrame) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    val density = LocalDensity.current
    val vertical = rememberScrollState()
    val horizontal = rememberScrollState()
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val basePx = with(density) { metrics.terminalFont.sp.toPx() }
        val maxPx = with(density) { metrics.terminalFontMaximum.sp.toPx() }
        val columns = frame.columns.toInt()
        val size = fitTextSize(columns, constraints.maxWidth.toFloat(), basePx, maxPx, paintOf(basePx).measureText("M"))
        val style = remember(size, roles, frame.dimOpacity) {
            val paint = paintOf(size)
            val font = paint.fontMetrics
            GridStyle(
                textSize = size,
                cellWidth = paint.measureText("M"),
                cellHeight = ceil(font.descent - font.ascent),
                baseline = -font.ascent,
                palette = roles.palette,
                ink = roles.ink,
                page = roles.screen,
                ground = roles.terminalWell,
                accent = roles.accent,
                onAccent = roles.onAccent,
                dimOpacity = frame.dimOpacity,
                dimInk = roles.dim,
                legible = roles.border != null,
            )
        }
        val cursorRow = frame.cursor?.row?.toInt() ?: (frame.rows.toInt() - 1)
        LaunchedEffect(cursorRow, frame.rows, style.cellHeight) {
            // Keep the line being typed in view: scroll down to it when it is below the screen.
            val bottom = ((cursorRow + 1) * style.cellHeight).toInt()
            if (bottom > vertical.value + vertical.viewportSize) vertical.scrollTo(bottom - vertical.viewportSize)
        }
        val fling = flingBehavior()
        Box(Modifier.fillMaxSize().verticalScroll(vertical, flingBehavior = fling).horizontalScroll(horizontal, flingBehavior = fling)) {
            val width = with(density) { (style.cellWidth * columns).toDp() }
            val height = with(density) { style.cellHeight.toDp() }
            Column(Modifier.width(width)) {
                frame.lines.forEachIndexed { row, runs ->
                    key(row) { TerminalRow(RowRuns(runs), style, width, height) }
                }
            }
            frame.cursor?.let { Cursor(it, style, height) }
        }
    }
}

@Composable
private fun TerminalRow(row: RowRuns, style: GridStyle, width: Dp, height: Dp) {
    val paint = remember(style.textSize) { paintOf(style.textSize) }
    Canvas(Modifier.size(width, height).graphicsLayer()) {
        drawIntoCanvas { canvas ->
            row.runs.forEach { run ->
                val left = run.column.toInt() * style.cellWidth
                val background = color(run.background, style, style.ground, style.ground)
                if (run.background !is TermColor.DefaultBackground) {
                    drawRect(background, Offset(left, 0f), Size(run.columns.toInt() * style.cellWidth, style.cellHeight))
                }
                var foreground = color(run.foreground, style, style.ink, style.ground)
                if (style.legible) foreground = legible(foreground, background, style.ink, style.page)
                val ink = when {
                    !run.dim -> foreground
                    // Dimmed text is a lighter step of ink where the palette has one, and a fainter ink elsewhere.
                    style.dimInk != null -> style.dimInk
                    else -> foreground.copy(alpha = style.dimOpacity)
                }
                paint.color = ink.toArgb()
                paint.typeface = faces[(if (run.bold) Typeface.BOLD else 0) or (if (run.italic) Typeface.ITALIC else 0)]
                paint.isUnderlineText = run.underline
                canvas.nativeCanvas.drawText(run.text, left, style.baseline, paint)
            }
        }
    }
}

private fun color(color: TermColor, style: GridStyle, foreground: Color, background: Color): Color =
    resolve(color, style.palette, foreground, background)

/** A static block, as the desktop draws a focused cursor: an accent cell with the glyph in the on-accent color. */
@Composable
private fun Cursor(cursor: TermCursor, style: GridStyle, height: Dp) {
    val paint = remember(style.textSize) { paintOf(style.textSize) }
    val width = with(LocalDensity.current) { style.cellWidth.toDp() }
    Canvas(
        Modifier
            .offset { IntOffset((cursor.column.toInt() * style.cellWidth).toInt(), (cursor.row.toInt() * style.cellHeight).toInt()) }
            .size(width, height)
            .graphicsLayer(),
    ) {
        drawRect(style.accent, Offset.Zero, Size(style.cellWidth, style.cellHeight))
        drawIntoCanvas { canvas ->
            paint.color = style.onAccent.toArgb()
            paint.typeface = faces[0]
            canvas.nativeCanvas.drawText(cursor.glyph, 0f, style.baseline, paint)
        }
    }
}
