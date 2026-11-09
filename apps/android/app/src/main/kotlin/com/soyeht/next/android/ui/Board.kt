package com.soyeht.next.android.ui

import com.soyeht.next.android.Destination
import kotlin.math.ceil
import kotlin.math.floor
import uniffi.soyeht_android.PaneKind
import uniffi.soyeht_android.PaneRow

/**
 * How a workspace's panes are arranged on a screen: as many as fit at a readable size, side by side on a landscape
 * screen and stacked on a portrait one, and, when they do not all fit, tabs with one pane shown (as the iPad does).
 * The arrangement is the same in the e-ink mode, which only draws larger text in the same places.
 */
object Board {
    /** The least room a pane gets, in dp: a conversation or a terminal below this is not usable. */
    const val MIN_PANE_WIDTH = 340f
    const val MIN_PANE_HEIGHT = 360f

    /** The screen is wide enough for the rail, the tabs and the panes together: a tablet. */
    const val TABLET_WIDTH = 600f

    /** The room the tabs and the status line take over the panes, in dp. */
    const val CHROME_HEIGHT = 120f

    /** Either [columns] by [rows] panes are shown, or, with [tabs], one pane at a time under a strip of tabs. */
    data class Plan(val columns: Int, val rows: Int, val tabs: Boolean)

    /**
     * [portrait] is the screen's orientation, not the board's shape: a keyboard shortens the board and must not turn
     * a portrait screen's stack into columns. The room to fit is the board's width and the screen's height under the
     * bar ([CHROME_HEIGHT]), likewise unshortened by the keyboard.
     */
    fun plan(count: Int, width: Float, screenHeight: Float, portrait: Boolean): Plan {
        val height = screenHeight - CHROME_HEIGHT
        if (count <= 1) return Plan(1, 1, tabs = false)
        val fitColumns = maxOf(1, floor(width / MIN_PANE_WIDTH).toInt())
        val fitRows = maxOf(1, floor(height / MIN_PANE_HEIGHT).toInt())
        // A landscape screen puts panes side by side first; a portrait one stacks them.
        val order = if (portrait) (1..fitColumns) else (minOf(count, fitColumns) downTo 1)
        for (columns in order) {
            val rows = ceil(count.toFloat() / columns).toInt()
            if (rows <= fitRows) return Plan(columns, rows, tabs = false)
        }
        return Plan(1, 1, tabs = true)
    }

    /** The pane that has the focus before the person touches one: the first agent, whose composer is the next thing to use. */
    fun defaultFocus(panes: List<PaneRow>): PaneRow? = panes.firstOrNull { it.kind == PaneKind.AGENT } ?: panes.firstOrNull()

    /** The panes the rail's destination shows: the agents page holds every pane, the others one kind. */
    fun panes(destination: Destination, all: List<PaneRow>): List<PaneRow> = when (destination) {
        Destination.Agents -> all
        Destination.Editor -> all.filter { it.kind == PaneKind.EDITOR || it.kind == PaneKind.WRITING }
        Destination.Web -> all.filter { it.kind == PaneKind.WEB }
        Destination.Terminal -> all.filter { it.kind == PaneKind.TERMINAL }
    }
}
