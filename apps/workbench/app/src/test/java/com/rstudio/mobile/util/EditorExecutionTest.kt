package com.rstudio.mobile.util

import org.junit.Assert.assertEquals
import org.junit.Test

class EditorExecutionTest {
    @Test
    fun selectedTextWins() {
        assertEquals("sum(x)", executionTarget("x <- 1\nsum(x)", 7, 13))
    }

    @Test
    fun collapsedSelectionRunsCurrentLine() {
        assertEquals("sum(x)", executionTarget("x <- 1\nsum(x)\nprint(x)", 10, 10))
    }

    @Test
    fun reverseSelectionIsSupported() {
        assertEquals("x <- 1", executionTarget("x <- 1\nsum(x)", 6, 0))
    }

    @Test
    fun blankLineFallsBackToWholeDocument() {
        assertEquals("x <- 1\n\nsum(x)", executionTarget("x <- 1\n\nsum(x)", 7, 7))
    }
}
