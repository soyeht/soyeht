package com.soyeht.next.android.theme

import androidx.compose.runtime.staticCompositionLocalOf

/**
 * The sizes the views draw with. The standard ones are the type scale and pane metrics of the token
 * file; the e-ink ones are its `eink` section: bigger text, a larger touch target and an edge on
 * every surface, because a screen of sixteen greys shows an edge and not a shadow.
 */
class Metrics(
    val eink: Boolean,
    val small: Float,
    val caption: Float,
    val body: Float,
    val subheading: Float,
    val title: Float,
    val touchTarget: Float,
    /** The width of the edge of a card, well, pill or modal; zero where surfaces have none. */
    val borderWidth: Float,
    /** The terminal's font size: the base, and how large it may grow to fill the pane's width. */
    val terminalFont: Float,
    val terminalFontMaximum: Float,
    /** The widest a list, form or conversation gets on a large screen. */
    val readingColumn: Float?,
) {
    companion object {
        val standard = Metrics(
            eink = false,
            small = TypeScale.small,
            caption = TypeScale.caption,
            body = TypeScale.body,
            subheading = TypeScale.subheading,
            title = TypeScale.title,
            touchTarget = PaneInterior.touchTarget,
            borderWidth = 0f,
            terminalFont = PaneInterior.monoPointSize,
            terminalFontMaximum = PaneInterior.monoPointSize,
            readingColumn = null,
        )

        val eink = Metrics(
            eink = true,
            small = Eink.smallTextSize,
            caption = Eink.captionTextSize,
            body = Eink.bodyTextSize,
            subheading = Eink.subheadingTextSize,
            title = Eink.titleTextSize,
            touchTarget = Eink.touchTarget,
            borderWidth = Eink.borderWidth,
            terminalFont = Eink.terminalFontSize,
            terminalFontMaximum = Eink.terminalFontSizeMaximum,
            readingColumn = Eink.readingColumnWidth,
        )
    }
}

val LocalMetrics = staticCompositionLocalOf { Metrics.standard }
