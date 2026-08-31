package com.rstudio.mobile.util

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.text.font.FontWeight
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class AnsiParserTest {
    @Test
    fun compoundStylesUseRenderedOffsetsAndResetIndependently() {
        val parsed = AnsiParser.parse("plain \u001B[1;31merror\u001B[22;39m done", AnsiPalette.light)

        assertEquals("plain error done", parsed.text)
        assertEquals(1, parsed.spanStyles.size)
        val error = parsed.spanStyles.single()
        assertEquals(6, error.start)
        assertEquals(11, error.end)
        assertEquals(AnsiPalette.light.normal[1], error.item.color)
        assertEquals(FontWeight.Bold, error.item.fontWeight)
    }

    @Test
    fun styleContinuesAcrossLinesUntilReset() {
        val lines = AnsiParser.parseLines("\u001B[94mfirst\nsecond\u001B[0m end", AnsiPalette.dark)

        assertEquals(listOf("first", "second end"), lines.map { it.text })
        assertEquals(AnsiPalette.dark.bright[4], lines[0].spanStyles.single().item.color)
        assertEquals(AnsiPalette.dark.bright[4], lines[1].spanStyles.single().item.color)
        assertEquals(0, lines[1].spanStyles.single().start)
        assertEquals(6, lines[1].spanStyles.single().end)
    }

    @Test
    fun malformedAndUnsupportedSequencesCannotCorruptOffsets() {
        val parsed = AnsiParser.parse("a\u001B[999999999999999999999mB\u001B[31unterminated", AnsiPalette.light)

        assertEquals("a\u001B[999999999999999999999mB\u001B[31unterminated", parsed.text)
        assertTrue(parsed.spanStyles.isEmpty())
    }

    @Test
    fun resetWithNoParametersRestoresDefaultStyle() {
        val parsed = AnsiParser.parse("\u001B[32mok\u001B[m plain", AnsiPalette.light)

        assertEquals("ok plain", parsed.text)
        assertEquals(0, parsed.spanStyles.single().start)
        assertEquals(2, parsed.spanStyles.single().end)
    }

    @Test
    fun palettesMeetNormalTextContrastOnConsoleBackgrounds() {
        assertPaletteContrast(AnsiPalette.light, Color(0xFFF7F7F7))
        assertPaletteContrast(AnsiPalette.dark, Color(0xFF1E1E1E))
    }

    private fun assertPaletteContrast(palette: AnsiPalette, background: Color) {
        (palette.normal + palette.bright).forEach { foreground ->
            val lighter = maxOf(foreground.luminance(), background.luminance())
            val darker = minOf(foreground.luminance(), background.luminance())
            val ratio = (lighter + 0.05f) / (darker + 0.05f)
            assertTrue("$foreground only has $ratio contrast", ratio >= 4.5f)
        }
    }
}
