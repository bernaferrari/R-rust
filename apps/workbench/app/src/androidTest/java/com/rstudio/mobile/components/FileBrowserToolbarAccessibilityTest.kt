package com.rstudio.mobile.components

import android.view.KeyEvent
import android.view.accessibility.AccessibilityNodeInfo
import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertWidthIsAtLeast
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.rstudio.mobile.runtime.ScriptFile
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@Suppress("DEPRECATION")
@RunWith(AndroidJUnit4::class)
class FileBrowserToolbarAccessibilityTest {
    @get:Rule val compose = createComposeRule()

    private fun nativeButton(description: String): AccessibilityNodeInfo? {
        fun find(node: AccessibilityNodeInfo?): AccessibilityNodeInfo? {
            if (node == null) return null
            if (node.contentDescription?.toString() == description || node.text?.toString() == description) return node
            for (i in 0 until node.childCount) {
                val found = find(node.getChild(i))
                if (found != null) { node.recycle(); return found }
            }
            node.recycle(); return null
        }
        var node = find(InstrumentationRegistry.getInstrumentation().uiAutomation.rootInActiveWindow)
        while (node != null && !node.isClickable) {
            val parent = node.parent; node.recycle(); node = parent
        }
        return node
    }

    @Test fun nativeToolbarNamesDispatchDistinctCallbacksWithUsableBounds() {
        val calls = mutableListOf<String>()
        compose.setContent { MaterialTheme {
            FileBrowser(projectName = null, projectRoot = null, projectFiles = emptyList(), importedPath = null,
                recentScripts = emptyList(), onOpenProject = { calls += "folder" }, onCloseProject = {},
                onImportCsv = { calls += "import" }, onOpenScript = { calls += "open" }, onNewScript = { calls += "new" },
                onOpenRecent = {}, onOpenProjectFile = {}, onRename = { _, _ -> }, onDelete = {},
                onCreateFolder = {}, onExportProject = {}, onSaveWorkspace = { calls += "save" }, onLoadWorkspace = { calls += "load" })
        } }
        compose.onNodeWithText("Open a project folder, an R script, or import a CSV/TSV file to begin.").assertExists()
        for (description in listOf("Open project folder", "Create R script", "Open R script", "Import data file")) {
            compose.onNodeWithContentDescription(description).assertHeightIsAtLeast(48.dp).assertWidthIsAtLeast(48.dp)
            compose.waitUntil(5000) { nativeButton(description)?.let { it.recycle(); true } ?: false }
            nativeButton(description)!!.let { assertTrue(it.performAction(AccessibilityNodeInfo.ACTION_CLICK)); it.recycle() }
        }
        for (description in listOf("Save workspace", "Load workspace")) {
            compose.onNodeWithText(description).assertHeightIsAtLeast(48.dp)
            nativeButton(description)!!.let { assertTrue(it.performAction(AccessibilityNodeInfo.ACTION_CLICK)); it.recycle() }
        }
        compose.runOnIdle { assertEquals(listOf("folder", "new", "open", "import", "save", "load"), calls) }
        compose.onNodeWithText("New folder").assertDoesNotExist()
    }

    @Test fun projectNamesAndCreateFolderKeepExistingActionFlow() {
        val calls = mutableListOf<String>()
        compose.setContent { MaterialTheme {
            FileBrowser(projectName = "analysis", projectRoot = "/analysis", projectFiles = emptyList(), importedPath = null,
                recentScripts = emptyList(), onOpenProject = {}, onCloseProject = { calls += "close" }, onImportCsv = {},
                onOpenScript = {}, onNewScript = {}, onOpenRecent = {}, onOpenProjectFile = {}, onRename = { _, _ -> },
                onDelete = {}, onCreateFolder = { calls += it }, onExportProject = { calls += "export" })
        } }
        compose.onNodeWithContentDescription("Export project analysis").assertHeightIsAtLeast(48.dp).performClick()
        compose.onNodeWithContentDescription("Close project analysis").assertHeightIsAtLeast(48.dp).performClick()
        compose.onNodeWithText("New folder").performClick()
        compose.onNodeWithText("Folder name").performClick().performTextInput("results")
        compose.onNodeWithText("Create").performClick()
        compose.onNodeWithText("Folder name").assertDoesNotExist()
        compose.runOnIdle { assertEquals(listOf("export", "close", "results"), calls) }
    }

    @Test fun hardwareTabReachesDistinctRecentScriptPath() {
        val opened = mutableListOf<String>()
        val scripts = listOf(ScriptFile("same.R", "/one/same.R"), ScriptFile("same.R", "/two/same.R"))
        compose.setContent { MaterialTheme {
            FileBrowser(projectName = null, projectRoot = null, projectFiles = emptyList(), importedPath = null,
                recentScripts = scripts, onOpenProject = {}, onCloseProject = {}, onImportCsv = {}, onOpenScript = {},
                onNewScript = {}, onOpenRecent = { opened += it }, onOpenProjectFile = {}, onRename = { _, _ -> },
                onDelete = {}, onCreateFolder = {}, onExportProject = {})
        } }
        val recent = compose.onNodeWithContentDescription("Open recent script /two/same.R")
        recent.assertHeightIsAtLeast(48.dp)
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        var reached = false
        for (attempt in 0 until 18) {
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_TAB)
            compose.waitForIdle()
            reached = try { recent.assertIsFocused(); true } catch (_: AssertionError) { false }
            if (reached) break
        }
        assertTrue("Hardware Tab must reach recent script by path", reached)
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ENTER)
        compose.runOnIdle { assertEquals(listOf("/two/same.R"), opened) }
    }
}
