package com.soyeht.next.android.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.*
import com.soyeht.next.android.AppModel
import com.soyeht.next.android.theme.*
import uniffi.soyeht_android.*

/**
 * The inside of an agent pane, flat as the approved interior is: the conversation (the person's messages in a
 * balloon, the agent's in plain text, tool calls as ruled rows), the question the agent waits on, and the composer.
 * What the transcript holds and what a question offers are the core's; this draws them.
 */
@Composable
fun AgentBody(model: AppModel, row: PaneRow, modifier: Modifier = Modifier) {
    val data = model.data(row.paneId)
    val conversation = data.conversation
    Column(modifier) {
        val list = rememberLazyListState()
        val count = conversation?.items?.size ?: 0
        val last = conversation?.items?.lastOrNull()
        LaunchedEffect(count, last?.revision) {
            if (count > 0) list.scrollToItem(count - 1)
        }
        LazyColumn(
            Modifier.weight(1f).fillMaxWidth(),
            state = list,
            flingBehavior = flingBehavior(),
            contentPadding = PaddingValues(vertical = PaneInterior.rowSpacing.dp),
            verticalArrangement = Arrangement.spacedBy(PaneInterior.stackSpacing.dp),
        ) {
            items(conversation?.items ?: emptyList(), key = { it.id }) { item ->
                if (item.kind == ItemKind.TOOL) ToolRow(item) else Message(item)
            }
            conversation?.notice?.let { notice ->
                item { Text(notice, color = LocalRoles.current.secondary, fontSize = LocalMetrics.current.paneCaption.sp) }
            }
        }
        conversation?.question?.let { question ->
            Question(question, row.name) { option -> model.answer(row.paneId, question.requestId, option) }
            Spacer(Modifier.height(PaneInterior.rowSpacing.dp))
        }
        MessageComposer(model, row, working = conversation?.working == true)
    }
}

@Composable
private fun Message(item: ConversationItem) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    val person = item.speaker == Speaker.PERSON
    Column(
        Modifier
            .fillMaxWidth()
            .then(if (person) Modifier.panel(roles.canvas, RoundedCornerShape(Radius.control.dp)).padding(PaneInterior.cardPadding.dp) else Modifier),
        verticalArrangement = Arrangement.spacedBy(PaneInterior.tightSpacing.dp),
    ) {
        if (person || item.fromAgent != null) {
            Text(
                (item.fromAgent?.let { "${it.uppercase()} → YOU" } ?: "YOU"),
                color = if (item.fromAgent != null) roles.accent else roles.secondary,
                fontFamily = Faces.mono,
                fontWeight = FontWeight.Medium,
                fontSize = metrics.paneMonoSmall.sp,
            )
        }
        if (item.kind == ItemKind.THOUGHT) {
            Text("Thinking", color = roles.muted, fontFamily = Faces.mono, fontSize = metrics.paneMonoSmall.sp)
        }
        val ink = if (item.kind == ItemKind.MESSAGE) roles.ink else roles.secondary
        if (item.speaker == Speaker.AGENT && (item.kind == ItemKind.MESSAGE || item.kind == ItemKind.THOUGHT)) {
            // Only an agent's replies and thoughts are Markdown (ADR 0087); what a person typed stays as typed.
            // The parse is kept until the message changes: a transcript that redraws does not parse what it has.
            val blocks = remember(item.id, item.revision, item.streaming) { markdownBlocks(item.text, item.streaming) }
            val copy = remember { codeCopy() }
            MarkdownView(blocks, copy, ink)
            if (item.streaming) Text("…", color = roles.muted, fontSize = metrics.paneBody.sp)
        } else {
            Text(item.text + if (item.streaming) " …" else "", color = ink, fontSize = metrics.paneBody.sp)
        }
    }
}

/** A tool call: its verb in bold, what it works on in the mono face, and how far it is at the end of the ruled row. */
@Composable
private fun ToolRow(item: ConversationItem) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    val verb = item.text.substringBefore(' ')
    val target = item.text.substringAfter(' ', "")
    Column {
        Separator()
        Row(
            Modifier.fillMaxWidth().padding(vertical = PaneInterior.rowVerticalPadding.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(PaneInterior.rowSpacing.dp),
        ) {
            Text(verb, color = roles.ink, fontWeight = FontWeight.Bold, fontSize = metrics.paneCaption.sp, maxLines = 1)
            Text(
                target,
                color = roles.secondary,
                fontFamily = Faces.mono,
                fontSize = metrics.paneMonoSmall.sp,
                maxLines = 1,
                modifier = Modifier.weight(1f),
            )
            val (mark, word, ink) = when (item.toolState) {
                ToolState.DONE -> Triple("✓", "done", roles.success)
                ToolState.FAILED -> Triple("✕", "failed", roles.danger)
                ToolState.RUNNING -> Triple("●", "running", roles.accent)
                else -> Triple("●", "waiting", roles.accent)
            }
            Text("$mark $word", color = ink, fontFamily = Faces.mono, fontSize = metrics.paneMonoSmall.sp, maxLines = 1)
        }
    }
}

/** What the agent asks to run: the command in a well, and the options as pills: the allowing one in ink, a refusal in red. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Question(question: PermissionQuestion, agent: String, answer: (String) -> Unit) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    Column(
        Modifier
            .fillMaxWidth()
            .panel(roles.canvas, RoundedCornerShape(Radius.control.dp))
            .padding(PaneInterior.cardPadding.dp),
        verticalArrangement = Arrangement.spacedBy(PaneInterior.rowSpacing.dp),
    ) {
        Text(question.title.ifBlank { "$agent asks" }, color = roles.ink, fontWeight = FontWeight.Bold, fontSize = metrics.paneBody.sp)
        if (question.detail.isNotBlank()) {
            Text(
                question.detail,
                color = roles.ink,
                fontFamily = Faces.mono,
                fontSize = metrics.paneMono.sp,
                modifier = Modifier.fillMaxWidth().panel(roles.well, RoundedCornerShape(PaneInterior.wellRadius.dp)).padding(PaneInterior.rowSpacing.dp),
            )
        }
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(PaneInterior.compactSpacing.dp),
            verticalArrangement = Arrangement.spacedBy(PaneInterior.compactSpacing.dp),
        ) {
            question.options.forEach { option ->
                val tone = when (option.kind) {
                    OptionKind.ALLOW -> Tone.Primary
                    OptionKind.ALLOW_ALWAYS -> Tone.Neutral
                    OptionKind.REJECT -> Tone.Danger
                }
                Pill(option.name, { answer(option.optionId) }, tone = tone)
            }
        }
    }
}

/**
 * The composer (the approved minimal one): the field, the mono line of menus under it, and the round send button in ink.
 * While a turn runs a message is queued for the agent, and the field says so.
 */
@Composable
private fun MessageComposer(model: AppModel, row: PaneRow, working: Boolean) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    var text by rememberSaveable(row.paneId) { mutableStateOf("") }
    val controls = model.data(row.paneId).composer
    val agent = controls?.harness?.title ?: "the agent"
    Column(
        Modifier
            .fillMaxWidth()
            .panel(roles.canvas, RoundedCornerShape(PaneInterior.composerRadius.dp))
            .padding(horizontal = PaneInterior.cardPadding.dp, vertical = PaneInterior.cardVerticalPadding.dp),
        verticalArrangement = Arrangement.spacedBy(PaneInterior.compactSpacing.dp),
    ) {
        BasicTextField(
            value = text,
            onValueChange = { text = it },
            textStyle = TextStyle(color = roles.ink, fontSize = metrics.paneBody.sp),
            cursorBrush = SolidColor(roles.accent),
            modifier = Modifier.fillMaxWidth().heightIn(min = PaneInterior.sendButtonSize.dp),
            decorationBox = { inner ->
                if (text.isEmpty()) {
                    Text(
                        if (working) "Queue a message for $agent…" else "Message $agent…",
                        color = roles.muted,
                        fontSize = metrics.paneBody.sp,
                    )
                }
                inner()
            },
        )
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (controls != null) {
                // A line longer than the room scrolls under the finger; the send button keeps its place.
                ComposerControlsLine(model, row.paneId, controls, Modifier.weight(1f).horizontalScroll(rememberScrollState()))
            } else {
                Spacer(Modifier.weight(1f))
            }
            SendButton { model.prompt(row.paneId, text); text = "" }
        }
    }
}

@Composable
private fun SendButton(onClick: () -> Unit) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    val diameter = maxOf(PaneInterior.sendButtonSize, metrics.controlHeight * PaneInterior.sendButtonSize / Layout.topBarControlHeight)
    Box(
        Modifier.size(diameter.dp).panel(roles.ink, CircleShape).clickable(onClickLabel = "Send", onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        IconView(Icon.Up, roles.face, diameter * (com.soyeht.next.android.theme.Composer.iconSize / PaneInterior.sendButtonSize))
    }
}
