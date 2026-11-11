package com.soyeht.next.android

import android.content.Context

/** Boox tablets, which draw on e-ink, report this manufacturer. */
private const val EINK_MANUFACTURER = "ONYX"

/**
 * Whether the e-ink mode is on: what the person chose, else on for a device whose maker draws on e-ink.
 * The choice is kept across launches, so a person who switched it off on a Boox is not switched back.
 */
fun einkEnabled(saved: Boolean?, manufacturer: String): Boolean =
    saved ?: manufacturer.equals(EINK_MANUFACTURER, ignoreCase = true)

/** The person's choice of the e-ink mode, in the app's private storage. */
class DisplaySettings(context: Context) {
    private val preferences = context.getSharedPreferences(FILE, Context.MODE_PRIVATE)

    /** What the person chose, or null while they have not. */
    fun saved(): Boolean? = if (preferences.contains(EINK)) preferences.getBoolean(EINK, false) else null

    fun save(eink: Boolean) {
        preferences.edit().putBoolean(EINK, eink).apply()
    }

    private companion object {
        const val FILE = "display"
        const val EINK = "eink"
    }
}
