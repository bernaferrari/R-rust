package com.rstudio.mobile.util

internal fun executionTarget(text: String, selectionStart: Int, selectionEnd: Int): String {
    val start = selectionStart.coerceIn(0, text.length)
    val end = selectionEnd.coerceIn(0, text.length)
    if (start != end) return text.substring(minOf(start, end), maxOf(start, end))
    val lineStart = text.lastIndexOf('\n', startIndex = (start - 1).coerceAtLeast(0))
        .let { if (it < 0) 0 else it + 1 }
    val lineEnd = text.indexOf('\n', startIndex = start).let { if (it < 0) text.length else it }
    return text.substring(lineStart, lineEnd).ifBlank { text }
}
