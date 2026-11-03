package com.soyeht.next.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.soyeht.next.android.theme.LocalRoles
import com.soyeht.next.android.theme.PaneInterior
import com.soyeht.next.android.theme.Radius
import com.soyeht.next.android.theme.TypeScale

/** The bar over a screen: a back button when there is somewhere to go back to, the title, and actions. */
@Composable
fun TopBar(title: String, subtitle: String? = null, onBack: (() -> Unit)? = null, actions: @Composable () -> Unit = {}) {
    val roles = LocalRoles.current
    Row(
        Modifier.fillMaxWidth().padding(horizontal = PaneInterior.phoneInset.dp, vertical = PaneInterior.compactSpacing.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(PaneInterior.compactSpacing.dp),
    ) {
        if (onBack != null) {
            Pill("‹", onBack, description = "Back")
        }
        Column(Modifier.weight(1f)) {
            Text(title, color = roles.ink, fontSize = TypeScale.subheading.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
            if (subtitle != null) {
                Text(subtitle, color = roles.secondary, fontSize = TypeScale.small.sp, maxLines = 1)
            }
        }
        actions()
    }
}

/** A flat, tappable label with a touch target of the size the tokens set. */
@Composable
fun Pill(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    primary: Boolean = false,
    description: String = label,
    padding: Float = PaneInterior.pillHorizontalPadding,
) {
    val roles = LocalRoles.current
    Box(
        modifier
            .defaultMinSize(minWidth = PaneInterior.touchTarget.dp, minHeight = PaneInterior.touchTarget.dp)
            .background(if (primary) roles.accent else roles.well, RoundedCornerShape(Radius.control.dp))
            .clickable(onClickLabel = description, onClick = onClick)
            .padding(horizontal = padding.dp, vertical = PaneInterior.pillVerticalPadding.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = if (primary) roles.onAccent else roles.ink, fontSize = TypeScale.body.sp)
    }
}
