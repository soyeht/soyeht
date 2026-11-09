package com.soyeht.next.android.eink

/**
 * What the panel does around a terminal's output. One changed frame (an echoed key) is left to the view's
 * usual mode. Two frames in a row are a stream: the fast mode, while it lasts. When the output stops, the fast
 * mode ends and one full refresh clears the ghosts it left.
 */
class OutputBurst(private val refresh: ScreenRefresh) {
    private var frames = 0
    private var fast = false

    /** The terminal drew a changed frame. */
    fun frame() {
        frames += 1
        if (frames >= STREAM_FRAMES && !fast) {
            fast = true
            refresh.fastMode()
        }
    }

    /** No frame came for the settle delay. */
    fun quiet() {
        if (fast) {
            refresh.textMode()
            refresh.fullRefresh()
        }
        frames = 0
        fast = false
    }

    private companion object {
        const val STREAM_FRAMES = 2
    }
}
