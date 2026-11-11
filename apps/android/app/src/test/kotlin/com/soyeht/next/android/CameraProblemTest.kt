package com.soyeht.next.android

import com.soyeht.next.android.scan.CameraProblem
import org.junit.Assert.assertTrue
import org.junit.Test

class CameraProblemTest {
    @Test
    fun everyCameraProblemSaysWhereElseToPair() {
        for (problem in CameraProblem.entries) {
            assertTrue(problem.name, problem.words.contains("pairing link"))
        }
    }

    @Test
    fun aCameraThatFailedToStartIsNotSaidToBeOffInTheSettings() {
        assertTrue(!CameraProblem.Failed.words.contains("settings"))
        assertTrue(CameraProblem.Denied.words.contains("settings"))
    }
}
