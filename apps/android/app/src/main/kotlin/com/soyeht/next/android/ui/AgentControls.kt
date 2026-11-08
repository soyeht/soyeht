package com.soyeht.next.android.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.*
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupPositionProvider
import androidx.compose.ui.window.PopupProperties
import com.soyeht.next.android.AppModel
import com.soyeht.next.android.theme.*
import uniffi.soyeht_android.ChoiceMenu
import uniffi.soyeht_android.ComposerControls
import uniffi.soyeht_android.HarnessChoice

/**
 * The line of mono words under the message field, as the approved composer draws it: the harness the pane runs
 * (choosing another restarts the pane on it and hands the conversation over), then the model, effort and permission
 * of the session, each opening its menu. What each menu lists and when it may open are the core's; this draws them.
 */
@Composable
fun ComposerControlsLine(model: AppModel, paneId: String, controls: ComposerControls, modifier: Modifier = Modifier) {
    var restart: HarnessChoice? by remember { mutableStateOf(null) }
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    val menus = listOfNotNull(controls.model, controls.effort, controls.permission)
    Row(modifier, verticalAlignment = Alignment.CenterVertically) {
        HarnessMenu(model, paneId, controls, onChoose = { restart = it })
        menus.forEachIndexed { index, menu ->
            Text(" · ", color = roles.muted, fontFamily = Faces.mono, fontSize = metrics.paneMonoSmall.sp)
            ChoiceMenuText(menu, enabled = !controls.configuring, last = index == menus.lastIndex) { value -> model.choose(paneId, menu.configId, value) }
        }
    }
    restart?.let { target -> RestartDialog(model, paneId, target) { restart = null } }
}

/** One mono word of the line, which opens [content] as a menu. */
@Composable
private fun MenuText(
    label: String,
    enabled: Boolean,
    content: @Composable ColumnScope.(close: () -> Unit) -> Unit,
    onOpen: () -> Unit = {},
    onClose: () -> Unit = {},
) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    var open by remember { mutableStateOf(false) }
    Box {
        Text(
            label,
            color = if (enabled) roles.ink else roles.muted,
            fontFamily = Faces.mono,
            fontWeight = FontWeight.Bold,
            fontSize = metrics.paneMonoSmall.sp,
            maxLines = 1,
            modifier = Modifier
                .defaultMinSize(minHeight = metrics.controlHeight.dp)
                .clickable(onClickLabel = label) { if (enabled) { onOpen(); open = true } }
                .wrapContentHeight(Alignment.CenterVertically),
        )
        val close = { open = false; onClose() }
        if (metrics.eink) {
            // Material's menu fades and scales in; on e-ink it appears in one frame, with an edge.
            if (open) {
                Popup(popupPositionProvider = MenuPosition, onDismissRequest = close, properties = PopupProperties(focusable = true)) {
                    Column(
                        Modifier.widthIn(min = MenuWidth.dp, max = MenuMaxWidth.dp).panel(roles.face, RoundedCornerShape(Radius.control.dp)).padding(vertical = PaneInterior.compactSpacing.dp),
                    ) { content(close) }
                }
            }
        } else {
            DropdownMenu(open, close, containerColor = roles.face) { content(close) }
        }
    }
}

@Composable
private fun ChoiceMenuText(menu: ChoiceMenu, enabled: Boolean, last: Boolean, choose: (String) -> Unit) {
    val roles = LocalRoles.current
    MenuText(menu.label + if (last) " ▾" else "", enabled, content = { close ->
        menu.choices.forEach { choice ->
            DropdownMenuItem(
                text = {
                    Text(
                        (if (choice.value == menu.current) "✓ " else "") + choice.name,
                        color = if (choice.value == menu.current) roles.accent else roles.ink,
                        fontSize = LocalMetrics.current.paneBody.sp,
                    )
                },
                onClick = { close(); choose(choice.value) },
            )
        }
    })
}

@Composable
private fun HarnessMenu(model: AppModel, paneId: String, controls: ComposerControls, onChoose: (HarnessChoice) -> Unit) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    val choices = model.harnessChoices
    var showMore by remember { mutableStateOf(false) }
    MenuText(
        (if (controls.harness.busy) "${controls.harness.title}…" else controls.harness.title) + if (controls.model == null) " ▾" else "",
        enabled = controls.harness.canSwitch,
        onOpen = { model.openHarnessMenu(paneId) },
        onClose = model::closeHarnessMenu,
        content = { close ->
            if (choices == null || (choices.sections.isEmpty() && choices.more.isEmpty())) {
                Text(
                    if (choices?.loaded == true) choices.noneText else choices?.loadingText.orEmpty(),
                    color = roles.secondary,
                    fontSize = metrics.paneCaption.sp,
                    modifier = Modifier.padding(PaneInterior.cardPadding.dp),
                )
            }
            choices?.sections?.forEach { section ->
                Text(
                    section.title,
                    color = roles.muted,
                    fontSize = metrics.small.sp,
                    modifier = Modifier.padding(horizontal = PaneInterior.cardPadding.dp, vertical = PaneInterior.tightSpacing.dp),
                )
                section.items.forEach { HarnessItem(it) { close(); onChoose(it) } }
            }
            if (choices != null && choices.more.isNotEmpty()) {
                DropdownMenuItem(
                    text = { Text(choices.moreTitle + if (showMore) " ▴" else " ▾", color = roles.secondary, fontSize = metrics.paneBody.sp) },
                    onClick = { showMore = !showMore },
                )
                if (showMore) choices.more.forEach { HarnessItem(it) { close(); onChoose(it) } }
            }
            DropdownMenuItem(
                text = { Text(choices?.rescanText.orEmpty(), color = roles.accent, fontSize = metrics.paneBody.sp) },
                onClick = { model.rescanHarnesses() },
            )
        },
    )
}

@Composable
private fun HarnessItem(item: HarnessChoice, onClick: () -> Unit) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    DropdownMenuItem(
        enabled = item.available,
        text = {
            Column {
                Text(item.name, color = if (item.available) roles.ink else roles.muted, fontSize = metrics.paneBody.sp, fontWeight = FontWeight.Medium)
                Text(item.status, color = roles.secondary, fontSize = metrics.small.sp)
            }
        },
        onClick = onClick,
    )
}

/** The switch restarts the conversation on another agent: it says so where the person confirms it. */
@Composable
private fun RestartDialog(model: AppModel, paneId: String, target: HarnessChoice, dismiss: () -> Unit) {
    val roles = LocalRoles.current
    AlertDialog(
        onDismissRequest = dismiss,
        containerColor = roles.face,
        modifier = dialogModifier(),
        shape = dialogShape,
        title = { Text(target.action + "?", color = roles.ink) },
        text = { Text(model.harnessChoices?.handover.orEmpty(), color = roles.secondary) },
        confirmButton = { TextButton({ dismiss(); model.switchHarness(paneId, target.id) }) { Text(target.action, color = roles.accent) } },
        dismissButton = { TextButton(dismiss) { Text(model.harnessChoices?.cancelText.orEmpty(), color = roles.secondary) } },
    )
}

private const val MenuWidth = 220f
private const val MenuMaxWidth = 420f

/** Under the word when the menu fits there, else over it: the line sits at the bottom of the pane. */
private object MenuPosition : PopupPositionProvider {
    override fun calculatePosition(anchorBounds: IntRect, windowSize: IntSize, layoutDirection: LayoutDirection, popupContentSize: IntSize): IntOffset {
        val below = anchorBounds.bottom
        val y = if (below + popupContentSize.height <= windowSize.height) below else anchorBounds.top - popupContentSize.height
        val x = anchorBounds.left.coerceIn(0, maxOf(0, windowSize.width - popupContentSize.width))
        return IntOffset(x, y.coerceAtLeast(0))
    }
}
