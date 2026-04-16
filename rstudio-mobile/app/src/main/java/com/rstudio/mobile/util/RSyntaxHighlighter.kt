package com.rstudio.mobile.util

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString

object RSyntaxHighlighter {
    private val KEYWORDS = setOf(
        "function", "if", "else", "for", "while", "repeat", "break", "next",
        "return", "in", "TRUE", "FALSE", "NULL", "NA", "Inf", "NaN",
        "library", "require", "source", "print", "summary", "plot", "ggplot"
    )

    private val COLOR_KEYWORD = Color(0xFF569CD6)
    private val COLOR_STRING = Color(0xFFCE9178)
    private val COLOR_NUMBER = Color(0xFFB5CEA8)
    private val COLOR_COMMENT = Color(0xFF6A9955)
    private val COLOR_OPERATOR = Color(0xFFD4D4D4)

    fun highlight(text: AnnotatedString): AnnotatedString {
        return buildAnnotatedString {
            append(text)

            val input = text.text
            var pos = 0

            while (pos < input.length) {
                when {
                    input.getOrNull(pos) == '#' -> {
                        val end = input.indexOf('\n', pos).takeIf { it != -1 } ?: input.length
                        addStyle(SpanStyle(color = COLOR_COMMENT), pos, end)
                        pos = end
                    }

                    input[pos] == '"' || input[pos] == '\'' -> {
                        val quote = input[pos]
                        val end = (pos + 1 until input.length).find { i ->
                            input[i] == quote && input[i - 1] != '\\'
                        } ?: input.length
                        addStyle(SpanStyle(color = COLOR_STRING), pos, end + 1)
                        pos = end + 1
                    }

                    input[pos].isDigit() -> {
                        val end = (pos until input.length).takeWhile { input[it].isDigit() || input[it] == '.' }.lastOrNull() ?: pos
                        addStyle(SpanStyle(color = COLOR_NUMBER), pos, end + 1)
                        pos = end + 1
                    }

                    input[pos].isJavaIdentifierStart() -> {
                        val end = (pos until input.length).takeWhile { input[it].isJavaIdentifierPart() }.lastOrNull() ?: pos
                        val word = input.substring(pos, end + 1)
                        if (word in KEYWORDS) {
                            addStyle(SpanStyle(color = COLOR_KEYWORD, fontWeight = androidx.compose.ui.text.font.FontWeight.Bold), pos, end + 1)
                        }
                        pos = end + 1
                    }

                    else -> pos++
                }
            }
        }
    }
}
