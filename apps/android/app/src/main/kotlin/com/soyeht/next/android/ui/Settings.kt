package com.soyeht.next.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import com.soyeht.next.android.AppModel
import com.soyeht.next.android.ink.Handwriting
import com.soyeht.next.android.ink.ModelState
import java.util.Locale
import com.soyeht.next.android.theme.*
import androidx.compose.ui.unit.*

/**
 * The settings: the display mode, and, once paired, the way to another computer and to forget this one. It is a raised
 * card over the page and draws in one frame, so it does not fade on a panel that refreshes slowly.
 */
@Composable
fun SettingsSheet(model: AppModel) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    val startScan = rememberStartScan(model)
    Dialog(onDismissRequest = model::closeSettings, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Column(
            Modifier
                .widthIn(max = SheetWidth.dp)
                .fillMaxWidth()
                .padding(PaneLayout.screenPadding.dp)
                .panel(roles.face, RoundedCornerShape(Radius.card.dp))
                .padding(PaneInterior.phoneInset.dp),
            verticalArrangement = Arrangement.spacedBy(PaneInterior.stackSpacing.dp),
        ) {
            Text("Settings", color = roles.ink, fontFamily = Faces.chrome, fontWeight = FontWeight.Bold, fontSize = metrics.subheading.sp)
            SettingRow("E-ink display", "The palette and sizes of a Boox's screen, with no animation.") {
                Pill(if (model.eink) "On" else "Off", model::toggleEink, tone = if (model.eink) Tone.Primary else Tone.Neutral, description = "Switch the e-ink display")
            }
            if (!model.handwriting.platformHandles) HandwritingRow(model.handwriting)
            if (model.paired) {
                SettingRow("Another computer", "Pairing replaces the computer this device controls.") {
                    Pill("Pair another", { model.closeSettings(); startScan() })
                }
                if (model.cameraDenied) CameraDenied()
                SettingRow("This computer", "This device stops controlling it.") {
                    Pill("Forget", { model.closeSettings(); model.askToForget() }, tone = Tone.Danger, description = "Forget this computer")
                }
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) { Pill("Close", model::closeSettings, tone = Tone.Primary) }
        }
    }
}

private const val SheetWidth = 520f

/**
 * Handwriting with the pen (below Android 14, where the keyboard does it). It is off until the person turns it on, and
 * only then Install can download the language model; nothing is downloaded before. The row says what it uses and what
 * ML Kit sends to Google, and the state says where the model is.
 */
@Composable
private fun HandwritingRow(handwriting: Handwriting) {
    LaunchedEffect(Unit) { handwriting.refresh() }
    val language = Locale.getDefault().displayLanguage
    SettingRow(
        "Handwriting on this device",
        "Write with the pen in the message field, read on this device by Google ML Kit. ML Kit sends usage metrics to " +
            "Google (device model, system and app version, speed, events, errors, languages), never what you write. " +
            "Off, the pen works as a finger.",
    ) { Pill(if (handwriting.enabled) "On" else "Off", { handwriting.enable(!handwriting.enabled) }, tone = if (handwriting.enabled) Tone.Primary else Tone.Neutral, description = "Switch handwriting on this device") }
    if (!handwriting.enabled) return
    val (detail, action) = when (val state = handwriting.state) {
        ModelState.Checking -> "Looking for the $language model…" to null
        ModelState.NotInstalled -> "The $language model downloads once, when you press Install." to ("Install" to handwriting::install)
        ModelState.Downloading -> "Installing the $language model…" to ("Installing…" to {})
        ModelState.Installed -> "The $language model is installed." to ("Remove" to handwriting::remove)
        is ModelState.Failed -> "Could not ${state.kind} the $language model. Try again." to ("Retry" to handwriting::install)
        ModelState.NoModel -> "There is no handwriting model for $language." to null
    }
    SettingRow("Language model", detail) {
        action?.let { (label, run) -> Pill(label, run, tone = if (label == "Install") Tone.Primary else Tone.Neutral) }
    }
}

@Composable
private fun SettingRow(title: String, detail: String, control: @Composable () -> Unit) {
    val roles = LocalRoles.current
    val metrics = LocalMetrics.current
    Row(Modifier.fillMaxWidth(), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(PaneInterior.rowSpacing.dp)) {
        Column(Modifier.weight(1f)) {
            Text(title, color = roles.ink, fontWeight = FontWeight.Bold, fontSize = metrics.paneBody.sp)
            Text(detail, color = roles.secondary, fontSize = metrics.paneCaption.sp)
        }
        control()
    }
}
