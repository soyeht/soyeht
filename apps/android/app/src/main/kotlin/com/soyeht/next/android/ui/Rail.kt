package com.soyeht.next.android.ui

import com.soyeht.next.android.AppModel
import com.soyeht.next.android.Destination
import com.soyeht.next.android.theme.*
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.*
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.*

/**
 * The bar of apps at the left of a wide screen: what to see of the workspace (the agents, the editor, the web, the
 * terminals), then a usage ring for each provider the engine meters, and the settings at the bottom.
 */
@Composable
fun AppRail(model: AppModel) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    val waiting = model.overview?.workspaces?.any { workspace -> workspace.panes.any { it.needsYou } } == true
    Column(
        Modifier
            .width((metrics.railButton + 2 * PaneInterior.rowSpacing).dp)
            .fillMaxHeight()
            .padding(vertical = PaneInterior.stackSpacing.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(PaneInterior.stackSpacing.dp),
    ) {
        Column(
            Modifier.weight(1f).verticalScroll(rememberScrollState()),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(PaneInterior.stackSpacing.dp),
        ) {
            Destination.entries.forEach { destination ->
                RailButton(
                    icon = destination.icon(),
                    label = destination.label,
                    selected = model.destination == destination,
                    attention = destination == Destination.Agents && waiting,
                ) { model.selectDestination(destination) }
            }
            if (model.rings.isNotEmpty()) {
                Box(Modifier.width(Layout.railDividerWidth.dp).height(PaneInterior.separatorWidth.dp).background(roles.border ?: roles.shadowDark))
            }
            model.rings.forEach { gauge ->
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    UsageRingView(gauge, logo = true)
                    Text(gauge.percentage, color = roles.secondary, fontWeight = FontWeight.Bold, fontSize = maxOf(UsageRing.percentageSize, metrics.paneMonoSmall - 4).sp)
                }
            }
        }
        RailButton(Icon.Settings, "Settings", selected = false) { model.openSettings() }
    }
}

private fun Destination.icon() = when (this) {
    Destination.Agents -> Icon.Agent
    Destination.Editor -> Icon.Editor
    Destination.Web -> Icon.Web
    Destination.Terminal -> Icon.Terminal
}

@Composable
private fun RailButton(icon: Icon, label: String, selected: Boolean, attention: Boolean = false, onClick: () -> Unit) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    Box(
        Modifier
            .size(metrics.railButton.dp)
            .then(if (selected) Modifier.panel(roles.well, RoundedCornerShape(Radius.compact.dp)) else Modifier)
            .clickable(onClickLabel = label, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        IconView(icon, if (selected) roles.accent else roles.secondary, metrics.railButton * Glyph.rail / Layout.railButtonSize)
        if (attention) {
            Box(Modifier.align(Alignment.TopEnd).padding(PaneInterior.compactSpacing.dp).size(PaneLayout.headerStateDot.dp).background(roles.accent, CircleShape))
        }
    }
}
