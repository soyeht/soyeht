package com.soyeht.next.android.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.Column
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.foundation.background
import com.soyeht.next.android.AppModel
import com.soyeht.next.android.theme.LocalRoles
import uniffi.soyeht_android.PaneKind

/** The three places the person can be: pairing, the list of panes, and one pane. */
@Composable
fun SoyehtApp(model: AppModel) {
    val roles = LocalRoles.current
    Box(Modifier.fillMaxSize().background(roles.screen).safeDrawingPadding()) {
        val pane = model.openPane
        BackHandler(enabled = pane != null) { model.close() }
        Column(Modifier.fillMaxSize()) {
            when {
                !model.paired -> PairScreen(model)
                pane == null -> HomeScreen(model)
                pane.kind == PaneKind.TERMINAL -> TerminalScreen(model, pane)
                pane.kind == PaneKind.AGENT -> AgentScreen(model, pane)
                else -> OnComputerOnly(model, pane)
            }
        }
        PairingDialogs(model)
    }
}
