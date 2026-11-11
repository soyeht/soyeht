package com.soyeht.next.android.ui

import com.soyeht.next.android.AppModel
import com.soyeht.next.android.theme.*
import uniffi.soyeht_android.Link
import uniffi.soyeht_android.PaneDot
import uniffi.soyeht_android.PaneRow
import uniffi.soyeht_android.WorkspaceRow
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.*
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.*

/**
 * The paired computer's workspaces. A wide screen (a tablet, in either orientation) shows the tabs, the rail and the
 * panes of the workspace side by side; a narrow one (a phone) shows the list of workspaces and, inside one, a pane at a
 * time. The e-ink mode takes the same layout, with its larger text and an edge for every raised surface.
 */
@Composable
fun Workspaces(model: AppModel) {
    BoxWithConstraints(Modifier.fillMaxSize()) {
        if (maxWidth.value >= Board.TABLET_WIDTH) TabletWorkspaces(model) else PhoneWorkspaces(model)
    }
}

@Composable
private fun TabletWorkspaces(model: AppModel) {
    Column(Modifier.fillMaxSize()) {
        WorkspaceTopBar(model)
        ConnectionNotice(model)
        Row(Modifier.weight(1f).fillMaxWidth()) {
            if (model.appBarOpen) AppRail(model)
            val panes = Board.panes(model.destination, model.workspace?.panes.orEmpty())
            PaneBoard(model, panes, Modifier.weight(1f).fillMaxHeight())
        }
    }
}

/** The link is not up, or the computer refused this device: said under the bar, with the way out. */
@Composable
fun ConnectionNotice(model: AppModel) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    val overview = model.overview
    val text = when (val link = overview?.link) {
        null, Link.Connecting -> "Connecting…"
        Link.Connected -> overview?.problem
        is Link.Disconnected -> "Reconnecting… ${link.reason}"
        is Link.AccessRefused -> "${link.reason} Forget this computer in Settings, then pair again."
    } ?: return
    Row(
        Modifier.padding(horizontal = PaneLayout.screenPadding.dp, vertical = PaneInterior.compactSpacing.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(PaneInterior.rowSpacing.dp),
    ) {
        StateDot(if (overview?.link is Link.AccessRefused) PaneDot.WARNING else PaneDot.SECONDARY)
        Text(text, color = roles.secondary, fontFamily = Faces.mono, fontSize = metrics.paneMonoSmall.sp, modifier = Modifier.weight(1f))
    }
}

@Composable
private fun PhoneWorkspaces(model: AppModel) {
    val inside = model.phoneWorkspaceId?.let { id -> model.overview?.workspaces?.firstOrNull { it.workspaceId == id } }
    if (inside == null) WorkspaceList(model) else PhoneWorkspace(model, inside)
}

@Composable
private fun WorkspaceList(model: AppModel) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = PaneLayout.screenPadding.dp, vertical = PaneInterior.rowSpacing.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                "Workspaces",
                color = roles.ink,
                fontFamily = Faces.chrome,
                fontWeight = FontWeight.Bold,
                fontSize = metrics.title.sp,
                modifier = Modifier.weight(1f),
            )
            SquareButton(Icon.More, model::openSettings, "Settings")
        }
        ConnectionNotice(model)
        val workspaces = model.overview?.workspaces.orEmpty()
        if (workspaces.isEmpty() && model.overview?.link == Link.Connected) {
            Text(
                "No workspaces. Open a folder on the computer to start here.",
                color = roles.secondary,
                fontSize = metrics.paneBody.sp,
                modifier = Modifier.padding(PaneLayout.screenPadding.dp),
            )
        }
        LazyColumn(
            Modifier.fillMaxSize(),
            flingBehavior = flingBehavior(),
            contentPadding = PaddingValues(PaneLayout.screenPadding.dp),
            verticalArrangement = Arrangement.spacedBy(PaneLayout.paneGap.dp),
        ) {
            items(workspaces, key = { it.workspaceId }) { workspace -> WorkspaceCard(workspace) { model.enterWorkspace(workspace.workspaceId) } }
        }
    }
}

@Composable
private fun WorkspaceCard(workspace: WorkspaceRow, onClick: () -> Unit) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    val waiting = workspace.panes.count { it.needsYou }
    Column(
        Modifier
            .fillMaxWidth()
            .raised(roles.face, Radius.card, Elevation.card)
            .clickable(onClickLabel = workspace.name, onClick = onClick)
            .padding(PaneInterior.phoneInset.dp),
        verticalArrangement = Arrangement.spacedBy(PaneInterior.compactSpacing.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(PaneInterior.compactSpacing.dp)) {
            IconView(Icon.Folder, roles.secondary, metrics.paneBody)
            Text(workspace.name, color = roles.ink, fontFamily = Faces.chrome, fontWeight = FontWeight.Bold, fontSize = metrics.subheading.sp, maxLines = 1, modifier = Modifier.weight(1f))
            IconView(Icon.Forward, roles.muted, metrics.paneBody)
        }
        Text(
            listOf("${workspace.panes.size} panes", "$waiting need you".takeIf { waiting > 0 }).filterNotNull().joinToString(" · "),
            color = if (waiting > 0) roles.accent else roles.secondary,
            fontFamily = Faces.mono,
            fontSize = metrics.paneMonoSmall.sp,
        )
        Row(horizontalArrangement = Arrangement.spacedBy(PaneInterior.compactSpacing.dp), verticalAlignment = Alignment.CenterVertically) {
            workspace.panes.forEach { StateDot(it.dot) }
        }
    }
}

/** One workspace on a phone: the back button, its name and the menu, the pane switch, and one pane at a time. */
@Composable
private fun PhoneWorkspace(model: AppModel, workspace: WorkspaceRow) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    val panes = workspace.panes
    val shown = panes.firstOrNull { it.paneId == model.focusedPaneId } ?: panes.firstOrNull()
    androidx.compose.runtime.LaunchedEffect(shown?.paneId) {
        model.present(setOfNotNull(shown?.paneId))
        shown?.let { model.focus(it.paneId) }
    }
    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = PaneLayout.screenPadding.dp, vertical = PaneInterior.rowSpacing.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(PaneInterior.rowSpacing.dp),
        ) {
            Row(
                Modifier
                    .heightIn(min = metrics.controlHeight.dp)
                    .raised(roles.face, Radius.pill, Elevation.pill)
                    .clickable(onClickLabel = "Workspaces") { model.enterWorkspace(null) }
                    .padding(horizontal = PaneInterior.rowSpacing.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                IconView(Icon.Back, roles.ink, metrics.paneBody)
                Text("Workspaces", color = roles.ink, fontFamily = Faces.chrome, fontWeight = FontWeight.Bold, fontSize = metrics.paneBody.sp, maxLines = 1)
            }
            Text(
                workspace.name,
                color = roles.ink,
                fontWeight = FontWeight.ExtraBold,
                fontSize = metrics.subheading.sp,
                maxLines = 1,
                modifier = Modifier.weight(1f),
            )
            SquareButton(Icon.More, model::openSettings, "Settings")
        }
        ConnectionNotice(model)
        PaneSwitch(model, panes, shown)
        if (shown == null) {
            Box(Modifier.fillMaxSize().padding(PaneLayout.screenPadding.dp), contentAlignment = Alignment.Center) {
                Text("No open panes. Open a pane on the computer to continue here.", color = roles.secondary, fontSize = metrics.paneBody.sp)
            }
        } else {
            PaneCard(model, shown, outlined = false, modifier = Modifier.weight(1f).fillMaxWidth().padding(horizontal = PaneLayout.screenPadding.dp, vertical = PaneInterior.rowSpacing.dp))
        }
    }
}

/** The segmented switch between a workspace's panes: the shown one sits on the face, with its state dot. */
@Composable
private fun PaneSwitch(model: AppModel, panes: List<PaneRow>, shown: PaneRow?) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    if (panes.size < 2) return
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = PaneLayout.screenPadding.dp, vertical = PaneInterior.compactSpacing.dp)
            .panel(roles.well, RoundedCornerShape(Radius.control.dp))
            .padding(PaneInterior.tightSpacing.dp),
    ) {
        panes.forEach { row ->
            val selected = row.paneId == shown?.paneId
            Row(
                Modifier
                    .weight(1f)
                    .heightIn(min = metrics.controlHeight.dp)
                    .then(if (selected) Modifier.panel(roles.face, RoundedCornerShape(Radius.compact.dp)) else Modifier)
                    .clickable(onClickLabel = row.name) { model.focus(row.paneId) },
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.Center,
            ) {
                if (selected) {
                    StateDot(row.dot)
                    Spacer(Modifier.width(PaneInterior.tightSpacing.dp))
                }
                Text(
                    row.name,
                    color = if (selected) roles.ink else roles.secondary,
                    fontWeight = if (selected) FontWeight.Bold else FontWeight.Medium,
                    fontSize = metrics.paneBody.sp,
                    maxLines = 1,
                )
            }
        }
    }
}
