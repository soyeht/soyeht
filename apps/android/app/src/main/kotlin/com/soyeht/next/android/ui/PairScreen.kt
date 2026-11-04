package com.soyeht.next.android.ui

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.*
import androidx.core.content.ContextCompat
import com.soyeht.next.android.*
import com.soyeht.next.android.theme.*

@Composable
fun PairScreen(model: AppModel) {
    val roles = LocalRoles.current
    // Not saved with the state of the screen: the link is the pairing code.
    var link by remember { mutableStateOf("") }
    val context = LocalContext.current
    // The camera is asked for when the person taps Scan, never before.
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { allowed ->
        if (allowed) model.startScanning() else model.cameraRefused()
    }
    Column(
        Modifier.padding(PaneInterior.phoneInset.dp),
        verticalArrangement = Arrangement.spacedBy(PaneInterior.stackSpacing.dp),
    ) {
        Text("Soyeht", color = roles.ink, fontSize = TypeScale.title.sp)
        Text(
            "Pair with the computer where Soyeht runs. On the computer choose “Pair phone”, then scan the code, " +
                "paste the link here or open it on this phone. Both must be on the same tailnet.",
            color = roles.secondary,
            fontSize = TypeScale.body.sp,
        )
        BasicTextField(
            value = link,
            onValueChange = { link = it },
            textStyle = TextStyle(color = roles.ink, fontSize = TypeScale.caption.sp),
            cursorBrush = SolidColor(roles.accent),
            modifier = Modifier
                .fillMaxWidth()
                .background(roles.well, RoundedCornerShape(Radius.control.dp))
                .padding(PaneInterior.cardPadding.dp),
            decorationBox = { inner ->
                if (link.isEmpty()) Text("soyeht-next://pair?…", color = roles.muted, fontSize = TypeScale.caption.sp)
                inner()
            },
        )
        Row(horizontalArrangement = Arrangement.spacedBy(PaneInterior.compactSpacing.dp)) {
            Pill("Pair", { model.pair(link) }, primary = true)
            Pill("Scan QR code", {
                val allowed = ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) ==
                    PackageManager.PERMISSION_GRANTED
                if (allowed) model.startScanning() else permission.launch(Manifest.permission.CAMERA)
            })
        }
        if (model.cameraDenied) CameraDenied()
    }
}

/** What a pairing asks and tells: the confirmation of a tapped link, the fingerprint, a failure. */
@Composable
fun PairingDialogs(model: AppModel) {
    val roles = LocalRoles.current
    if (model.confirmingForget) {
        AlertDialog(
            onDismissRequest = model::keepComputer,
            containerColor = roles.face,
            title = { Text("Forget this computer?", color = roles.ink) },
            text = {
                Text(
                    "This phone stops controlling it. To use it again, pair again with a new link from the computer.",
                    color = roles.secondary,
                )
            },
            confirmButton = { TextButton(model::forget) { Text("Forget", color = roles.accent) } },
            dismissButton = { TextButton(model::keepComputer) { Text("Cancel", color = roles.secondary) } },
        )
    }
    when (val state = model.pairing) {
        Pairing.Idle -> Unit
        is Pairing.Confirm -> AlertDialog(
            onDismissRequest = model::dismissPairing,
            containerColor = roles.face,
            title = { Text("Pair with the computer at ${state.address}?", color = roles.ink) },
            text = {
                Text(
                    (if (state.replaces) "This phone will stop controlling the computer it is paired with now. " else "") +
                        "A paired phone controls that computer: it types in terminals and drives agents. " +
                        "Only continue if you opened this link on purpose.",
                    color = roles.secondary,
                )
            },
            confirmButton = { TextButton({ model.pair(state.link) }) { Text("Pair", color = roles.accent) } },
            dismissButton = { TextButton(model::dismissPairing) { Text("Cancel", color = roles.secondary) } },
        )
        is Pairing.Working -> AlertDialog(
            onDismissRequest = {},
            containerColor = roles.face,
            title = { Text(if (state.waiting) "Allow access on the computer" else "Connecting…", color = roles.ink) },
            text = {
                Text(
                    if (state.fingerprint != null) {
                        "Compare this fingerprint with the one the computer shows:\n\n" +
                            state.fingerprint.chunked(8).joinToString(" ")
                    } else {
                        "Reaching the computer."
                    },
                    color = roles.secondary,
                )
            },
            confirmButton = { TextButton(model::cancelPairing) { Text("Cancel", color = roles.accent) } },
        )
        is Pairing.Failed -> AlertDialog(
            onDismissRequest = model::dismissPairing,
            containerColor = roles.face,
            title = { Text("Could not pair", color = roles.ink) },
            text = { Text(state.message, color = roles.secondary) },
            confirmButton = { TextButton(model::dismissPairing) { Text("OK", color = roles.accent) } },
        )
    }
}
