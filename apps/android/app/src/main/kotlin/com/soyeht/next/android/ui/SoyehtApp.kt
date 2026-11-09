package com.soyeht.next.android.ui

import androidx.compose.ui.unit.dp
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import com.soyeht.next.android.AppModel
import com.soyeht.next.android.theme.LocalMetrics
import com.soyeht.next.android.theme.LocalRoles
import uniffi.soyeht_android.PaneKind

/** The three places the person can be: pairing, the list of panes, and one pane. */
@Composable
fun SoyehtApp(model: AppModel) {
    val roles = LocalRoles.current
    val column = LocalMetrics.current.readingColumn
    Box(Modifier.fillMaxSize().background(roles.screen).safeDrawingPadding()) {
        val pane = model.openPane
        BackHandler(enabled = pane != null && !model.scanning) { model.close() }
        BackHandler(enabled = model.scanning) { model.stopScanning() }
        // Lists, forms and conversations stay a readable width on a large screen; a terminal takes all of it.
        val terminal = model.paired && pane?.kind == PaneKind.TERMINAL && !model.scanning
        val width = if (column != null && !terminal) Modifier.widthIn(max = column.dp) else Modifier
        Column(Modifier.fillMaxSize(), horizontalAlignment = Alignment.CenterHorizontally) {
            Column(width.fillMaxWidth().weight(1f)) {
                when {
                    model.scanning -> ScanScreen(model)
                    !model.paired -> PairScreen(model)
                    pane == null -> HomeScreen(model)
                    pane.kind == PaneKind.TERMINAL -> TerminalScreen(model, pane)
                    pane.kind == PaneKind.AGENT -> AgentScreen(model, pane)
                    else -> OnComputerOnly(model, pane)
                }
            }
        }
        PairingDialogs(model)
    }
}
