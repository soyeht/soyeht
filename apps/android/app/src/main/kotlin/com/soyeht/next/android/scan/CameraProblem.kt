package com.soyeht.next.android.scan

/** Why the camera cannot read the pairing code, with the words that say where else to pair. */
enum class CameraProblem(val words: String) {
    /** The person did not allow the camera. */
    Denied(
        "The camera is off for Soyeht. Allow it in the phone’s settings to scan, or open the pairing link on this phone.",
    ),

    /** The camera did not start: another app holds it, or it broke. */
    Failed("The camera could not start. Open the pairing link on this phone, or paste it here."),
}
