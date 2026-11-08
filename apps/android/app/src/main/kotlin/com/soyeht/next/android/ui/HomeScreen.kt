package com.soyeht.next.android.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.*
import com.soyeht.next.android.AppModel
import com.soyeht.next.android.theme.*
import uniffi.soyeht_android.*

private fun kindLabel(kind: PaneKind) = when (kind) {
    PaneKind.TERMINAL -> "Terminal"
    PaneKind.AGENT -> "Agent"
    PaneKind.WEB -> "Web"
    PaneKind.EDITOR -> "Editor"
    PaneKind.WRITING -> "Writing"
}

@Composable
fun HomeScreen(model: AppModel) {
    val roles = LocalRoles.current
    val overview = model.overview
    val link = overview?.link
    TopBar(
        title = "Workspaces",
        subtitle = when (link) {
            null, Link.Connecting -> "Connecting…"
            Link.Connected -> "Connected"
            is Link.Disconnected -> "Reconnecting… ${link.reason}"
            is Link.AccessRefused -> link.reason
        },
        actions = { Pill("Forget", model::askToForget, description = "Forget this computer") },
    )
    if (link is Link.AccessRefused) {
        Text(
            "This computer no longer accepts this phone. Forget it, then pair again.",
            color = roles.secondary,
            modifier = Modifier.padding(PaneInterior.phoneInset.dp),
        )
    }
    // A person who gets another computer pairs it from here: the scan replaces the one this phone controls,
    // after asking, as a tapped link does.
    val startScan = rememberStartScan(model)
    val paste = rememberPasteLink(model::offer)
    val hasCamera = rememberHasCamera()
    Row(
        Modifier.padding(horizontal = PaneInterior.phoneInset.dp),
        horizontalArrangement = Arrangement.spacedBy(PaneInterior.compactSpacing.dp),
    ) {
        if (hasCamera) Pill("Pair another computer", startScan) else Pill("Paste a pairing link", paste)
    }
    model.cameraProblem?.let { Box(Modifier.padding(PaneInterior.phoneInset.dp)) { CameraNotice(it) } }
    overview?.problem?.let {
        Text(it, color = roles.secondary, fontSize = TypeScale.caption.sp, modifier = Modifier.padding(horizontal = PaneInterior.phoneInset.dp))
    }
    LazyColumn(
        Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(PaneInterior.tightSpacing.dp),
    ) {
        overview?.workspaces?.forEach { workspace ->
            item(key = workspace.workspaceId) {
                Text(
                    workspace.name,
                    color = roles.secondary,
                    fontSize = TypeScale.caption.sp,
                    modifier = Modifier.padding(horizontal = PaneInterior.phoneInset.dp, vertical = PaneInterior.compactSpacing.dp),
                )
            }
            items(workspace.panes, key = { it.paneId }) { pane -> PaneItem(pane) { model.open(pane) } }
        }
    }
}

@Composable
private fun PaneItem(pane: PaneRow, onClick: () -> Unit) {
    val roles = LocalRoles.current
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = PaneInterior.touchTarget.dp)
            .clickable(onClick = onClick)
            .padding(horizontal = PaneInterior.phoneInset.dp, vertical = PaneInterior.rowVerticalPadding.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(PaneInterior.rowSpacing.dp),
    ) {
        Column(Modifier.weight(1f)) {
            Text(pane.name, color = roles.ink, fontSize = TypeScale.body.sp, maxLines = 1)
            Text(
                listOf(kindLabel(pane.kind), pane.status).filter { it.isNotBlank() }.joinToString(" · "),
                color = roles.secondary,
                fontSize = TypeScale.small.sp,
                maxLines = 1,
            )
        }
    }
}

/** A web, editor or writing pane is shown on the computer only (as on the iPad). */
@Composable
fun OnComputerOnly(model: AppModel, pane: PaneRow) {
    val roles = LocalRoles.current
    TopBar(pane.name, kindLabel(pane.kind), onBack = model::close)
    Text(
        "This pane is shown on the computer.",
        color = roles.secondary,
        modifier = Modifier.padding(PaneInterior.phoneInset.dp),
    )
}
