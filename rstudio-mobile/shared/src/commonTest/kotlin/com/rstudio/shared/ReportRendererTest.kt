package com.rstudio.shared

import kotlin.test.Test
import kotlin.test.assertContains

class ReportRendererTest {
    @Test
    fun rendersMarkdownAndChunksOnEveryTarget() {
        val html = ReportRenderer.render("# Title\n\n```{r}\n1 + 1\n```") {
            ReportChunkResult(it, "[1] 2")
        }
        assertContains(html, "<h1>Title</h1>")
        assertContains(html, "[1] 2")
    }
}
