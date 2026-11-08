package com.soyeht.next.android.ui

import android.content.res.Configuration
import com.soyeht.next.android.AppModel
import uniffi.soyeht_android.PaneRow
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.*
import androidx.compose.ui.*
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.*
import com.soyeht.next.android.theme.*
import androidx.compose.material3.Text

/**
 * The panes of the workspace on screen, side by side or stacked as [Board.plan] says, or as tabs when they do not
 * fit. The panes it draws are the ones the core is read for (`AppModel.present`).
 */
@Composable
fun PaneBoard(model: AppModel, panes: List<PaneRow>, modifier: Modifier = Modifier) {
    val roles = LocalRoles.current
    BoxWithConstraints(modifier.padding(PaneLayout.gridPadding.dp)) {
        val configuration = LocalConfiguration.current
        val plan = Board.plan(panes.size, maxWidth.value, configuration.screenHeightDp.toFloat(), configuration.orientation == Configuration.ORIENTATION_PORTRAIT)
        // Tabs show the focused pane, else the first.
        val shown = if (plan.tabs) listOf(panes.firstOrNull { it.paneId == model.focusedPaneId } ?: Board.defaultFocus(panes) ?: panes.first()) else panes
        LaunchedEffect(shown.map { it.paneId }) { model.present(shown.map { it.paneId }.toSet()) }
        LaunchedEffect(panes.map { it.paneId }, model.focusedPaneId) {
            if (model.focusedPaneId == null) Board.defaultFocus(panes)?.let { model.focus(it.paneId) }
        }
        when {
            panes.isEmpty() -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text(
                    "No open panes. Open a pane on the computer to continue here.",
                    color = roles.secondary,
                    fontSize = LocalMetrics.current.paneBody.sp,
                    modifier = Modifier.padding(PaneInterior.phoneInset.dp),
                )
            }
            plan.tabs -> Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(PaneLayout.paneStackSpacing.dp)) {
                PaneTabs(model, panes, shown.first().paneId)
                PaneCard(model, shown.first(), outlined = false, modifier = Modifier.weight(1f).fillMaxWidth().padding(PaneLayout.paneStackSpacing.dp))
            }
            else -> Column(Modifier.fillMaxSize()) {
                panes.chunked(plan.columns).forEach { line ->
                    Row(Modifier.weight(1f).fillMaxWidth()) {
                        line.forEach { row ->
                            PaneCard(
                                model, row, outlined = panes.size > 1,
                                modifier = Modifier.weight(1f).fillMaxHeight().padding(PaneLayout.paneStackSpacing.dp),
                            )
                        }
                        // A short last line leaves its empty cells empty, so a pane keeps its size.
                        repeat(plan.columns - line.size) { Spacer(Modifier.weight(1f)) }
                    }
                }
            }
        }
    }
}

/** The tabs of the panes that did not fit: flat, the shown tab on the face of the palette. */
@Composable
private fun PaneTabs(model: AppModel, panes: List<PaneRow>, shown: String) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = PaneLayout.paneStackSpacing.dp),
        horizontalArrangement = Arrangement.spacedBy(PaneInterior.compactSpacing.dp),
    ) {
        panes.forEach { row ->
            val isShown = row.paneId == shown
            Row(
                Modifier
                    .heightIn(min = metrics.controlHeight.dp)
                    .panel(if (isShown) roles.face else roles.canvas, RoundedCornerShape(PaneInterior.cardRadius.dp))
                    .clickable(onClickLabel = row.name) { model.focus(row.paneId) }
                    .padding(horizontal = PaneInterior.phoneInset.dp, vertical = PaneInterior.tightSpacing.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(PaneInterior.tightSpacing.dp),
            ) {
                StateDot(row.dot)
                Text(
                    row.name,
                    color = if (isShown) roles.ink else roles.secondary,
                    fontWeight = if (isShown) FontWeight.ExtraBold else FontWeight.Medium,
                    fontSize = metrics.paneBody.sp,
                    maxLines = 1,
                )
            }
        }
    }
}
