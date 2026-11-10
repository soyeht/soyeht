package com.soyeht.next.android.ink

import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue

/** The edits the writing makes to the message field: pure functions of the field's text and cursor. */
object TextEdits {
    /** [text] at the cursor (or over the selection), with a space between it and a word before it. */
    fun insert(value: TextFieldValue, text: String): TextFieldValue {
        val start = value.selection.min
        val before = value.text.substring(0, start)
        val glued = if (before.isNotEmpty() && !before.last().isWhitespace()) " $text" else text
        val inserted = value.text.replaceRange(start, value.selection.max, glued)
        return TextFieldValue(inserted, TextRange(start + glued.length))
    }

    /** The field without the word (and the space after it) before the cursor. */
    fun deleteWordBefore(value: TextFieldValue): TextFieldValue {
        val cursor = value.selection.min
        val before = value.text.substring(0, cursor).trimEnd()
        val keep = before.lastIndexOf(' ') + 1
        return TextFieldValue(value.text.removeRange(keep, cursor), TextRange(keep))
    }
}
