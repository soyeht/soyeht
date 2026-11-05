package com.soyeht.next.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.*
import com.soyeht.next.android.AppModel
import com.soyeht.next.android.theme.*
import uniffi.soyeht_android.*

@Composable
fun ColumnScope.AgentScreen(model: AppModel, pane: PaneRow) {
    val roles = LocalRoles.current
    val conversation = model.conversation
    TopBar(pane.name, pane.status, onBack = model::close)
    val list = rememberLazyListState()
    val count = conversation?.items?.size ?: 0
    val last = conversation?.items?.lastOrNull()
    LaunchedEffect(count, last?.revision) {
        if (count > 0) list.scrollToItem(count - 1)
    }
    LazyColumn(
        Modifier.weight(1f).fillMaxWidth(),
        state = list,
        contentPadding = androidx.compose.foundation.layout.PaddingValues(PaneInterior.phoneInset.dp),
        verticalArrangement = Arrangement.spacedBy(PaneInterior.stackSpacing.dp),
    ) {
        items(conversation?.items ?: emptyList(), key = { it.id }) { item -> Message(item) }
        conversation?.notice?.let { notice ->
            item { Text(notice, color = roles.secondary, fontSize = TypeScale.caption.sp) }
        }
    }
    conversation?.question?.let { Question(it) { option -> model.answer(it.requestId, option) } }
    model.composer?.let { AgentControls(model, it) }
    MessageField(model, working = conversation?.working == true)
}

@Composable
private fun Message(item: ConversationItem) {
    val roles = LocalRoles.current
    val person = item.speaker == Speaker.PERSON
    Column(
        Modifier
            .fillMaxWidth()
            .then(if (person) Modifier.background(roles.well, RoundedCornerShape(Radius.control.dp)).padding(PaneInterior.cardPadding.dp) else Modifier),
    ) {
        if (item.kind == ItemKind.TOOL || item.kind == ItemKind.THOUGHT) {
            Text(
                (if (item.kind == ItemKind.TOOL) "Tool" else "Thinking"),
                color = roles.muted,
                fontSize = TypeScale.small.sp,
            )
        }
        val ink = if (item.kind == ItemKind.MESSAGE) roles.ink else roles.secondary
        if (item.speaker == Speaker.AGENT && (item.kind == ItemKind.MESSAGE || item.kind == ItemKind.THOUGHT)) {
            // Only an agent's replies and thoughts are Markdown (ADR 0087); what a person typed stays as typed.
            // The parse is kept until the message changes: a transcript that redraws does not parse what it has.
            val blocks = remember(item.id, item.revision, item.streaming) { markdownBlocks(item.text, item.streaming) }
            val copy = remember { codeCopy() }
            MarkdownView(blocks, copy, ink)
            if (item.streaming) Text("…", color = roles.muted, fontSize = TypeScale.body.sp)
        } else {
            Text(item.text + if (item.streaming) " …" else "", color = ink, fontSize = TypeScale.body.sp)
        }
    }
}

@Composable
private fun Question(question: PermissionQuestion, answer: (String) -> Unit) {
    val roles = LocalRoles.current
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = PaneInterior.phoneInset.dp)
            .background(roles.well, RoundedCornerShape(Radius.control.dp))
            .padding(PaneInterior.cardPadding.dp),
        verticalArrangement = Arrangement.spacedBy(PaneInterior.compactSpacing.dp),
    ) {
        Text(question.title, color = roles.ink, fontSize = TypeScale.body.sp)
        if (question.detail.isNotBlank()) Text(question.detail, color = roles.secondary, fontSize = TypeScale.caption.sp)
        Row(horizontalArrangement = Arrangement.spacedBy(PaneInterior.compactSpacing.dp)) {
            question.options.forEach { option -> Pill(option.name, { answer(option.optionId) }) }
        }
    }
}

@Composable
private fun MessageField(model: AppModel, working: Boolean) {
    val roles = LocalRoles.current
    var text by rememberSaveable { mutableStateOf("") }
    Row(
        Modifier.fillMaxWidth().padding(PaneInterior.compactSpacing.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(PaneInterior.compactSpacing.dp),
    ) {
        BasicTextField(
            value = text,
            onValueChange = { text = it },
            textStyle = TextStyle(color = roles.ink, fontSize = TypeScale.body.sp),
            cursorBrush = SolidColor(roles.accent),
            modifier = Modifier
                .weight(1f)
                .background(roles.well, RoundedCornerShape(PaneInterior.composerRadius.dp))
                .padding(PaneInterior.cardPadding.dp),
            decorationBox = { inner ->
                if (text.isEmpty()) Text(if (working) "Working…" else "Message the agent", color = roles.muted, fontSize = TypeScale.body.sp)
                inner()
            },
        )
        Pill("Send", { model.prompt(text); text = "" }, primary = true)
    }
}
