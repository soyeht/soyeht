package com.soyeht.next.android.ui

import android.graphics.Paint
import android.graphics.Typeface
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isAltPressed
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.utf16CodePoint
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.soyeht.next.android.AppModel
import com.soyeht.next.android.theme.LocalRoles
import com.soyeht.next.android.theme.PaneInterior
import com.soyeht.next.android.theme.resolve
import uniffi.soyeht_android.PaneRow
import uniffi.soyeht_android.TermColor

@Composable
fun ColumnScope.TerminalScreen(model: AppModel, pane: PaneRow) {
    val roles = LocalRoles.current
    TopBar(pane.name, pane.status, onBack = model::close)
    var control by remember { mutableStateOf(false) }
    Box(Modifier.weight(1f).fillMaxWidth().background(roles.well)) {
        TerminalView(model)
        KeyCatcher(model, control) { control = false }
    }
    KeyRow(model, control) { control = !control }
}

/** The grid the computer has, painted run by run. The phone never resizes it; a wide pane scrolls sideways. */
@Composable
private fun TerminalView(model: AppModel) {
    val roles = LocalRoles.current
    val frame = model.terminal ?: return
    val density = LocalDensity.current
    val paint = remember(density) {
        Paint(Paint.ANTI_ALIAS_FLAG).apply {
            typeface = Typeface.MONOSPACE
            textSize = with(density) { TEXT_SP.sp.toPx() }
        }
    }
    val cellWidth = paint.measureText("M")
    val metrics = paint.fontMetrics
    val cellHeight = metrics.descent - metrics.ascent
    val vertical = rememberScrollState()
    val horizontal = rememberScrollState()
    val cursorRow = frame.cursor?.row?.toInt() ?: (frame.rows.toInt() - 1)
    LaunchedEffect(cursorRow, frame.rows) {
        // Keep the line being typed in view: scroll down to it when it is below the screen.
        val bottom = ((cursorRow + 1) * cellHeight).toInt()
        if (bottom > vertical.value + vertical.viewportSize) vertical.scrollTo(bottom - vertical.viewportSize)
    }
    val palette = roles.palette
    Box(Modifier.fillMaxSize().verticalScroll(vertical).horizontalScroll(horizontal)) {
        Canvas(
            Modifier.size(
                with(density) { (cellWidth * frame.columns.toInt()).toDp() },
                with(density) { (cellHeight * frame.rows.toInt()).toDp() },
            ),
        ) {
            drawIntoCanvas { canvas ->
                frame.lines.forEachIndexed { row, runs ->
                    val top = row * cellHeight
                    runs.forEach { run ->
                        val left = run.column.toInt() * cellWidth
                        val background = resolve(run.background, palette, roles.ink, roles.well)
                        if (run.background !is TermColor.DefaultBackground) {
                            drawRect(background, Offset(left, top), Size(run.columns.toInt() * cellWidth, cellHeight))
                        }
                        paint.color = resolve(run.foreground, palette, roles.ink, roles.well).copy(alpha = if (run.dim) DIM else 1f).toArgb()
                        paint.isFakeBoldText = run.bold
                        paint.isUnderlineText = run.underline
                        paint.textSkewX = if (run.italic) ITALIC_SKEW else 0f
                        canvas.nativeCanvas.drawText(run.text, left, top - metrics.ascent, paint)
                    }
                }
            }
            frame.cursor?.let { cursor ->
                drawRect(
                    roles.accent.copy(alpha = CURSOR),
                    Offset(cursor.column.toInt() * cellWidth, cursor.row.toInt() * cellHeight),
                    Size(cellWidth, cellHeight),
                )
            }
        }
    }
}

/**
 * An invisible text field that holds the soft keyboard. What it receives is sent on at once and the field
 * stays empty, so the terminal is the only copy of what was typed.
 */
@Composable
private fun KeyCatcher(model: AppModel, control: Boolean, release: () -> Unit) {
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    BasicTextField(
        value = "",
        onValueChange = { typed ->
            if (typed.isEmpty()) return@BasicTextField
            if (control && typed.length == 1) {
                model.press(typed.lowercase(), control = true)
                release()
            } else if (typed.length > 1) {
                model.paste(typed)
            } else {
                model.type(typed)
            }
        },
        // A password-type field (no suggestions) makes the keyboard commit each key at once, with no composing text to reconcile.
        keyboardOptions = KeyboardOptions(
            keyboardType = KeyboardType.Password,
            imeAction = ImeAction.Send,
            autoCorrectEnabled = false,
        ),
        keyboardActions = KeyboardActions(onSend = { model.press("enter") }),
        modifier = Modifier
            .size(1.dp)
            .alpha(0f)
            .focusRequester(focus)
            .onPreviewKeyEvent { event ->
                if (event.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                val name = when (event.key) {
                    Key.Enter -> "enter"
                    Key.Backspace -> "backspace"
                    Key.Tab -> "tab"
                    Key.Escape -> "escape"
                    Key.DirectionUp -> "up"
                    Key.DirectionDown -> "down"
                    Key.DirectionLeft -> "left"
                    Key.DirectionRight -> "right"
                    else -> null
                }
                val typed = event.utf16CodePoint
                when {
                    name != null -> {
                        model.press(name, control = event.isCtrlPressed)
                        true
                    }
                    // A key that types a character (a hardware keyboard, or `adb shell input text`).
                    typed >= SPACE && !event.isCtrlPressed && !event.isAltPressed -> {
                        model.type(String(Character.toChars(typed)))
                        true
                    }
                    else -> false
                }
            },
    )
}

/** The keys a soft keyboard does not have: Esc, Tab, Ctrl, arrows, Backspace. */
@Composable
private fun KeyRow(model: AppModel, control: Boolean, toggleControl: () -> Unit) {
    val keys = listOf(
        "Esc" to "escape", "Tab" to "tab", "↑" to "up", "↓" to "down", "←" to "left", "→" to "right",
        "⌫" to "backspace", "⏎" to "enter",
    )
    Row(
        Modifier.fillMaxWidth().padding(PaneInterior.tightSpacing.dp),
        horizontalArrangement = Arrangement.spacedBy(PaneInterior.tightSpacing.dp),
    ) {
        val fit = Modifier.weight(1f)
        Pill(if (control) "Ctrl ●" else "Ctrl", toggleControl, fit, primary = control, padding = PaneInterior.tightSpacing)
        keys.forEach { (label, key) -> Pill(label, { model.press(key) }, fit, padding = PaneInterior.tightSpacing) }
    }
}

private const val SPACE = 0x20
private const val TEXT_SP = 12f
private const val DIM = 0.6f
private const val CURSOR = 0.5f
private const val ITALIC_SKEW = -0.2f
