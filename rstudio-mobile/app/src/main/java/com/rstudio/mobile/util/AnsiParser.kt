package com.rstudio.mobile.util

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString

object AnsiParser {
    private val COLORS = mapOf(
        30 to Color.Black,
        31 to Color(0xFFCD3131),
        32 to Color(0xFF0DBC79),
        33 to Color(0xFFE5E510),
        34 to Color(0xFF2472C8),
        35 to Color(0xFFBC3FBC),
        36 to Color(0xFF11A8CD),
        37 to Color(0xFFE5E5E5),
        39 to Color.White
    )

    fun parse(input: String): AnnotatedString {
        return buildAnnotatedString {
            var currentPos = 0
            val escapeStart = "\u001B["

            while (currentPos < input.length) {
                val escapeIndex = input.indexOf(escapeStart, currentPos)
                if (escapeIndex == -1) {
                    append(input.substring(currentPos))
                    break
                }

                append(input.substring(currentPos, escapeIndex))

                val mIndex = input.indexOf('m', escapeIndex)
                if (mIndex == -1) {
                    append(input.substring(escapeIndex))
                    break
                }

                val code = input.substring(escapeIndex + 2, mIndex).toIntOrNull()
                val color = COLORS[code] ?: Color.Unspecified

                val nextEscape = input.indexOf(escapeStart, mIndex + 1).takeIf { it != -1 } ?: input.length
                addStyle(SpanStyle(color = color), mIndex + 1, nextEscape)

                currentPos = mIndex + 1
            }
        }
    }
}
