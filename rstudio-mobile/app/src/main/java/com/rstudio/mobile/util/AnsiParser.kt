package com.rstudio.mobile.util

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.font.FontWeight

/** Colors used for the sixteen portable ANSI foreground codes. */
internal data class AnsiPalette(
    val normal: List<Color>,
    val bright: List<Color>,
) {
    init {
        require(normal.size == ANSI_COLOR_COUNT) { "ANSI normal palette must contain 8 colors" }
        require(bright.size == ANSI_COLOR_COUNT) { "ANSI bright palette must contain 8 colors" }
    }

    internal fun color(index: Int, isBright: Boolean): Color =
        (if (isBright) bright else normal)[index]

    companion object {
        private const val ANSI_COLOR_COUNT = 8

        // Each entry has at least 4.5:1 contrast against the corresponding console background.
        val light = AnsiPalette(
            normal = listOf(
                Color(0xFF1F2328), Color(0xFFB42318), Color(0xFF176B3A), Color(0xFF765A00),
                Color(0xFF0958D9), Color(0xFF7622A8), Color(0xFF005F69), Color(0xFF4A4A4A),
            ),
            bright = listOf(
                Color(0xFF57606A), Color(0xFFA40E26), Color(0xFF116329), Color(0xFF6E5600),
                Color(0xFF0349B4), Color(0xFF6F1D91), Color(0xFF00545D), Color(0xFF24292F),
            ),
        )

        val dark = AnsiPalette(
            normal = listOf(
                Color(0xFFB8B8B8), Color(0xFFFF7B72), Color(0xFF7EE787), Color(0xFFE3B341),
                Color(0xFF79C0FF), Color(0xFFD2A8FF), Color(0xFF56D4DD), Color(0xFFF0F0F0),
            ),
            bright = listOf(
                Color(0xFFD0D0D0), Color(0xFFFFA198), Color(0xFFAFF5B4), Color(0xFFF2CC60),
                Color(0xFFA5D6FF), Color(0xFFE2C5FF), Color(0xFF8BE9F0), Color(0xFFFFFFFF),
            ),
        )
    }
}

/**
 * Converts terminal SGR sequences to Compose text without exposing escape bytes or raw offsets.
 *
 * Parsing the complete console preserves terminal state across newlines. Malformed and unsupported
 * control sequences are emitted literally so arbitrary runtime output cannot crash rendering.
 */
internal object AnsiParser {
    fun parse(input: String, palette: AnsiPalette): AnnotatedString {
        val output = AnnotatedString.Builder()
        parseLines(input, palette).forEachIndexed { index, line ->
            if (index > 0) output.append('\n')
            output.append(line)
        }
        return output.toAnnotatedString()
    }

    fun parseLines(input: String, palette: AnsiPalette): List<AnnotatedString> {
        val output = mutableListOf(AnnotatedString.Builder())
        var style = SgrStyle()
        var position = 0

        while (position < input.length) {
            when (input[position]) {
                '\n' -> {
                    output += AnnotatedString.Builder()
                    position += 1
                }
                ESCAPE -> {
                    val sequence = readSgr(input, position)
                    if (sequence == null) {
                        appendStyled(output.last(), input[position].toString(), style, palette)
                        position += 1
                    } else {
                        style = applyCodes(style, sequence.codes)
                        position = sequence.endExclusive
                    }
                }
                else -> {
                    val nextControl = input.indexOfAny(charArrayOf(ESCAPE, '\n'), position)
                        .takeIf { it >= 0 } ?: input.length
                    appendStyled(output.last(), input.substring(position, nextControl), style, palette)
                    position = nextControl
                }
            }
        }

        return output.map(AnnotatedString.Builder::toAnnotatedString)
    }

    private fun appendStyled(
        output: AnnotatedString.Builder,
        text: String,
        style: SgrStyle,
        palette: AnsiPalette,
    ) {
        if (text.isEmpty()) return
        val foreground = style.foreground
        if (foreground == null && !style.bold) {
            output.append(text)
            return
        }

        output.pushStyle(
            SpanStyle(
                color = foreground?.let { palette.color(it.index, it.bright) } ?: Color.Unspecified,
                fontWeight = if (style.bold) FontWeight.Bold else null,
            ),
        )
        output.append(text)
        output.pop()
    }

    private fun readSgr(input: String, start: Int): SgrSequence? {
        if (start + 1 >= input.length || input[start + 1] != '[') return null
        var end = start + 2
        while (end < input.length && (input[end].isDigit() || input[end] == ';')) end += 1
        if (end >= input.length || input[end] != 'm') return null

        val parameters = input.substring(start + 2, end)
        val codes = if (parameters.isEmpty()) {
            listOf(RESET)
        } else {
            parameters.split(';').map { parameter ->
                if (parameter.isEmpty()) RESET else parameter.toIntOrNull() ?: return null
            }
        }
        return SgrSequence(codes, end + 1)
    }

    private fun applyCodes(initial: SgrStyle, codes: List<Int>): SgrStyle {
        var style = initial
        for (code in codes) {
            style = when (code) {
                RESET -> SgrStyle()
                BOLD -> style.copy(bold = true)
                NORMAL_INTENSITY -> style.copy(bold = false)
                DEFAULT_FOREGROUND -> style.copy(foreground = null)
                in NORMAL_FOREGROUND -> style.copy(
                    foreground = AnsiColor(code - NORMAL_FOREGROUND.first, bright = false),
                )
                in BRIGHT_FOREGROUND -> style.copy(
                    foreground = AnsiColor(code - BRIGHT_FOREGROUND.first, bright = true),
                )
                else -> style
            }
        }
        return style
    }

    private data class SgrSequence(val codes: List<Int>, val endExclusive: Int)
    private data class SgrStyle(val foreground: AnsiColor? = null, val bold: Boolean = false)
    private data class AnsiColor(val index: Int, val bright: Boolean)

    private const val ESCAPE = '\u001B'
    private const val RESET = 0
    private const val BOLD = 1
    private const val NORMAL_INTENSITY = 22
    private const val DEFAULT_FOREGROUND = 39
    private val NORMAL_FOREGROUND = 30..37
    private val BRIGHT_FOREGROUND = 90..97
}
