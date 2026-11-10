package com.soyeht.next.android.ui

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isAltPressed
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.key.utf16CodePoint
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.*
import com.soyeht.next.android.AppModel
import com.soyeht.next.android.eink.ScreenRefresh
import com.soyeht.next.android.theme.LocalMetrics
import com.soyeht.next.android.theme.LocalRoles
import com.soyeht.next.android.theme.PaneInterior
import com.soyeht.next.android.theme.Radius
import uniffi.soyeht_android.PaneRow
import uniffi.soyeht_android.TerminalKey

/**
 * The inside of a terminal pane: the computer's screen, and under it the keys a soft keyboard does not have. The
 * pane that has the focus takes the keyboard. On e-ink its output also drives the panel's refresh (ADR 0155).
 */
@Composable
fun TerminalBody(model: AppModel, row: PaneRow, focused: Boolean, modifier: Modifier = Modifier) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    val view = LocalView.current
    val refresh = remember(view) { ScreenRefresh.forView(view) }
    val frame = model.data(row.paneId).terminal
    // The refresh follows the pane the person types in: two terminals would fight over the panel's mode.
    TerminalRefresh(if (focused) frame else null, refresh, metrics.eink && focused)
    var control by remember { mutableStateOf(false) }
    Column(modifier, verticalArrangement = Arrangement.spacedBy(PaneInterior.compactSpacing.dp)) {
        Box(Modifier.weight(1f).fillMaxWidth().panel(roles.terminalWell, RoundedCornerShape(Radius.compact.dp))) {
            frame?.let { TerminalGrid(it) }
            if (focused) KeyCatcher(model, row.paneId, control) { control = false }
        }
        if (focused) KeyRow(model, row.paneId, control) { control = !control }
    }
}

/**
 * An invisible text field that holds the soft keyboard. What it receives is sent on at once and the field
 * stays empty, so the terminal is the only copy of what was typed.
 */
@Composable
private fun KeyCatcher(model: AppModel, paneId: String, control: Boolean, release: () -> Unit) {
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    BasicTextField(
        value = "",
        onValueChange = { typed ->
            if (typed.isEmpty()) return@BasicTextField
            if (control && typed.length == 1) {
                model.press(paneId, TerminalKey.Character(typed.lowercase()), control = true)
                release()
            } else if (typed.length > 1) {
                model.paste(paneId, typed)
            } else {
                model.type(paneId, typed)
            }
        },
        // A password-type field (no suggestions) makes the keyboard commit each key at once, with no composing text to reconcile.
        keyboardOptions = KeyboardOptions(
            keyboardType = KeyboardType.Password,
            imeAction = ImeAction.Send,
            autoCorrectEnabled = false,
        ),
        keyboardActions = KeyboardActions(onSend = { model.press(paneId, TerminalKey.Enter) }),
        // The field is invisible, so its caret is too, and no brush means no blink: an idle terminal draws no frames.
        cursorBrush = SolidColor(Color.Unspecified),
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
                        model.press(paneId, named, control = event.isCtrlPressed)
                        true
                    }
                    // Control or Alt with a character is a key press, not text.
                    text != null && (event.isCtrlPressed || event.isAltPressed) -> {
                        model.press(paneId, TerminalKey.Character(text.lowercase()), control = event.isCtrlPressed, alt = event.isAltPressed)
                        true
                    }
                    // A key that types a character (a hardware keyboard, or `adb shell input text`).
                    text != null -> {
                        model.type(paneId, text)
                        true
                    }
                    else -> false
                }
            },
    )
}

/** The keys a soft keyboard does not have: Esc, Tab, Ctrl, arrows, Backspace. */
@Composable
private fun KeyRow(model: AppModel, paneId: String, control: Boolean, toggleControl: () -> Unit) {
    val keys = listOf(
        "Esc" to TerminalKey.Escape, "Tab" to TerminalKey.Tab, "↑" to TerminalKey.Up, "↓" to TerminalKey.Down,
        "←" to TerminalKey.Left, "→" to TerminalKey.Right, "⌫" to TerminalKey.Backspace, "⏎" to TerminalKey.Enter,
    )
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(PaneInterior.tightSpacing.dp),
    ) {
        val fit = Modifier.weight(1f)
        val size = LocalMetrics.current.paneMonoSmall
        Pill("Ctrl", toggleControl, fit, tone = if (control) Tone.Primary else Tone.Neutral, padding = 0f, textSize = size)
        keys.forEach { (label, key) -> Pill(label, { model.press(paneId, key) }, fit, padding = 0f, textSize = size) }
    }
}

private const val SPACE = 0x20
