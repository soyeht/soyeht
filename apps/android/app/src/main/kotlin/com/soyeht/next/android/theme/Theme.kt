package com.soyeht.next.android.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color

/** The palette roles as Compose colors. Views read these and never write a color of their own. */
class Roles(val palette: Palette) {
    val screen = Color(palette.screen)
    val canvas = Color(palette.canvas)
    val face = Color(palette.face)
    val hover = Color(palette.hover)
    val well = Color(palette.well)
    val ink = Color(palette.ink)
    val secondary = Color(palette.secondary)
    val muted = Color(palette.muted)
    val accent = Color(palette.accent)
    val onAccent = Color(palette.onAccent)
    val selection = Color(palette.selection)

    // The bands of a gauge are the palette's own red, green and yellow (the ANSI slots 1, 2 and 3).
    val danger = Color(palette.ansi[1])
    val success = Color(palette.ansi[2])
    val warning = Color(palette.ansi[3])
}

val LocalRoles = staticCompositionLocalOf { Roles(Palettes.milk) }

/** The first palette of the token file is the default, as on the other clients. */
@Composable
fun SoyehtTheme(palette: Palette = Palettes.all.first().second, content: @Composable () -> Unit) {
    val roles = Roles(palette)
    val scheme = lightColorScheme(
        primary = roles.accent,
        onPrimary = roles.onAccent,
        background = roles.screen,
        onBackground = roles.ink,
        surface = roles.face,
        onSurface = roles.ink,
        surfaceVariant = roles.well,
        onSurfaceVariant = roles.secondary,
    )
    CompositionLocalProvider(LocalRoles provides roles) {
        MaterialTheme(colorScheme = scheme, content = content)
    }
}
