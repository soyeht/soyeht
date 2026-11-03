package com.soyeht.next.android

import android.app.Application
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import uniffi.soyeht_android.Conversation
import uniffi.soyeht_android.Core
import uniffi.soyeht_android.CoreException
import uniffi.soyeht_android.KeyPress
import uniffi.soyeht_android.Listener
import uniffi.soyeht_android.Overview
import uniffi.soyeht_android.PaneKind
import uniffi.soyeht_android.PaneRow
import uniffi.soyeht_android.TerminalFrame

/** What the core says went wrong, in words for the person. */
private fun CoreException.reason(): String = when (this) {
    is CoreException.PairAgain -> reason
    is CoreException.Failed -> reason
}

/** Where the person is in a pairing. */
sealed interface Pairing {
    data object Idle : Pairing

    /** A link reached the app from outside it: pairing replaces the computer this phone controls, so it asks first. */
    data class Confirm(val link: String, val address: String, val replaces: Boolean) : Pairing

    data class Working(val fingerprint: String?, val waiting: Boolean) : Pairing

    data class Failed(val message: String) : Pairing
}

/**
 * What the views read. The rules are the core's; this holds the screen the person is on, redraws
 * when the core says the replica changed, and forwards what the person does.
 */
class AppModel(application: Application) : AndroidViewModel(application) {
    private val credentials = CredentialStore(application)
    private val main = Handler(Looper.getMainLooper())
    private val pending = AtomicBoolean(false)

    var paired by mutableStateOf(credentials.load() != null)
        private set
    var pairing: Pairing by mutableStateOf(Pairing.Idle)
        private set
    var overview: Overview? by mutableStateOf(null)
        private set
    var openPane: PaneRow? by mutableStateOf(null)
        private set
    var terminal: TerminalFrame? by mutableStateOf(null)
        private set
    var conversation: Conversation? by mutableStateOf(null)
        private set

    private val core = Core(
        object : Listener {
            override fun changed() = refreshSoon()

            override fun fingerprint(fingerprint: String) {
                main.post { pairing = Pairing.Working(fingerprint, waiting = false) }
            }

            override fun waitingForApproval() {
                main.post {
                    val current = pairing as? Pairing.Working
                    pairing = Pairing.Working(current?.fingerprint, waiting = true)
                }
            }
        },
    )

    init {
        credentials.load()?.let { connect(it) }
    }

    private fun connect(credential: String) {
        try {
            core.connect(credential)
        } catch (failure: CoreException) {
            credentials.forget()
            paired = false
            pairing = Pairing.Failed(failure.reason())
        }
    }

    /** At most one redraw is queued, however fast the core reports: a terminal can change a hundred times a second. */
    private fun refreshSoon() {
        if (pending.compareAndSet(false, true)) {
            main.postDelayed({
                pending.set(false)
                refresh()
            }, REDRAW_MS)
        }
    }

    private fun refresh() {
        val current = core.overview()
        overview = current
        val pane = openPane ?: return
        if (current.workspaces.none { workspace -> workspace.panes.any { it.paneId == pane.paneId } }) {
            // The pane closed on the computer: the list is where the person goes.
            close()
            return
        }
        when (pane.kind) {
            PaneKind.TERMINAL -> terminal = core.terminal(pane.paneId)
            PaneKind.AGENT -> conversation = core.conversation(pane.paneId)
            else -> Unit
        }
    }

    /** A pairing link reached the app (a tapped link). */
    fun offer(link: String) {
        val address = Uri.parse(link).getQueryParameter("address") ?: "an unknown address"
        pairing = Pairing.Confirm(link, address, replaces = paired)
    }

    fun pair(link: String) {
        pairing = Pairing.Working(null, waiting = false)
        viewModelScope.launch(Dispatchers.IO) {
            val name = "${Build.MANUFACTURER} ${Build.MODEL}".take(NAME_LIMIT)
            try {
                val credential = core.pair(link.trim(), name)
                credentials.save(credential)
                main.post {
                    paired = true
                    pairing = Pairing.Idle
                    connect(credential)
                }
            } catch (failure: CoreException) {
                main.post { pairing = Pairing.Failed(failure.reason()) }
            }
        }
    }

    fun cancelPairing() {
        core.cancelPairing()
        pairing = Pairing.Idle
    }

    fun dismissPairing() {
        pairing = Pairing.Idle
    }

    /** "Forget this computer": the credential goes, and so does the connection. */
    fun forget() {
        core.disconnect()
        credentials.forget()
        paired = false
        overview = null
        close()
    }

    fun open(pane: PaneRow) {
        openPane = pane
        terminal = null
        conversation = null
        core.focus(pane.paneId)
        refresh()
    }

    fun close() {
        openPane = null
        terminal = null
        conversation = null
    }

    fun type(text: String) = openPane?.let { core.typeText(it.paneId, text) }

    fun paste(text: String) = openPane?.let { core.paste(it.paneId, text) }

    fun press(key: String, control: Boolean = false, alt: Boolean = false, shift: Boolean = false) {
        openPane?.let { core.pressKey(it.paneId, KeyPress(key, control, alt, shift)) }
    }

    fun prompt(text: String) {
        val pane = openPane ?: return
        if (text.isNotBlank()) core.sendPrompt(pane.paneId, text)
    }

    fun answer(requestId: String, optionId: String) {
        openPane?.let { core.answerPermission(it.paneId, requestId, optionId) }
    }

    private companion object {
        const val REDRAW_MS = 16L
        const val NAME_LIMIT = 64
    }
}
