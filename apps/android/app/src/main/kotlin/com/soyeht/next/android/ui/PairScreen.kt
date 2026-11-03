package com.soyeht.next.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.soyeht.next.android.AppModel
import com.soyeht.next.android.Pairing
import com.soyeht.next.android.theme.LocalRoles
import com.soyeht.next.android.theme.PaneInterior
import com.soyeht.next.android.theme.Radius
import com.soyeht.next.android.theme.TypeScale

@Composable
fun PairScreen(model: AppModel) {
    val roles = LocalRoles.current
    var link by rememberSaveable { mutableStateOf("") }
    Column(
        Modifier.padding(PaneInterior.phoneInset.dp),
        verticalArrangement = Arrangement.spacedBy(PaneInterior.stackSpacing.dp),
    ) {
        Text("Soyeht", color = roles.ink, fontSize = TypeScale.title.sp)
        Text(
            "Pair with the computer where Soyeht runs. On the computer choose “Pair phone”, then paste " +
                "the link here or open it on this phone. Both must be on the same tailnet.",
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
        Pill("Pair", { model.pair(link) }, primary = true)
    }
}

/** What a pairing asks and tells: the confirmation of a tapped link, the fingerprint, a failure. */
@Composable
fun PairingDialogs(model: AppModel) {
    val roles = LocalRoles.current
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
