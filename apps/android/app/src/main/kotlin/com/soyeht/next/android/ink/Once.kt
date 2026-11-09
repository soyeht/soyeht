package com.soyeht.next.android.ink

/** Runs an action once: later calls do nothing. An action that throws did not run, and the next call tries it again. */
class Once {
    private var done = false

    @Synchronized
    fun run(action: () -> Unit) {
        if (done) return
        action()
        done = true
    }
}
