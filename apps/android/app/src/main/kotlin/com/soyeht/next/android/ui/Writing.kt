package com.soyeht.next.android.ui

import androidx.compose.runtime.getValue
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.IntSize
import com.soyeht.next.android.eink.ScreenRefresh
import com.soyeht.next.android.ink.Handwriting
import com.soyeht.next.android.ink.InkOutcome
import com.soyeht.next.android.ink.InkPoint
import com.soyeht.next.android.ink.InkSession
import com.soyeht.next.android.ink.InkStroke
import com.soyeht.next.android.ink.TextEdits
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * The pen writing on the message field. A stylus touch on the field opens an ink area (the field grows to give room),
 * the strokes are drawn as they come, and after a pause they are recognized and inserted at the cursor. A scribble
 * over the writing takes it away, a scribble with nothing under it deletes the word before the cursor, and a tap
 * ends the writing. A finger is not read here: it scrolls and types as before. On a Boox the panel is in its fast
 * mode while the pen writes and takes one clean refresh when the writing ends (ADR 0155).
 */
class Writing(
    private val handwriting: Handwriting,
    private val refresh: ScreenRefresh,
    private val scope: CoroutineScope,
    unit: Float,
    private val read: () -> TextFieldValue,
    private val write: (TextFieldValue) -> Unit,
    private val focus: () -> Unit,
) {
    private val session = InkSession(unit)
    private var live = mutableListOf<InkPoint>()
    private var timer: Job? = null

    /** The ink area is open. */
    var active by mutableStateOf(false)
        private set

    /** Changes with every point, so the drawing redraws and nothing else does. */
    var drawn by mutableIntStateOf(0)
        private set

    /** The size of the writing surface in pixels. */
    var area = IntSize.Zero

    /**
     * Where the writing surface is in the window. The surface grows when the ink area opens, and the pen does not move
     * with it: the points are kept in window coordinates, which do not shift, and drawn relative to this.
     */
    var origin = Offset.Zero

    /** The pen writes: the model is installed and the platform does not do it itself. */
    val enabled get() = handwriting.ready

    /** The finished strokes and the one being drawn, for the drawing. */
    val paths: List<List<InkPoint>> get() = session.strokes.map { it.points } + listOfNotNull(live.takeIf { it.isNotEmpty() })

    /** A point of the surface in window coordinates. */
    fun point(local: Offset, time: Long) = InkPoint(local.x + origin.x, local.y + origin.y, time)

    fun down(point: InkPoint) {
        timer?.cancel()
        if (!active) {
            active = true
            refresh.fastMode()
        }
        live = mutableListOf(point)
        drawn += 1
    }

    fun move(point: InkPoint) {
        live.add(point)
        drawn += 1
    }

    fun up(point: InkPoint) {
        live.add(point)
        val stroke = InkStroke(live.toList())
        live = mutableListOf()
        drawn += 1
        when (session.finish(stroke)) {
            InkOutcome.Written -> waitThenRecognize()
            is InkOutcome.Erased -> waitThenRecognize()
            InkOutcome.DeleteWord -> {
                write(TextEdits.deleteWordBefore(read()))
                waitThenRecognize()
            }
            InkOutcome.Tapped -> scope.launch { recognizeNow(); end(tapped = true) }
        }
    }

    /** After a pause with the pen up the writing is recognized; a longer one closes the ink area. */
    private fun waitThenRecognize() {
        timer?.cancel()
        timer = scope.launch {
            delay(PAUSE_MILLIS)
            recognizeNow()
            delay(IDLE_MILLIS)
            end(tapped = false)
        }
    }

    private suspend fun recognizeNow() {
        val strokes = session.strokes.toList()
        if (strokes.isEmpty()) return
        val field = read()
        val text = handwriting.recognize(strokes, field.text.substring(0, field.selection.min), area.width.toFloat(), area.height.toFloat())
        session.clear()
        if (text != null) write(TextEdits.insert(read(), text))
        drawn += 1
    }

    private fun end(tapped: Boolean) {
        timer?.cancel()
        session.clear()
        active = false
        drawn += 1
        refresh.textMode()
        refresh.fullRefresh()
        // A tap on an empty ink area is a tap on the field: it takes the keyboard.
        if (tapped) focus()
    }
}

@OptIn(ExperimentalComposeUiApi::class)
/** Reads the stylus (and only the stylus) on this area as writing when [Writing.enabled]; touch goes on to the field. */
fun Modifier.stylusWriting(writing: Writing): Modifier = pointerInput(writing) {
    awaitPointerEventScope {
        while (true) {
            val first = awaitPointerEvent(PointerEventPass.Initial)
            val down = first.changes.firstOrNull { it.type == PointerType.Stylus && it.pressed && !it.previousPressed }
            if (down == null || !writing.enabled) continue
            down.consume()
            writing.down(writing.point(down.position, down.uptimeMillis))
            while (true) {
                val event = awaitPointerEvent(PointerEventPass.Initial)
                val change = event.changes.firstOrNull { it.id == down.id } ?: break
                change.consume()
                for (past in change.historical) writing.move(writing.point(past.position, past.uptimeMillis))
                if (change.pressed) {
                    writing.move(writing.point(change.position, change.uptimeMillis))
                } else {
                    writing.up(writing.point(change.position, change.uptimeMillis))
                    break
                }
            }
        }
    }
}

/** Draws the strokes over the content, in [color], [width] pixels wide. */
fun Modifier.inkStrokes(writing: Writing, color: Color, width: Float): Modifier = drawWithContent {
    drawContent()
    // Read so that a point redraws this layer and nothing above it.
    writing.drawn
    for (points in writing.paths) {
        if (points.size < 2) continue
        val path = Path().apply {
            moveTo(points.first().x - writing.origin.x, points.first().y - writing.origin.y)
            for (point in points.drop(1)) lineTo(point.x - writing.origin.x, point.y - writing.origin.y)
        }
        drawPath(path, color, style = Stroke(width, cap = StrokeCap.Round, join = StrokeJoin.Round))
    }
}

/** The least pause with the pen up before the writing is read, and how long the ink area waits for more. */
private const val PAUSE_MILLIS = 1200L
private const val IDLE_MILLIS = 4000L
