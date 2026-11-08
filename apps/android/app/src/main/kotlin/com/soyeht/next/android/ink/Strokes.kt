package com.soyeht.next.android.ink

import kotlin.math.abs
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min

/** A point of a pen stroke: where, and when (the event's time in milliseconds). */
data class InkPoint(val x: Float, val y: Float, val time: Long)

/** The box a set of points lies in. */
data class Bounds(val left: Float, val top: Float, val right: Float, val bottom: Float) {
    val width get() = right - left
    val height get() = bottom - top

    fun intersects(other: Bounds) = left < other.right && other.left < right && top < other.bottom && other.top < bottom

    /** The share of this box's area that [other] covers. */
    fun coveredBy(other: Bounds): Float {
        val w = min(right, other.right) - max(left, other.left)
        val h = min(bottom, other.bottom) - max(top, other.top)
        if (w <= 0f || h <= 0f) return 0f
        val area = max(width, 1f) * max(height, 1f)
        return (w * h) / area
    }
}

/** One stroke: the pen down, its moves, and the pen up. */
class InkStroke(val points: List<InkPoint>) {
    val bounds: Bounds
        get() = Bounds(points.minOf { it.x }, points.minOf { it.y }, points.maxOf { it.x }, points.maxOf { it.y })

    val length: Float
        get() = points.zipWithNext { a, b -> hypot(b.x - a.x, b.y - a.y) }.sum()

    val duration: Long get() = points.last().time - points.first().time

    /** How many times the pen turned back along the x axis, ignoring moves shorter than [slop]. */
    fun horizontalReversals(slop: Float): Int {
        var direction = 0
        var reversals = 0
        var anchor = points.first().x
        for (point in points) {
            val delta = point.x - anchor
            if (abs(delta) < slop) continue
            val now = if (delta > 0) 1 else -1
            if (direction != 0 && now != direction) reversals += 1
            direction = now
            anchor = point.x
        }
        return reversals
    }
}

/**
 * What a stroke means, the way iPad Scribble's basics read it: a short touch is a tap, a back-and-forth scribble is a
 * scratch-out, anything else is writing. The sizes are in pixels the caller scales from dp.
 */
object StrokeShapes {
    /** A touch no longer than [slop] of travel and [maxMillis] of time is a tap. */
    fun isTap(stroke: InkStroke, slop: Float, maxMillis: Long = TAP_MILLIS) = stroke.length <= slop && stroke.duration <= maxMillis

    /** A scribble: wide for its height, and turning back at least [MIN_REVERSALS] times. */
    fun isScratchOut(stroke: InkStroke, unit: Float): Boolean {
        val bounds = stroke.bounds
        return bounds.width >= SCRATCH_WIDTH * unit &&
            bounds.height <= bounds.width &&
            stroke.horizontalReversals(slop = SCRATCH_SLOP * unit) >= MIN_REVERSALS
    }

    const val TAP_MILLIS = 300L
    const val MIN_REVERSALS = 3
    private const val SCRATCH_WIDTH = 6f
    private const val SCRATCH_SLOP = 1.5f
}
