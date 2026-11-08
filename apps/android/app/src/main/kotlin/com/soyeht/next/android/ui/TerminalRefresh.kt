package com.soyeht.next.android.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import com.soyeht.next.android.eink.OutputBurst
import com.soyeht.next.android.eink.ScreenRefresh
import com.soyeht.next.android.theme.Eink
import kotlinx.coroutines.delay
import uniffi.soyeht_android.TerminalFrame

/**
 * Keeps the e-ink panel's refresh in step with a terminal's output (ADR 0153): the view draws in the text mode;
 * output that streams switches to the fast mode, and about [Eink.settleDelayMs] after the last changed frame the
 * fast mode ends and one full refresh clears the ghosts. Off e-ink, or off a Boox, it does nothing.
 */
@Composable
fun TerminalRefresh(frame: TerminalFrame?, refresh: ScreenRefresh, eink: Boolean) {
    val burst = remember(refresh) { OutputBurst(refresh) }
    DisposableEffect(refresh, eink) {
        if (eink) refresh.textMode()
        onDispose { if (eink) refresh.textMode() }
    }
    // A new frame restarts this: it reaches the delay's end only when no frame came for that long.
    LaunchedEffect(frame) {
        if (!eink || frame == null) return@LaunchedEffect
        burst.frame()
        delay(Eink.settleDelayMs.toLong())
        burst.quiet()
    }
}
