package com.rstudio.mobile.runtime

import com.rstudio.mobile.data.ProjectFile
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class EditorStateTransitionsTest {
    private val first = EditorDocument("a", "a.R", "a <- 1", "content://a", "/a.R")
    private val second = EditorDocument("b", "b.R", "b <- 1", "content://b", "/b.R")

    @Test
    fun invalidPersistedActiveIdFallsBackConsistently() {
        val restored = restoredEditorState(listOf(first, second), "missing", listOf("x"), null)

        assertEquals("a", restored.activeDocumentId)
        assertEquals("a.R", restored.currentFileName)
        assertEquals("a <- 1", restored.code)
        assertEquals(listOf("x"), restored.consoleHistory)
    }

    @Test
    fun matchingRecoveryUpdatesOnlyItsDocument() {
        val restored = restoredEditorState(
            listOf(first, second),
            "a",
            emptyList(),
            RecoveryDraft("b.R", "content://b", "b <- 2"),
        )

        assertEquals(2, restored.documents.size)
        assertEquals("a <- 1", restored.documents.first { it.id == "a" }.code)
        assertEquals("b <- 2", restored.documents.first { it.id == "b" }.code)
        assertEquals("b", restored.activeDocumentId)
        assertTrue(restored.isDirty)
    }

    @Test
    fun unmatchedRecoveryAddsDocumentAndPreservesTabs() {
        val restored = restoredEditorState(
            listOf(first, second),
            "a",
            emptyList(),
            RecoveryDraft("draft.R", null, "draft <- TRUE"),
        )

        assertEquals(3, restored.documents.size)
        assertTrue(restored.documents.any { it.id == "a" })
        assertTrue(restored.documents.any { it.id == "b" })
        assertEquals("draft <- TRUE", restored.code)
        assertTrue(restored.activeDocumentId.startsWith("recovery-draft.R"))
    }

    @Test
    fun openingProjectDocumentMakesSubsequentEditTargetIt() {
        val opened = state(active = "a").openEditorDocument(second, "Opened b.R")
        val edited = opened.editActiveDocument("b <- 2")

        assertEquals("b", edited.activeDocumentId)
        assertEquals("a <- 1", edited.documents.first { it.id == "a" }.code)
        assertEquals("b <- 2", edited.documents.first { it.id == "b" }.code)
        assertTrue(edited.documents.first { it.id == "b" }.isDirty)
    }

    @Test
    fun unchangedSaveMarksTargetClean() {
        val dirty = state(active = "a").editActiveDocument("a <- 2")
        val snapshot = dirty.activeSaveSnapshot()
        val completed = dirty.completeDocumentSave(snapshot)

        assertFalse(completed.isDirty)
        assertFalse(completed.documents.first { it.id == "a" }.isDirty)
    }

    @Test
    fun editDuringSaveRemainsDirty() {
        val dirty = state(active = "a").editActiveDocument("a <- 2")
        val snapshot = dirty.activeSaveSnapshot()
        val newer = dirty.editActiveDocument("a <- 3")
        val completed = newer.completeDocumentSave(snapshot)

        assertEquals("a <- 3", completed.code)
        assertTrue(completed.isDirty)
        assertTrue(completed.documents.first { it.id == "a" }.isDirty)
    }

    @Test
    fun tabSwitchDuringSaveDoesNotMutateActiveTab() {
        val dirty = state(active = "a").editActiveDocument("a <- 2")
        val snapshot = dirty.activeSaveSnapshot()
        val switched = dirty.openEditorDocument(second, "Opened b.R")
        val completed = switched.completeDocumentSave(snapshot)

        assertEquals("b", completed.activeDocumentId)
        assertEquals("b <- 1", completed.code)
        assertEquals("b.R", completed.currentFileName)
        assertFalse(completed.documents.first { it.id == "a" }.isDirty)
    }

    @Test
    fun saveAsMetadataAppliesOnlyToTargetDocument() {
        val dirty = state(active = "a").editActiveDocument("a <- 2")
        val snapshot = dirty.activeSaveSnapshot()
        val switched = dirty.openEditorDocument(second, "Opened b.R")
        val completed = switched.completeDocumentSave(
            snapshot,
            savedName = "renamed.R",
            savedSourceUri = "content://renamed",
            savedLocalPath = null,
        )

        val saved = completed.documents.first { it.id == "a" }
        assertEquals("renamed.R", saved.name)
        assertEquals("content://renamed", saved.sourceUri)
        assertNull(saved.localPath)
        assertEquals("b.R", completed.currentFileName)
        assertEquals("content://b", completed.currentDocumentUri)
    }

    @Test
    fun snapshotCurrentCheckRejectsNewerRevision() {
        val dirty = state(active = "a").editActiveDocument("a <- 2")
        val snapshot = dirty.activeSaveSnapshot()

        assertTrue(dirty.isCurrent(snapshot))
        assertFalse(dirty.editActiveDocument("a <- 3").isCurrent(snapshot))
        assertFalse(dirty.openEditorDocument(second, "Opened b.R").canClearRecovery(snapshot))
    }

    @Test
    fun projectReconciliationUpdatesRenamedOpenDocumentByStableUri() {
        val state = state(active = "b")
        val renamed = ProjectFile(
            uri = "content://b",
            name = "renamed.R",
            relativePath = "renamed.R",
            localPath = "/mirror/renamed.R",
            mimeType = "text/plain",
            isDirectory = false,
            size = 6,
        )

        val reconciled = state.reconcileProjectDocuments(listOf(renamed))

        assertEquals("b", reconciled.activeDocumentId)
        assertEquals("renamed.R", reconciled.currentFileName)
        assertEquals("/mirror/renamed.R", reconciled.currentScriptPath)
        assertEquals("b <- 1", reconciled.code)
    }

    private fun state(active: String): RStudioUiState =
        restoredEditorState(listOf(first, second), active, emptyList(), null)
}
