package com.soyeht.next.android

import android.app.Application
import android.os.Build
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import java.io.IOException
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import com.soyeht.next.android.ink.Handwriting
import com.soyeht.next.android.ink.StoredHandwritingChoice
import com.soyeht.next.android.scan.CameraProblem
import com.soyeht.next.android.theme.Eink
import uniffi.soyeht_android.ComposerControls
import uniffi.soyeht_android.Conversation
import uniffi.soyeht_android.Core
import uniffi.soyeht_android.CoreException
import uniffi.soyeht_android.HarnessChoices
import uniffi.soyeht_android.KeyPress
import uniffi.soyeht_android.Listener
import uniffi.soyeht_android.Link
import uniffi.soyeht_android.Overview
import uniffi.soyeht_android.PaneKind
import uniffi.soyeht_android.WorkspaceRow
import uniffi.soyeht_android.TerminalKey
import uniffi.soyeht_android.UsageGauge
import uniffi.soyeht_android.deviceName
import uniffi.soyeht_android.pairingOffer

/** What the core says went wrong, in words for the person. */
private fun CoreException.reason(): String = when (this) {
    is CoreException.PairAgain -> reason
    is CoreException.Failed -> reason
    is CoreException.Cancelled -> "Pairing was cancelled."
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

    /** Handwriting on the message field, below Android 14 (ADR 0157). */
    val handwriting = Handwriting(StoredHandwritingChoice(application))

    private val saved = credentials.load()
    private val display = DisplaySettings(application)

    /** The e-ink mode: the palette and sizes of a Boox's screen, no animation, and output batched into few redraws. */
    var eink by mutableStateOf(einkEnabled(display.saved(), Build.MANUFACTURER))
        private set

    var paired by mutableStateOf(saved != null)
        private set
    var pairing: Pairing by mutableStateOf(Pairing.Idle)
        private set
    var overview: Overview? by mutableStateOf(null)
        private set

    /** The usage ring of every provider the engine meters: the rings of the app rail. */
    var rings: List<UsageGauge> by mutableStateOf(emptyList())
        private set

    /** The workspace of the tab on screen; the first one until the person chooses. */
    var workspaceId: String? by mutableStateOf(null)
        private set

    /** What the rail shows of the workspace: the agents (every kind of pane), or one kind. */
    var destination by mutableStateOf(Destination.Agents)
        private set

    /** The rail is on screen (a large screen); the toggle of the top bar hides it. */
    var appBarOpen by mutableStateOf(true)
        private set

    /** The pane that has the keyboard and the outline: the one the person touched last. */
    var focusedPaneId: String? by mutableStateOf(null)
        private set

    /** A phone: the workspace the person went into from the list, or null at the list. */
    var phoneWorkspaceId: String? by mutableStateOf(null)
        private set

    /** The settings sheet: the display mode, and the computer this device controls. */
    var showingSettings by mutableStateOf(false)
        private set

    /** What each pane on screen shows of itself; a pane draws only what its own data changes. */
    private val data = HashMap<String, PaneData>()
    private var shown: Set<String> = emptySet()
    private var usageAsked = false

    /** The pane whose harness menu is open, kept current while it is open. */
    var harnessChoices: HarnessChoices? by mutableStateOf(null)
        private set
    private var choosingHarness: String? = null

    /** The camera is looking for the pairing code. */
    var scanning by mutableStateOf(false)
        private set

    /** Why the last thing the camera read is not a pairing link, in words for the person. */
    var scanProblem: String? by mutableStateOf(null)
        private set

    /** The camera cannot be used (refused, or it failed to start): the screen says where else to pair. */
    var cameraProblem: CameraProblem? by mutableStateOf(null)
        private set

    /** "Forget this computer" waits for the person to say yes: it cannot be undone from the phone. */
    var confirmingForget by mutableStateOf(false)
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
        saved?.let { connect(it) }
        handwriting.refresh()
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

    fun toggleEink() {
        eink = !eink
        display.save(eink)
    }

    /** At most one redraw is queued, however fast the core reports: a terminal can change a hundred times a second. */
    private fun refreshSoon() {
        if (pending.compareAndSet(false, true)) {
            main.postDelayed({
                pending.set(false)
                refresh()
            }, if (eink) Eink.redrawIntervalMs.toLong() else REDRAW_MS)
        }
    }

    private fun refresh() {
        val current = core.overview()
        overview = current
        if (!usageAsked && current.link == Link.Connected) {
            // The rail's rings read the providers' meters: asked for once the connection is up.
            usageAsked = true
            core.loadUsage()
        }
        rings = core.usageGauges()
        val rows = current.workspaces.flatMap { it.panes }
        if (workspaceId != null && current.workspaces.none { it.workspaceId == workspaceId }) workspaceId = null
        if (phoneWorkspaceId != null && current.workspaces.none { it.workspaceId == phoneWorkspaceId }) phoneWorkspaceId = null
        // A pane that closed on the computer is dropped: the board is where the person goes.
        if (rows.none { it.paneId == focusedPaneId }) focusedPaneId = null
        data.keys.retainAll(rows.map { it.paneId }.toSet())
        for (row in rows) {
            if (row.paneId !in shown) continue
            val pane = data(row.paneId)
            when (row.kind) {
                PaneKind.TERMINAL -> pane.terminal = core.terminal(row.paneId)
                PaneKind.AGENT -> {
                    pane.conversation = core.conversation(row.paneId)
                    pane.usage = core.usageGauge(row.paneId)
                    pane.composer = core.composer(row.paneId)
                }
                else -> Unit
            }
        }
        choosingHarness?.let { harnessChoices = core.harnessChoices(it) }
    }

    /** What a pane on screen shows of itself. */
    fun data(paneId: String): PaneData = data.getOrPut(paneId) { PaneData() }

    /** The panes the shell draws now: only they are read from the core, which is every redraw's work. */
    fun present(paneIds: Set<String>) {
        if (paneIds == shown) return
        shown = paneIds
        refresh()
    }

    /** The workspace on screen: the person's choice, else the first. */
    val workspace: WorkspaceRow?
        get() = overview?.workspaces?.let { all -> all.firstOrNull { it.workspaceId == workspaceId } ?: all.firstOrNull() }

    fun selectWorkspace(id: String) {
        workspaceId = id
        focusedPaneId = null
    }

    fun selectDestination(next: Destination) {
        destination = next
    }

    fun toggleAppBar() {
        appBarOpen = !appBarOpen
    }

    /** The phone goes into a workspace from the list, or back to the list with null. */
    fun enterWorkspace(id: String?) {
        phoneWorkspaceId = id
        if (id != null) selectWorkspace(id)
        focusedPaneId = null
    }

    fun openSettings() {
        showingSettings = true
    }

    fun closeSettings() {
        showingSettings = false
    }

    /** The person touched a pane: it takes the keyboard here and on the computer. */
    fun focus(paneId: String) {
        if (focusedPaneId == paneId) return
        focusedPaneId = paneId
        core.focus(paneId)
    }

    /**
     * Asks the person whether to pair with the computer a link names. The core's parser reads it, the
     * one every way in uses; null is a link it took, else the words for what it refused.
     */
    private fun ask(link: String): String? = try {
        val text = link.trim()
        pairing = Pairing.Confirm(text, pairingOffer(text).address, replaces = paired)
        null
    } catch (failure: CoreException) {
        failure.reason()
    }

    /** A pairing link reached the app (a tapped link). */
    fun offer(link: String) {
        ask(link)?.let { pairing = Pairing.Failed(it) }
    }

    /** The person asked to scan, and the camera is allowed. */
    fun startScanning() {
        cameraProblem = null
        scanProblem = null
        scanning = true
    }

    fun stopScanning() {
        scanning = false
        scanProblem = null
    }

    fun cameraRefused() {
        cameraProblem = CameraProblem.Denied
    }

    /** The camera did not start (none on this device, or it is in use): leave the scan and say so, never crash. */
    fun cameraFailed() {
        scanning = false
        scanProblem = null
        cameraProblem = CameraProblem.Failed
    }

    /** The camera read a code: a pairing link is asked about as a tapped one is, anything else is said and the camera keeps looking. */
    fun scanned(text: String) {
        val refusal = ask(text)
        scanProblem = refusal
        if (refusal == null) scanning = false
    }

    fun pair(link: String) {
        pairing = Pairing.Working(null, waiting = false)
        viewModelScope.launch(Dispatchers.IO) {
            val name = deviceName(Build.MANUFACTURER, Build.MODEL)
            try {
                val credential = core.pair(link.trim(), name)
                try {
                    credentials.save(credential)
                } catch (failure: IOException) {
                    // The computer allowed this phone but it cannot keep the access: say so.
                    main.post { pairing = Pairing.Failed("This phone could not save the access. Pair again.") }
                    return@launch
                }
                main.post {
                    paired = true
                    pairing = Pairing.Idle
                    connect(credential)
                }
            } catch (failure: CoreException.Cancelled) {
                // The person stopped it: nothing to tell.
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

    fun askToForget() {
        confirmingForget = true
    }

    fun keepComputer() {
        confirmingForget = false
    }

    /** "Forget this computer": the credential goes, and so does the connection. */
    fun forget() {
        confirmingForget = false
        core.disconnect()
        credentials.forget()
        paired = false
        overview = null
        rings = emptyList()
        workspaceId = null
        phoneWorkspaceId = null
        focusedPaneId = null
        showingSettings = false
        shown = emptySet()
        usageAsked = false
        data.clear()
        harnessChoices = null
        choosingHarness = null
    }

    /** The harness menu of a pane opens: it lists what the engine has now and is kept current while it is open. */
    fun openHarnessMenu(paneId: String) {
        choosingHarness = paneId
        core.loadHarnesses(false)
        harnessChoices = core.harnessChoices(paneId)
    }

    /** The menu closes; what it listed stays for the confirmation of a switch, which reads its words. */
    fun closeHarnessMenu() {
        choosingHarness = null
    }

    /** "Check agents again": the engine reads the machine's environment afresh. */
    fun rescanHarnesses() = core.loadHarnesses(true)

    /** Chooses a value of the model, effort or permission menu. */
    fun choose(paneId: String, configId: String, value: String) = core.chooseOption(paneId, configId, value)

    /** Restarts the pane on another harness, handing the conversation over. The core refuses a switch that may not start. */
    fun switchHarness(paneId: String, harnessId: String) = core.switchHarness(paneId, harnessId)

    fun type(paneId: String, text: String) = core.typeText(paneId, text)

    fun paste(paneId: String, text: String) = core.paste(paneId, text)

    fun press(paneId: String, key: TerminalKey, control: Boolean = false, alt: Boolean = false, shift: Boolean = false) =
        core.pressKey(paneId, KeyPress(key, control, alt, shift))

    fun prompt(paneId: String, text: String) {
        if (text.isNotBlank()) core.sendPrompt(paneId, text)
    }

    fun answer(paneId: String, requestId: String, optionId: String) = core.answerPermission(paneId, requestId, optionId)

    /** The phone stops being a client of the computer when the app's state goes. */
    override fun onCleared() {
        core.disconnect()
        handwriting.close()
    }

    private companion object {
        const val REDRAW_MS = 16L
    }
}
