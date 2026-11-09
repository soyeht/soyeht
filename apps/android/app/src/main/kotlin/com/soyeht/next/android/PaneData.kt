package com.soyeht.next.android

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import uniffi.soyeht_android.ComposerControls
import uniffi.soyeht_android.Conversation
import uniffi.soyeht_android.TerminalFrame
import uniffi.soyeht_android.UsageGauge

/** What the rail goes to: the agents page shows every pane of the workspace, the others one kind of pane. */
enum class Destination(val label: String) {
    Agents("Agents"),
    Editor("Editor"),
    Web("Web"),
    Terminal("Terminal"),
}

/** What one pane on screen shows of itself. Each part is its own state, so a terminal's output redraws that terminal alone. */
class PaneData {
    var terminal: TerminalFrame? by mutableStateOf(null)
    var conversation: Conversation? by mutableStateOf(null)

    /** The usage of the meter that reads the pane's harness. */
    var usage: UsageGauge? by mutableStateOf(null)

    /** The harness chip and the model, effort and permission menus of an agent pane. */
    var composer: ComposerControls? by mutableStateOf(null)
}
