package com.rstudio.mobile.data

import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ProjectRepositoryTest {
    private val repository = ProjectRepository(ApplicationProvider.getApplicationContext())

    @After
    fun cleanUp() {
        repository.clearRecovery()
        repository.clearProject()
    }

    @Test
    fun unsavedEditorDraftSurvivesRepositoryRecreation() {
        repository.saveRecovery("analysis.R", "content://example/analysis", "answer <- 42")

        val restored = ProjectRepository(ApplicationProvider.getApplicationContext()).restoreRecovery()

        assertEquals("analysis.R", restored?.name)
        assertEquals("content://example/analysis", restored?.sourceUri)
        assertEquals("answer <- 42", restored?.code)
    }

    @Test
    fun clearingRecoveryRemovesDraft() {
        repository.saveRecovery("analysis.R", null, "x <- 1")
        repository.clearRecovery()

        assertNull(repository.restoreRecovery())
    }
}
