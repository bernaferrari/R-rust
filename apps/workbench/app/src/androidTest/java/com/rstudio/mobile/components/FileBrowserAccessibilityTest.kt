package com.rstudio.mobile.components

import android.view.KeyEvent
import android.view.accessibility.AccessibilityNodeInfo
import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertHasNoClickAction
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.assertWidthIsAtLeast
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextReplacement
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.rstudio.mobile.data.ProjectFile
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@Suppress("DEPRECATION")
@RunWith(AndroidJUnit4::class)
class FileBrowserAccessibilityTest {
    @get:Rule val compose = createComposeRule()
    private val first = ProjectFile("uri1", "same.R", "scripts/same.R", "/project/scripts/same.R", "text/plain", false, 12)
    private val second = ProjectFile("uri2", "same.R", "analysis/same.R", "/project/analysis/same.R", "text/plain", false, 24)
    private val folder = ProjectFile("uri3", "scripts", "scripts", "/project/scripts", null, true, 0)

    private fun show(onOpen: (ProjectFile) -> Unit = {}, onRename: (ProjectFile, String) -> Unit = { _, _ -> }, onDelete: (ProjectFile) -> Unit = {}) {
        compose.setContent { MaterialTheme {
            FileBrowser("project", "/project", listOf(first, second, folder), null, emptyList(),
                {}, {}, {}, {}, {}, {}, onOpen, onRename, onDelete, {}, {})
        } }
    }

    private fun nativeButton(description: String): AccessibilityNodeInfo? {
        fun find(node: AccessibilityNodeInfo?): AccessibilityNodeInfo? {
            if (node == null) return null
            if (node.contentDescription?.toString() == description) return node
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

    @Test fun nativeOpenAndDeleteDisambiguateDuplicateNames() {
        val opened = mutableListOf<ProjectFile>(); val deleted = mutableListOf<ProjectFile>()
        show(onOpen = { opened += it }, onDelete = { deleted += it })
        compose.onNodeWithContentDescription("File scripts/same.R").assert(
            SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Button))
        compose.waitUntil(5000) { nativeButton("File scripts/same.R")?.let { it.recycle(); true } ?: false }
        nativeButton("File scripts/same.R")!!.let {
            assertEquals("Open scripts/same.R", it.actionList.first { action -> action.id == AccessibilityNodeInfo.ACTION_CLICK }.label.toString())
            assertTrue(it.performAction(AccessibilityNodeInfo.ACTION_CLICK)); it.recycle()
        }
        compose.onNodeWithContentDescription("Actions for analysis/same.R").assertHeightIsAtLeast(48.dp).assertWidthIsAtLeast(48.dp)
        nativeButton("Actions for analysis/same.R")!!.let { assertTrue(it.performAction(AccessibilityNodeInfo.ACTION_CLICK)); it.recycle() }
        compose.onNodeWithContentDescription("Delete analysis/same.R").performClick()
        compose.runOnIdle { assertEquals(listOf(first), opened); assertEquals(listOf(second), deleted) }
    }

    @Test fun contextualRenameKeepsPrefillCancelAndExactCallback() {
        val renamed = mutableListOf<Pair<ProjectFile, String>>()
        show(onRename = { file, name -> renamed += file to name })
        compose.onNodeWithContentDescription("Actions for analysis/same.R").performClick()
        compose.onNodeWithContentDescription("Rename analysis/same.R").performClick()
        compose.onNodeWithText("Rename analysis/same.R").assertExists()
        compose.onNodeWithText("New name").assertTextContains("same.R")
        compose.onNodeWithText("Cancel").performClick()
        compose.runOnIdle { assertTrue(renamed.isEmpty()) }
        compose.onNodeWithContentDescription("Actions for analysis/same.R").performClick()
        compose.onNodeWithContentDescription("Rename analysis/same.R").performClick()
        compose.onNodeWithText("New name").performClick().performTextReplacement("renamed.R")
        compose.onNodeWithText("Rename").performClick()
        compose.runOnIdle { assertEquals(listOf(second to "renamed.R"), renamed) }
    }

    @Test fun folderIsNonActionAndHardwareTabOpensContextMenu() {
        val opened = mutableListOf<ProjectFile>()
        show(onOpen = { opened += it })
        compose.onNodeWithContentDescription("Folder scripts").assertHasNoClickAction()
        val actions = compose.onNodeWithContentDescription("Actions for scripts/same.R")
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        var reached = false
        for (attempt in 0 until 24) {
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_TAB)
            compose.waitForIdle()
            reached = try { actions.assertIsFocused(); true } catch (_: AssertionError) { false }
            if (reached) break
        }
        assertTrue("Hardware Tab must reach contextual file actions", reached)
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ENTER)
        compose.onNodeWithContentDescription("Rename scripts/same.R").assertExists()
        compose.runOnIdle { assertTrue(opened.isEmpty()) }
    }
}
