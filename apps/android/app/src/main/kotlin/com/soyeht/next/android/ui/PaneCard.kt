package com.soyeht.next.android.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.soyeht.next.android.AppModel
import com.soyeht.next.android.eink.ScreenRefresh
import com.soyeht.next.android.theme.*
import uniffi.soyeht_android.PaneKind
import uniffi.soyeht_android.PaneRow

/**
 * One pane: a shell, the only raised surface, holding the pane's header, a rule and its flat interior. The focused pane
 * of a board has an accent outline; a tap anywhere on a pane focuses it, here and on the computer.
 */
@Composable
fun PaneCard(model: AppModel, row: PaneRow, outlined: Boolean, modifier: Modifier = Modifier) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    val focused = model.focusedPaneId == row.paneId
    // On e-ink the shell has an edge already: the focused pane's is twice as wide, in the accent.
    val width = maxOf(PaneInterior.separatorWidth, 2 * metrics.borderWidth)
    val outline = if (outlined && focused) Modifier.border(width.dp, roles.accent, RoundedCornerShape(PaneShell.cornerRadius.dp)) else Modifier
    Column(
        modifier
            .paneShell()
            .then(outline)
            .clickable(onClickLabel = row.name, indication = null, interactionSource = null) { model.focus(row.paneId) }
            .padding(PaneLayout.paneCardInset.dp)
            .padding(horizontal = PaneInterior.phoneInset.dp),
    ) {
        PaneHeader(model, row)
        Separator()
        val body = Modifier.weight(1f).fillMaxWidth()
        when (row.kind) {
            PaneKind.AGENT -> AgentBody(model, row, body)
            PaneKind.TERMINAL -> TerminalBody(model, row, focused, body)
            else -> OnComputerOnly(row, body)
        }
        Spacer(Modifier.height(PaneInterior.cardVerticalPadding.dp))
    }
}

/**
 * The header: the state dot, the name, what the pane runs in the mono face, and the state at the right in words
 * (the accent when the agent needs the person). A terminal says what it is doing, and on a Boox offers the panel refresh.
 */
@Composable
private fun PaneHeader(model: AppModel, row: PaneRow) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    val controls = model.data(row.paneId).composer
    val subtitle = when (row.kind) {
        PaneKind.AGENT -> listOfNotNull(controls?.harness?.title, controls?.model?.label, controls?.effort?.label).joinToString(" · ").lowercase()
        else -> row.status
    }
    Row(
        Modifier.fillMaxWidth().heightIn(min = (PaneLayout.headerHeight + 2 * PaneInterior.rowSpacing).dp).padding(vertical = PaneInterior.rowSpacing.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(PaneInterior.rowSpacing.dp),
    ) {
        StateDot(row.dot)
        Text(row.name, color = roles.ink, fontWeight = FontWeight.ExtraBold, fontSize = metrics.paneBody.sp, maxLines = 1)
        Text(
            subtitle,
            color = roles.secondary,
            fontFamily = Faces.mono,
            fontSize = metrics.paneMonoSmall.sp,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        if (row.kind == PaneKind.AGENT) {
            Text(
                row.stateTitle,
                color = if (row.needsYou) roles.accent else roles.secondary,
                fontWeight = FontWeight.SemiBold,
                fontSize = metrics.paneMonoSmall.sp,
                maxLines = 1,
            )
        } else if (row.kind == PaneKind.TERMINAL) {
            val view = LocalView.current
            val refresh = remember(view) { ScreenRefresh.forView(view) }
            if (metrics.eink && refresh.available) {
                Text(
                    "Refresh screen",
                    color = roles.accent,
                    fontWeight = FontWeight.SemiBold,
                    fontSize = metrics.paneMonoSmall.sp,
                    modifier = Modifier.clickable(onClickLabel = "Refresh screen", onClick = refresh::fullRefresh).padding(PaneInterior.tightSpacing.dp),
                )
            }
        }
    }
}

/** A web, editor or writing pane is shown on the computer only (as on the iPad). */
@Composable
private fun OnComputerOnly(row: PaneRow, modifier: Modifier) {
    val roles = LocalRoles.current
    Box(modifier, contentAlignment = Alignment.Center) {
        Text("This pane is shown on the computer.", color = roles.secondary, fontSize = LocalMetrics.current.paneBody.sp)
    }
}
