package com.soyeht.next.android.ink

import java.util.Locale

/** Which language model to use: the app's (the system's) language first, then its base language, then English. */
object HandwritingLanguage {
    fun tags(locale: Locale): List<String> =
        listOf(locale.toLanguageTag(), locale.language, FALLBACK).filter { it.isNotBlank() && it != "und" }.distinct()

    const val FALLBACK = "en"
}
