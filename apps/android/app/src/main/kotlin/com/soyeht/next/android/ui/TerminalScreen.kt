package com.soyeht.next.android.ui

import android.graphics.Paint
import android.graphics.Typeface
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.*
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isAltPressed
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.key.utf16CodePoint
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.*
import com.soyeht.next.android.AppModel
import com.soyeht.next.android.theme.LocalRoles
import com.soyeht.next.android.theme.PaneInterior
import com.soyeht.next.android.theme.resolve
import uniffi.soyeht_android.PaneRow
import uniffi.soyeht_android.TermColor
import uniffi.soyeht_android.TerminalKey

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
            textSize = with(density) { PaneInterior.monoPointSize.sp.toPx() }
        }
    }
    // The monospace family in each style, so that bold and italic are the font's own.
    val faces = remember { (0..3).map { style -> Typeface.create(Typeface.MONOSPACE, style) } }
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
                        val foreground = resolve(run.foreground, palette, roles.ink, roles.well)
                        paint.color = foreground.copy(alpha = if (run.dim) frame.dimOpacity else 1f).toArgb()
                        paint.typeface = faces[(if (run.bold) Typeface.BOLD else 0) or (if (run.italic) Typeface.ITALIC else 0)]
                        paint.isUnderlineText = run.underline
                        canvas.nativeCanvas.drawText(run.text, left, top - metrics.ascent, paint)
                    }
                }
            }
            frame.cursor?.let { cursor ->
                // As the desktop draws a focused cursor: an accent block with the glyph in the on-accent color.
                val left = cursor.column.toInt() * cellWidth
                val top = cursor.row.toInt() * cellHeight
                drawRect(roles.accent, Offset(left, top), Size(cellWidth, cellHeight))
                drawIntoCanvas { canvas ->
                    paint.color = roles.onAccent.toArgb()
                    paint.typeface = faces[0]
                    paint.isUnderlineText = false
                    canvas.nativeCanvas.drawText(cursor.glyph, left, top - metrics.ascent, paint)
                }
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
                model.press(TerminalKey.Character(typed.lowercase()), control = true)
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
        keyboardActions = KeyboardActions(onSend = { model.press(TerminalKey.Enter) }),
        modifier = Modifier
            .size(1.dp)
            .alpha(0f)
            .focusRequester(focus)
            .onPreviewKeyEvent { event ->
                if (event.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                val named = when (event.key) {
                    Key.Enter -> TerminalKey.Enter
                    Key.Backspace -> TerminalKey.Backspace
                    Key.Tab -> TerminalKey.Tab
                    Key.Escape -> TerminalKey.Escape
                    Key.DirectionUp -> TerminalKey.Up
                    Key.DirectionDown -> TerminalKey.Down
                    Key.DirectionLeft -> TerminalKey.Left
                    Key.DirectionRight -> TerminalKey.Right
                    else -> null
                }
                val typed = event.utf16CodePoint
                val text = if (typed >= SPACE) String(Character.toChars(typed)) else null
                when {
                    named != null -> {
                        model.press(named, control = event.isCtrlPressed)
                        true
                    }
                    // Control or Alt with a character is a key press, not text.
                    text != null && (event.isCtrlPressed || event.isAltPressed) -> {
                        model.press(TerminalKey.Character(text.lowercase()), control = event.isCtrlPressed, alt = event.isAltPressed)
                        true
                    }
                    // A key that types a character (a hardware keyboard, or `adb shell input text`).
                    text != null -> {
                        model.type(text)
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
        "Esc" to TerminalKey.Escape, "Tab" to TerminalKey.Tab, "↑" to TerminalKey.Up, "↓" to TerminalKey.Down,
        "←" to TerminalKey.Left, "→" to TerminalKey.Right, "⌫" to TerminalKey.Backspace, "⏎" to TerminalKey.Enter,
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
