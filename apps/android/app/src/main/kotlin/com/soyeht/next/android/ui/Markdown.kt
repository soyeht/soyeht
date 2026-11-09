package com.soyeht.next.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.*
import com.soyeht.next.android.theme.*
import kotlinx.coroutines.delay
import uniffi.soyeht_android.CodeCopy
import uniffi.soyeht_android.MarkdownBlock
import uniffi.soyeht_android.MarkdownRun

/** How a run list is dressed: the colors are palette roles the caller passes, never ones of this file's own. */
class RunLook(val accent: Color, val codeBackground: Color, val bold: Boolean = false)

/**
 * The text of a paragraph, heading or cell with its styles. A link carries its address and asks [open] to follow it;
 * the core keeps only `http`, `https` and `mailto` addresses, so nothing else can be here.
 */
fun annotate(runs: List<MarkdownRun>, look: RunLook, open: (String) -> Unit): AnnotatedString = buildAnnotatedString {
    for (run in runs) {
        val decorations = listOfNotNull(
            TextDecoration.LineThrough.takeIf { run.strike },
            TextDecoration.Underline.takeIf { run.link != null },
        )
        val style = SpanStyle(
            fontWeight = FontWeight.Bold.takeIf { run.bold || look.bold },
            fontStyle = FontStyle.Italic.takeIf { run.italic },
            fontFamily = FontFamily.Monospace.takeIf { run.code },
            background = if (run.code) look.codeBackground else Color.Unspecified,
            color = if (run.link != null) look.accent else Color.Unspecified,
            textDecoration = if (decorations.isEmpty()) null else TextDecoration.combine(decorations),
        )
        val address = run.link
        if (address == null) {
            withStyle(style) { append(run.text) }
        } else {
            val link = LinkAnnotation.Url(address, TextLinkStyles(style)) { open(address) }
            withLink(link) { append(run.text) }
        }
    }
}

/** An agent's reply, drawn flat: the blocks of the shared Markdown subset (ADR 0087) in the pane's palette roles. */
@Composable
fun MarkdownView(blocks: List<MarkdownBlock>, copy: CodeCopy, ink: Color, modifier: Modifier = Modifier) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(PaneInterior.compactSpacing.dp)) {
        blocks.forEach { Block(it, copy, ink) }
    }
}

@Composable
private fun Block(block: MarkdownBlock, copy: CodeCopy, ink: Color) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    // A rule and a quote's bar are the well's grey, which on e-ink is too near the page to show: they take the border.
    val line = roles.border ?: roles.well
    val uri = LocalUriHandler.current
    // A phone with nothing to open a link with must not lose the conversation over it.
    val open: (String) -> Unit = { address -> runCatching { uri.openUri(address) } }
    when (block) {
        is MarkdownBlock.Paragraph ->
            Text(annotate(block.runs, RunLook(roles.accent, roles.well), open), color = ink, fontSize = metrics.body.sp)
        // Headings are bold at body size (ADR 0087).
        is MarkdownBlock.Heading ->
            Text(annotate(block.runs, RunLook(roles.accent, roles.well, bold = true), open), color = ink, fontSize = metrics.body.sp)
        is MarkdownBlock.Code -> CodeBlock(block.text, copy, ink)
        is MarkdownBlock.Quote -> Row(Modifier.height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(PaneInterior.compactSpacing.dp)) {
            Box(Modifier.fillMaxHeight().width(PaneInterior.tightSpacing.dp).background(line, RoundedCornerShape(Radius.compact.dp)))
            MarkdownView(block.blocks, copy, ink, Modifier.weight(1f))
        }
        is MarkdownBlock.ItemList -> Column(verticalArrangement = Arrangement.spacedBy(PaneInterior.tightSpacing.dp)) {
            block.items.forEach { item ->
                Row(horizontalArrangement = Arrangement.spacedBy(PaneInterior.tightSpacing.dp)) {
                    Text(item.marker, color = roles.secondary, fontSize = metrics.body.sp, modifier = Modifier.widthIn(min = Space.x18.dp))
                    MarkdownView(item.blocks, copy, ink, Modifier.weight(1f))
                }
            }
        }
        is MarkdownBlock.Table -> Table(block, ink, open)
        MarkdownBlock.Rule -> Box(Modifier.fillMaxWidth().height(maxOf(PaneInterior.separatorWidth, metrics.borderWidth).dp).background(line))
    }
}

/** Code in a well, in the mono face, with a flat Copy button at the top right: it copies the text as the parser kept it. */
@Composable
private fun CodeBlock(text: String, copy: CodeCopy, ink: Color) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    val clipboard = LocalClipboardManager.current
    var copies by remember { mutableIntStateOf(0) }
    var copied by remember { mutableStateOf(false) }
    // A second press starts the wait again.
    LaunchedEffect(copies) {
        if (copied) {
            delay(copy.feedbackMillis.toLong())
            copied = false
        }
    }
    Row(
        Modifier.fillMaxWidth().panel(roles.well, RoundedCornerShape(PaneInterior.wellRadius.dp)),
        verticalAlignment = Alignment.Top,
    ) {
        Text(
            text,
            color = ink,
            fontFamily = FontFamily.Monospace,
            fontSize = metrics.terminalFont.sp,
            modifier = Modifier.weight(1f).padding(PaneInterior.rowSpacing.dp),
        )
        Box(
            Modifier
                .defaultMinSize(minWidth = metrics.touchTarget.dp, minHeight = metrics.touchTarget.dp)
                .semantics { contentDescription = if (copied) copy.copied else copy.label }
                .clickable {
                    clipboard.setText(AnnotatedString(text))
                    copied = true
                    copies += 1
                }
                .padding(horizontal = PaneInterior.compactSpacing.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                if (copied) copy.copied else copy.label,
                color = if (copied) roles.accent else roles.secondary,
                fontSize = metrics.small.sp,
            )
        }
    }
}

/** A table: its columns line up, and one wider than the screen scrolls sideways. */
@Composable
private fun Table(table: MarkdownBlock.Table, ink: Color, open: (String) -> Unit) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    val rows = listOf(table.header) + table.rows
    val columns = rows.maxOf { it.cells.size }
    Row(Modifier.horizontalScroll(rememberScrollState()).panel(roles.well, RoundedCornerShape(PaneInterior.wellRadius.dp))) {
        repeat(columns) { column ->
            Column(Modifier.width(IntrinsicSize.Max).padding(PaneInterior.rowSpacing.dp)) {
                rows.forEachIndexed { index, row ->
                    val runs = row.cells.getOrNull(column)?.runs.orEmpty()
                    Text(
                        annotate(runs, RunLook(roles.accent, roles.well, bold = index == 0), open),
                        color = ink,
                        fontSize = metrics.caption.sp,
                        modifier = Modifier.padding(vertical = PaneInterior.tightSpacing.dp),
                    )
                }
            }
        }
    }
}
