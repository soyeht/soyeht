package com.soyeht.next.android.ink

/** What finishing a stroke did to the writing in progress. */
sealed interface InkOutcome {
    /** The stroke is part of the writing. */
    data object Written : InkOutcome

    /** A scribble over the writing took [removed] strokes away. */
    data class Erased(val removed: Int) : InkOutcome

    /** A scribble with nothing under it: the caller deletes the word before the cursor, as Scribble does. */
    data object DeleteWord : InkOutcome

    /** A tap: the writing ends. */
    data object Tapped : InkOutcome
}

/**
 * The strokes the person has written and the device has not recognized yet. [unit] is the size of one dp in pixels.
 * It holds no clock and no view: the caller says when the pen goes down and up, and decides when a pause is long
 * enough to recognize.
 */
class InkSession(private val unit: Float) {
    private val written = mutableListOf<InkStroke>()

    val strokes: List<InkStroke> get() = written

    val isEmpty get() = written.isEmpty()

    /** Takes a finished stroke and says what it was. */
    fun finish(stroke: InkStroke): InkOutcome {
        if (StrokeShapes.isTap(stroke, slop = TAP_SLOP * unit)) {
            // A touch beside the writing is a dot (the one of an i), and writing; elsewhere it is a tap, which ends it.
            return if (nearWriting(stroke)) InkOutcome.Written.also { written.add(stroke) } else InkOutcome.Tapped
        }
        if (StrokeShapes.isScratchOut(stroke, unit)) {
            val area = stroke.bounds
            val before = written.size
            written.removeAll { it.bounds.coveredBy(area) >= COVERED }
            val removed = before - written.size
            return if (removed > 0) InkOutcome.Erased(removed) else InkOutcome.DeleteWord
        }
        written.add(stroke)
        return InkOutcome.Written
    }

    /** The stroke is within [DOT_REACH] dp of the writing: over it, beside it, or above it. */
    private fun nearWriting(stroke: InkStroke): Boolean {
        if (written.isEmpty()) return false
        val at = stroke.bounds
        val reach = DOT_REACH * unit
        return written.any {
            val box = it.bounds
            at.left <= box.right + reach && at.right >= box.left - reach && at.top <= box.bottom + reach && at.bottom >= box.top - reach
        }
    }

    /** The writing is handed over (recognized or dropped): the session is empty again. */
    fun clear() = written.clear()

    private companion object {
        /** A stroke that moved less than this many dp is a tap. */
        const val TAP_SLOP = 6f

        /** How far from a stroke a short touch is still part of the writing: the dot of an i, the bar of a t. */
        const val DOT_REACH = 40f

        /** A scribble removes the strokes it covers at least this much of. */
        const val COVERED = 0.5f
    }
}
