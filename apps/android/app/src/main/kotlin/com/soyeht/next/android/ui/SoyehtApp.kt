package com.soyeht.next.android.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.soyeht.next.android.AppModel
import com.soyeht.next.android.theme.LocalMetrics
import com.soyeht.next.android.theme.LocalRoles

/** The places the person can be: pairing, the camera, and the paired computer's workspaces and panes. */
@Composable
fun SoyehtApp(model: AppModel) {
    val roles = LocalRoles.current
    val column = LocalMetrics.current.readingColumn
    Box(Modifier.fillMaxSize().background(roles.canvas).safeDrawingPadding()) {
        BackHandler(enabled = model.scanning) { model.stopScanning() }
        BackHandler(enabled = model.paired && !model.scanning && model.phoneWorkspaceId != null) { model.enterWorkspace(null) }
        when {
            model.scanning -> Column(Modifier.fillMaxSize()) { ScanScreen(model) }
            !model.paired -> {
                // The form stays a readable width on a large screen.
                val width = if (column != null) Modifier.widthIn(max = column.dp) else Modifier
                Column(Modifier.fillMaxSize(), horizontalAlignment = Alignment.CenterHorizontally) {
                    Column(width.fillMaxWidth().weight(1f)) { PairScreen(model) }
                }
            }
            else -> Workspaces(model)
        }
        PairingDialogs(model)
        if (model.showingSettings) SettingsSheet(model)
    }
}
