package com.rstudio.mobile.util

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight

object RSyntaxHighlighter {

    // Control-flow and declaration keywords
    private val KEYWORDS = setOf(
        "function", "if", "else", "for", "while", "repeat", "break", "next",
        "return", "in", "library", "require", "source",
    )

    // Special constants
    private val CONSTANTS = setOf(
        "TRUE", "FALSE", "NULL", "NA", "Inf", "NaN", "T", "F",
        "NA_real_", "NA_integer_", "NA_character_", "NA_complex_",
    )

    // Assignment / pipe operators (multi-char, checked before single-char fallback)
    private val MULTI_CHAR_OPS = listOf("<<-", "->>", "<-", "->", "|>", "%%", "%/%", "!=", "==", ">=", "<=", "&&", "||")

    // ── Palette (VS-Code-dark inspired) ───────────────────────────
    private val COLOR_KEYWORD    = Color(0xFF569CD6)  // blue
    private val COLOR_CONSTANT   = Color(0xFF4EC9B0)  // teal
    private val COLOR_STRING     = Color(0xFFCE9178)  // orange
    private val COLOR_NUMBER     = Color(0xFFB5CEA8)  // green
    private val COLOR_COMMENT    = Color(0xFF6A9955)  // olive
    private val COLOR_OPERATOR   = Color.Unspecified   // inherit editor foreground in both themes
    private val COLOR_FUNCTION   = Color(0xFFDCDCAA)  // yellow
    // ──────────────────────────────────────────────────────────────

    fun highlight(text: AnnotatedString): AnnotatedString {
        return buildAnnotatedString {
            append(text)
            val input = text.text
            val len = input.length
            var pos = 0

            while (pos < len) {
                val ch = input[pos]
                when {
                    // ── Comments ──────────────────────────────────
                    ch == '#' -> {
                        val end = input.indexOf('\n', pos).takeIf { it != -1 } ?: len
                        addStyle(SpanStyle(color = COLOR_COMMENT, fontStyle = FontStyle.Italic), pos, end)
                        pos = end
                    }

                    // ── Strings (single / double quoted) ─────────
                    ch == '"' || ch == '\'' -> {
                        val quote = ch
                        var i = pos + 1
                        while (i < len) {
                            if (input[i] == '\\') { i += 2; continue }
                            if (input[i] == quote) { i++; break }
                            i++
                        }
                        addStyle(SpanStyle(color = COLOR_STRING), pos, i)
                        pos = i
                    }

                    // ── Numbers: hex, scientific, complex, integer suffix, float ──
                    ch.isDigit() || (ch == '.' && pos + 1 < len && input[pos + 1].isDigit()) -> {
                        val start = pos
                        if (ch == '0' && pos + 1 < len && input[pos + 1].lowercaseChar() == 'x') {
                            // hex literal  0xFF
                            pos += 2
                            while (pos < len && input[pos].isHexDigit()) pos++
                        } else {
                            // integer / float part
                            while (pos < len && (input[pos].isDigit() || input[pos] == '.')) pos++
                            // scientific notation  1e-5  3.14E+2
                            if (pos < len && input[pos].lowercaseChar() == 'e') {
                                pos++
                                if (pos < len && (input[pos] == '+' || input[pos] == '-')) pos++
                                while (pos < len && input[pos].isDigit()) pos++
                            }
                        }
                        // R integer suffix  1L
                        if (pos < len && input[pos] == 'L') pos++
                        // complex suffix  2+3i  (only the trailing 'i')
                        if (pos < len && input[pos] == 'i') pos++
                        addStyle(SpanStyle(color = COLOR_NUMBER), start, pos)
                    }

                    // ── Identifiers: keywords / constants / function calls ──
                    ch.isLetter() || ch == '.' || ch == '_' -> {
                        val start = pos
                        while (pos < len && (input[pos].isLetterOrDigit() || input[pos] == '.' || input[pos] == '_')) pos++
                        val word = input.substring(start, pos)

                        // Peek ahead past whitespace to detect function call  foo(
                        val peekIdx = (pos until len).firstOrNull { !input[it].isWhitespace() }

                        when {
                            word in KEYWORDS -> addStyle(
                                SpanStyle(color = COLOR_KEYWORD, fontWeight = FontWeight.Bold), start, pos,
                            )
                            word in CONSTANTS -> addStyle(
                                SpanStyle(color = COLOR_CONSTANT, fontWeight = FontWeight.Bold), start, pos,
                            )
                            word == "..." -> addStyle(
                                SpanStyle(color = COLOR_CONSTANT, fontWeight = FontWeight.Bold), start, pos,
                            )
                            peekIdx != null && input[peekIdx] == '(' -> addStyle(
                                SpanStyle(color = COLOR_FUNCTION), start, pos,
                            )
                        }
                    }

                    // ── Multi-character operators  <<-  ->  |>  etc. ──
                    else -> {
                        val matched = MULTI_CHAR_OPS.firstOrNull { op ->
                            input.startsWith(op, pos)
                        }
                        if (matched != null) {
                            addStyle(SpanStyle(color = COLOR_OPERATOR, fontWeight = FontWeight.Bold), pos, pos + matched.length)
                            pos += matched.length
                        } else {
                            pos++
                        }
                    }
                }
            }
        }
    }

    private fun Char.isHexDigit(): Boolean =
        this in '0'..'9' || this in 'a'..'f' || this in 'A'..'F'
}
