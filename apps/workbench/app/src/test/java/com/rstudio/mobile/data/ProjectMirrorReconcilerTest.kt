package com.rstudio.mobile.data

import java.io.ByteArrayInputStream
import java.io.File
import java.nio.file.Files
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class ProjectMirrorReconcilerTest {
    @Test
    fun successfulRefreshAddsModifiesAndDeletesExactly() = withMirror { root ->
        val reconciler = ProjectMirrorReconciler()
        reconciler.rebuild(root, sequenceOf(file("a.R", "old"), file("stale.R", "stale")))

        val files = reconciler.rebuild(root, sequenceOf(file("a.R", "new"), file("added.R", "added")))

        assertEquals("new", File(root, "a.R").readText())
        assertEquals("added", File(root, "added.R").readText())
        assertFalse(File(root, "stale.R").exists())
        assertEquals(listOf("a.R", "added.R"), files.map(ProjectFile::relativePath))
    }

    @Test
    fun oversizedFilePreservesPreviousMirror() = withMirror { root ->
        val reconciler = ProjectMirrorReconciler(ProjectMirrorLimits(maxFileBytes = 3))
        reconciler.rebuild(root, sequenceOf(file("kept.R", "ok")))

        assertThrows(IllegalArgumentException::class.java) {
            reconciler.rebuild(root, sequenceOf(file("too-large.R", "1234")))
        }

        assertEquals("ok", File(root, "kept.R").readText())
        assertFalse(File(root, "too-large.R").exists())
    }

    @Test
    fun aggregateLimitIsEnforcedWhileStreaming() = withMirror { root ->
        val reconciler = ProjectMirrorReconciler(ProjectMirrorLimits(maxFileBytes = 10, maxTotalBytes = 5))

        assertThrows(IllegalArgumentException::class.java) {
            reconciler.rebuild(root, sequenceOf(file("a", "123"), file("b", "456")))
        }
        assertFalse(root.exists())
    }

    @Test
    fun entryLimitIsExact() = withMirror { root ->
        val reconciler = ProjectMirrorReconciler(ProjectMirrorLimits(maxEntries = 1))
        reconciler.rebuild(root, sequenceOf(file("a", "1")))

        assertThrows(IllegalArgumentException::class.java) {
            reconciler.rebuild(root, sequenceOf(file("a", "1"), file("b", "2")))
        }
        assertEquals("1", File(root, "a").readText())
    }

    @Test
    fun nestingLimitIsExact() = withMirror { root ->
        val reconciler = ProjectMirrorReconciler(ProjectMirrorLimits(maxDepth = 2))
        reconciler.rebuild(root, sequenceOf(file("one/two", "ok")))

        assertThrows(IllegalArgumentException::class.java) {
            reconciler.rebuild(root, sequenceOf(file("one/two/three", "bad")))
        }
        assertEquals("ok", File(root, "one/two").readText())
    }

    @Test
    fun traversalAndAbsolutePathsAreRejected() = withMirror { root ->
        val reconciler = ProjectMirrorReconciler()

        assertThrows(IllegalArgumentException::class.java) {
            reconciler.rebuild(root, sequenceOf(file("../escape", "bad")))
        }
        assertThrows(IllegalArgumentException::class.java) {
            reconciler.rebuild(root, sequenceOf(file("/absolute", "bad")))
        }
    }

    @Test
    fun sanitizedPathCollisionsCannotOverwrite() = withMirror { root ->
        val reconciler = ProjectMirrorReconciler()

        assertThrows(IllegalArgumentException::class.java) {
            reconciler.rebuild(root, sequenceOf(file("same", "first"), file("same", "second")))
        }
        assertFalse(root.exists())
    }

    @Test
    fun directoriesAndActualSizesAreReported() = withMirror { root ->
        val reconciler = ProjectMirrorReconciler()
        val entries = sequenceOf(directory("src"), file("src/main.R", "12345"))

        val files = reconciler.rebuild(root, entries)

        assertTrue(File(root, "src").isDirectory)
        assertEquals(0L, files.first { it.isDirectory }.size)
        assertEquals(5L, files.first { !it.isDirectory }.size)
    }

    @Test
    fun interruptedSwapBackupIsRecoveredBeforeRefresh() = withMirror { root ->
        val backup = File(root.parentFile, ".${root.name}.backup").also { it.mkdirs() }
        File(backup, "old.R").writeText("old")
        val reconciler = ProjectMirrorReconciler(ProjectMirrorLimits(maxFileBytes = 1))

        assertThrows(IllegalArgumentException::class.java) {
            reconciler.rebuild(root, sequenceOf(file("too-large.R", "12")))
        }

        assertEquals("old", File(root, "old.R").readText())
    }

    private fun file(path: String, contents: String) = ProjectMirrorEntry(
        uri = "content://$path",
        displayName = path.substringAfterLast('/'),
        relativePath = path,
        mimeType = "text/plain",
        isDirectory = false,
        openInput = { ByteArrayInputStream(contents.toByteArray()) },
    )

    private fun directory(path: String) = ProjectMirrorEntry(
        uri = "content://$path",
        displayName = path.substringAfterLast('/'),
        relativePath = path,
        mimeType = null,
        isDirectory = true,
    )

    private fun withMirror(block: (File) -> Unit) {
        val parent = Files.createTempDirectory("rport-mirror-test").toFile()
        try {
            block(File(parent, "mirror"))
        } finally {
            parent.deleteRecursively()
        }
    }
}
