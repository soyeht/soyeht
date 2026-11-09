package com.soyeht.next.android

import com.soyeht.next.android.ui.Board
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class BoardTest {
    @Test
    fun a_portrait_screen_stacks_the_panes() {
        val plan = Board.plan(count = 2, width = 900f, screenHeight = 1300f, portrait = true)
        assertEquals(1, plan.columns)
        assertEquals(2, plan.rows)
        assertFalse(plan.tabs)
    }

    @Test
    fun a_landscape_screen_puts_panes_side_by_side() {
        val plan = Board.plan(count = 2, width = 1200f, screenHeight = 820f, portrait = false)
        assertEquals(2, plan.columns)
        assertEquals(1, plan.rows)
        assertFalse(plan.tabs)
    }

    @Test
    fun panes_that_do_not_fit_at_a_readable_size_become_tabs() {
        assertTrue(Board.plan(count = 5, width = 700f, screenHeight = 820f, portrait = false).tabs)
        assertTrue(Board.plan(count = 2, width = 400f, screenHeight = 620f, portrait = true).tabs)
    }

    @Test
    fun one_pane_takes_the_whole_board() {
        val plan = Board.plan(count = 1, width = 400f, screenHeight = 620f, portrait = true)
        assertEquals(1, plan.columns)
        assertEquals(1, plan.rows)
        assertFalse(plan.tabs)
    }

    @Test
    fun a_tab_beyond_the_grid_is_a_tab_not_a_smaller_pane() {
        // Three panes on a landscape screen with room for two columns and one row.
        assertTrue(Board.plan(count = 3, width = 800f, screenHeight = 520f, portrait = false).tabs)
    }
}
