package com.soyeht.next.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
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
import com.soyeht.next.android.AppModel
import com.soyeht.next.android.theme.*
import uniffi.soyeht_android.ChoiceMenu
import uniffi.soyeht_android.ComposerControls
import uniffi.soyeht_android.HarnessChoice

/**
 * The row of menus under the message field: the harness the pane runs (choosing another restarts the
 * pane on it and hands the conversation over), then the model, effort and permission of the session.
 * What each menu lists and when it may open are the core's; this draws them.
 */
@Composable
fun AgentControls(model: AppModel, controls: ComposerControls) {
    var restart: HarnessChoice? by remember { mutableStateOf(null) }
    Row(
        Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState())
            .padding(horizontal = PaneInterior.compactSpacing.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(PaneInterior.tightSpacing.dp),
    ) {
        HarnessMenu(model, controls, onChoose = { restart = it })
        controls.model?.let { ChoiceChip(it, enabled = !controls.configuring) { value -> model.choose(it.configId, value) } }
        controls.effort?.let { ChoiceChip(it, enabled = !controls.configuring) { value -> model.choose(it.configId, value) } }
        controls.permission?.let { ChoiceChip(it, enabled = !controls.configuring) { value -> model.choose(it.configId, value) } }
    }
    restart?.let { target -> RestartDialog(model, target) { restart = null } }
}

/** A flat chip: the label and a chevron, which opens [content] as a menu. */
@Composable
private fun Chip(
    label: String,
    enabled: Boolean,
    content: @Composable ColumnScope.(close: () -> Unit) -> Unit,
    onOpen: () -> Unit = {},
    onClose: () -> Unit = {},
) {
    val roles = LocalRoles.current
    var open by remember { mutableStateOf(false) }
    Box {
        Pill(
            "$label ▾",
            { if (enabled) { onOpen(); open = true } },
            padding = PaneInterior.compactSpacing,
            description = label,
        )
        val close = { open = false; onClose() }
        DropdownMenu(open, close, containerColor = roles.face) { content(close) }
    }
}

@Composable
private fun ChoiceChip(menu: ChoiceMenu, enabled: Boolean, choose: (String) -> Unit) {
    val roles = LocalRoles.current
    Chip(menu.label, enabled, content = { close ->
        menu.choices.forEach { choice ->
            DropdownMenuItem(
                text = {
                    Text(
                        (if (choice.value == menu.current) "✓ " else "") + choice.name,
                        color = if (choice.value == menu.current) roles.accent else roles.ink,
                        fontSize = TypeScale.body.sp,
                    )
                },
                onClick = { close(); choose(choice.value) },
            )
        }
    })
}

@Composable
private fun HarnessMenu(model: AppModel, controls: ComposerControls, onChoose: (HarnessChoice) -> Unit) {
    val roles = LocalRoles.current
    val choices = model.harnessChoices
    var showMore by remember { mutableStateOf(false) }
    Chip(
        if (controls.harness.busy) "${controls.harness.title}…" else controls.harness.title,
        enabled = controls.harness.canSwitch,
        onOpen = model::openHarnessMenu,
        onClose = model::closeHarnessMenu,
        content = { close ->
            if (choices == null || (choices.sections.isEmpty() && choices.more.isEmpty())) {
                Text(
                    if (choices?.loaded == true) choices.noneText else choices?.loadingText.orEmpty(),
                    color = roles.secondary,
                    fontSize = TypeScale.caption.sp,
                    modifier = Modifier.padding(PaneInterior.cardPadding.dp),
                )
            }
            choices?.sections?.forEach { section ->
                Text(
                    section.title,
                    color = roles.muted,
                    fontSize = TypeScale.small.sp,
                    modifier = Modifier.padding(horizontal = PaneInterior.cardPadding.dp, vertical = PaneInterior.tightSpacing.dp),
                )
                section.items.forEach { HarnessItem(it) { close(); onChoose(it) } }
            }
            if (choices != null && choices.more.isNotEmpty()) {
                DropdownMenuItem(
                    text = { Text(choices.moreTitle + if (showMore) " ▴" else " ▾", color = roles.secondary, fontSize = TypeScale.body.sp) },
                    onClick = { showMore = !showMore },
                )
                if (showMore) choices.more.forEach { HarnessItem(it) { close(); onChoose(it) } }
            }
            DropdownMenuItem(
                text = { Text(choices?.rescanText.orEmpty(), color = roles.accent, fontSize = TypeScale.body.sp) },
                onClick = { model.rescanHarnesses() },
            )
        },
    )
}

@Composable
private fun HarnessItem(item: HarnessChoice, onClick: () -> Unit) {
    val roles = LocalRoles.current
    DropdownMenuItem(
        enabled = item.available,
        text = {
            Column {
                Text(item.name, color = if (item.available) roles.ink else roles.muted, fontSize = TypeScale.body.sp, fontWeight = FontWeight.Medium)
                Text(item.status, color = roles.secondary, fontSize = TypeScale.small.sp)
            }
        },
        onClick = onClick,
    )
}

/** The switch restarts the conversation on another agent: it says so where the person confirms it. */
@Composable
private fun RestartDialog(model: AppModel, target: HarnessChoice, dismiss: () -> Unit) {
    val roles = LocalRoles.current
    AlertDialog(
        onDismissRequest = dismiss,
        containerColor = roles.face,
        title = { Text(target.action + "?", color = roles.ink) },
        text = { Text(model.harnessChoices?.handover.orEmpty(), color = roles.secondary) },
        confirmButton = { TextButton({ dismiss(); model.switchHarness(target.id) }) { Text(target.action, color = roles.accent) } },
        dismissButton = { TextButton(dismiss) { Text(model.harnessChoices?.cancelText.orEmpty(), color = roles.secondary) } },
    )
}
