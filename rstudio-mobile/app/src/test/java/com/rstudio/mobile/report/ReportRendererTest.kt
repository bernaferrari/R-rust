package com.rstudio.mobile.report

import org.junit.Assert.assertTrue
import org.junit.Test

class ReportRendererTest {
    @Test
    fun rendersMarkdownAndEvaluatedChunks() {
        val html = ReportRenderer.render("# Title\n\n```{r}\n1 + 1\n```") { code -> ReportChunkResult(code, "[1] 2") }
        assertTrue(html.contains("<h1>Title</h1>"))
        assertTrue(html.contains("1 + 1"))
        assertTrue(html.contains("[1] 2"))
    }
}
