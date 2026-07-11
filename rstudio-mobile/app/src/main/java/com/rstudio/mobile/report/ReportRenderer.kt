package com.rstudio.mobile.report

data class ReportChunkResult(
    val code: String,
    val output: String,
    val error: String? = null,
)

object ReportRenderer {
    private val chunkPattern = Regex("(?s)```\\s*\\{r[^}]*}\\s*\\n(.*?)```")

    fun render(markdown: String, evaluate: (String) -> ReportChunkResult): String {
        val html = StringBuilder()
        html.append("<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><style>")
        html.append("body{font-family:system-ui,sans-serif;max-width:960px;margin:32px auto;padding:0 20px;color:#202124}pre{background:#f1f3f4;padding:12px;border-radius:8px;overflow:auto}code{font-family:ui-monospace,monospace}h1,h2,h3{margin-top:1.6em}.error{color:#a50e0e;background:#fce8e6}")
        html.append("</style></head><body>")

        var cursor = 0
        chunkPattern.findAll(markdown).forEach { match ->
            html.append(markdownToHtml(markdown.substring(cursor, match.range.first)))
            val code = match.groupValues[1].trim()
            val result = evaluate(code)
            html.append("<details open><summary>R code</summary><pre><code>${escape(code)}</code></pre>")
            if (result.error != null) html.append("<pre class=\"error\">${escape(result.error)}</pre>")
            else if (result.output.isNotBlank()) html.append("<pre>${escape(result.output)}</pre>")
            html.append("</details>")
            cursor = match.range.last + 1
        }
        html.append(markdownToHtml(markdown.substring(cursor)))
        html.append("</body></html>")
        return html.toString()
    }

    private fun markdownToHtml(markdown: String): String = markdown
        .split("\n")
        .joinToString("\n") { line ->
            when {
                line.startsWith("### ") -> "<h3>${escape(line.removePrefix("### "))}</h3>"
                line.startsWith("## ") -> "<h2>${escape(line.removePrefix("## "))}</h2>"
                line.startsWith("# ") -> "<h1>${escape(line.removePrefix("# "))}</h1>"
                line.isBlank() -> ""
                else -> "<p>${escape(line)}</p>"
            }
        }

    private fun escape(value: String): String = value
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
}
