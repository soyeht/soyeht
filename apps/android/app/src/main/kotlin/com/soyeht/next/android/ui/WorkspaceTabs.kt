package com.soyeht.next.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.*
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.*
import com.soyeht.next.android.theme.*
import com.soyeht.next.android.AppModel

/**
 * The bar over the panes of a wide screen: the button that shows or hides the rail, and a tab for each workspace of
 * the computer. The active tab is set into the page and takes the accent; the others are raised.
 */
@Composable
fun WorkspaceTopBar(model: AppModel, modifier: Modifier = Modifier) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    val active = model.workspace?.workspaceId
    Row(
        modifier.padding(horizontal = PaneLayout.screenPadding.dp, vertical = PaneInterior.rowSpacing.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(PaneInterior.rowSpacing.dp),
    ) {
        SquareButton(Icon.Apps, model::toggleAppBar, "Show or hide the app bar")
        Row(
            // Room for the shadow of a raised tab above and below it.
            Modifier.weight(1f).horizontalScroll(rememberScrollState()).padding(vertical = PaneInterior.compactSpacing.dp, horizontal = PaneInterior.compactSpacing.dp),
            horizontalArrangement = Arrangement.spacedBy(PaneInterior.rowSpacing.dp),
        ) {
            model.overview?.workspaces?.forEach { workspace ->
                val selected = workspace.workspaceId == active
                Row(
                    Modifier
                        .heightIn(min = metrics.controlHeight.dp)
                        .then(
                            if (selected) Modifier.panel(roles.well, RoundedCornerShape(Radius.pill.dp))
                            else Modifier.raised(roles.face, Radius.pill, Elevation.chip),
                        )
                        .clickable(onClickLabel = workspace.name) { model.selectWorkspace(workspace.workspaceId) }
                        .padding(horizontal = PaneInterior.phoneInset.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(PaneInterior.compactSpacing.dp),
                ) {
                    IconView(Icon.Folder, if (selected) roles.accent else roles.secondary, metrics.paneCaption)
                    Text(
                        workspace.name,
                        color = if (selected) roles.accent else roles.ink,
                        fontFamily = Faces.chrome,
                        fontWeight = if (selected) FontWeight.Bold else FontWeight.SemiBold,
                        fontSize = metrics.paneCaption.sp,
                        maxLines = 1,
                    )
                }
            }
        }
    }
}
