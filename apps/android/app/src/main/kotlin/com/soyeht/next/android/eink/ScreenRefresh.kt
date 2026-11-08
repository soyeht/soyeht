package com.soyeht.next.android.eink

import android.os.Build
import android.view.View
import java.lang.reflect.InvocationTargetException

/**
 * Control of how an e-ink panel refreshes a view (ADR 0153). A Boox's firmware lets an app pick the waveform
 * a view is drawn with: a quick partial one that ghosts, or a slow full one that clears the ghosts.
 * Anywhere else there is nothing to control and every call does nothing.
 */
interface ScreenRefresh {
    /** The panel is one this adapter controls: the "Refresh screen" action is worth showing. */
    val available: Boolean

    /** The view's usual mode: partial, optimised for text pages, with the least ghosting for its speed. */
    fun textMode()

    /** Output is streaming: the fastest mode, which draws in black and white and leaves ghosts. */
    fun fastMode()

    /** One full-screen refresh in the clean mode: it flashes, and clears what the fast modes left. */
    fun fullRefresh()

    companion object {
        /** The adapter of [view] on [manufacturer]'s device: a Boox's, or one that does nothing. */
        fun forView(view: View, manufacturer: String = Build.MANUFACTURER): ScreenRefresh =
            if (manufacturer.equals(BOOX_MANUFACTURER, ignoreCase = true)) BooxRefresh.of(view) ?: NoRefresh else NoRefresh
    }
}

private const val BOOX_MANUFACTURER = "ONYX"

/** A device with no e-ink panel to control. */
object NoRefresh : ScreenRefresh {
    override val available = false

    override fun textMode() = Unit

    override fun fastMode() = Unit

    override fun fullRefresh() = Unit
}

/**
 * A Boox's firmware adds two public methods to `android.view.View` (they are in its SDK listing, so no
 * hidden API is used): `setDefaultUpdateMode(int)`, the waveform a view is drawn with, and `refreshScreen(int)`,
 * which redraws the screen once in a waveform. They are looked up by reflection, so the app has no
 * compile-time dependency on the vendor: a firmware without them gives [NoRefresh], and a call that fails
 * turns the adapter off rather than disturbing the app.
 *
 * The mode numbers are the firmware's `UI_*_MODE` constants (the `ViewUpdateHelper` class of its framework,
 * read from the Note Air 3 C firmware); they name the modes of the vendor's EPD documentation:
 * REGAL (optimised partial update for text), ANIMATION (the fast black-and-white mode) and GC (full 16-grey refresh).
 */
class BooxRefresh private constructor(
    private val view: View,
    private val setDefault: java.lang.reflect.Method,
    private val refresh: java.lang.reflect.Method,
) : ScreenRefresh {
    private var working = true

    override val available get() = working

    override fun textMode() = call(setDefault, REGAL)

    override fun fastMode() = call(setDefault, ANIMATION)

    override fun fullRefresh() = call(refresh, GC)

    private fun call(method: java.lang.reflect.Method, mode: Int) {
        if (!working) return
        try {
            method.invoke(view, mode)
        } catch (failure: InvocationTargetException) {
            // The firmware refused the call: it is not one this adapter can drive, so it stops trying.
            working = false
        } catch (failure: IllegalAccessException) {
            working = false
        }
    }

    companion object {
        const val REGAL = 6
        const val ANIMATION = 4
        const val GC = 98

        /** The adapter, or null when the firmware has no such methods. */
        fun of(view: View): BooxRefresh? = try {
            val int = Int::class.javaPrimitiveType
            BooxRefresh(
                view,
                View::class.java.getMethod("setDefaultUpdateMode", int),
                View::class.java.getMethod("refreshScreen", int),
            )
        } catch (missing: NoSuchMethodException) {
            null
        }
    }
}
