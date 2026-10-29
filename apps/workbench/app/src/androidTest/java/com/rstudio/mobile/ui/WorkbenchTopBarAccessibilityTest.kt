package com.rstudio.mobile.ui

import android.view.KeyEvent
import android.view.accessibility.AccessibilityNodeInfo
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertWidthIsAtLeast
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@Suppress("DEPRECATION")
@RunWith(AndroidJUnit4::class)
class WorkbenchTopBarAccessibilityTest {
    @get:Rule val compose = createComposeRule()

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

    @Test fun idleNativeActionsHaveUsableBoundsAndDirtyStateIsExplicit() {
        val calls = mutableListOf<String>()
        compose.setContent { MaterialTheme {
            WorkbenchTopBar(null, "analysis.R", true, false,
                { calls += "open" }, { calls += "info" }, { calls += "stop" }, { calls += "run" })
        } }
        compose.onNodeWithText("R Workbench").assertExists()
        compose.onNodeWithText("analysis.R").assert(
            SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Unsaved changes"))
        compose.onNodeWithText(" •").assertDoesNotExist()
        for (description in listOf("Open project folder", "Runtime information", "Run entire script")) {
            compose.onNodeWithContentDescription(description).assertHeightIsAtLeast(48.dp).assertWidthIsAtLeast(48.dp)
            compose.waitUntil(5000) { nativeButton(description)?.let { it.recycle(); true } ?: false }
            nativeButton(description)!!.let { assertTrue(it.performAction(AccessibilityNodeInfo.ACTION_CLICK)); it.recycle() }
        }
        compose.onNodeWithContentDescription("Stop R execution").assertDoesNotExist()
        compose.runOnIdle { assertEquals(listOf("open", "info", "run"), calls) }
    }

    @Test fun runningStateKeepsOpenDisabledAndReplacesRunWithStop() {
        val running = mutableStateOf(true)
        val dirty = mutableStateOf(true)
        val calls = mutableListOf<String>()
        compose.setContent { MaterialTheme {
            WorkbenchTopBar("project", "analysis.R", dirty.value, running.value,
                { calls += "open" }, { calls += "info" },
                { calls += "stop"; running.value = false; dirty.value = false }, { calls += "run" })
        } }
        compose.onNodeWithText("project").assertExists()
        compose.onNodeWithContentDescription("Open project folder").assertIsNotEnabled()
        compose.onNodeWithContentDescription("Runtime information").assertIsEnabled()
        compose.onNodeWithContentDescription("Run entire script").assertDoesNotExist()
        compose.onNodeWithContentDescription("Stop R execution").assertHeightIsAtLeast(48.dp).assertWidthIsAtLeast(48.dp)
        compose.waitUntil(5000) { nativeButton("Stop R execution")?.let { it.recycle(); true } ?: false }
        nativeButton("Stop R execution")!!.let { assertTrue(it.performAction(AccessibilityNodeInfo.ACTION_CLICK)); it.recycle() }
        compose.onNodeWithContentDescription("Run entire script").assertIsEnabled()
        compose.onNodeWithContentDescription("Open project folder").assertIsEnabled()
        compose.onNodeWithText("analysis.R").assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Saved"))
        compose.runOnIdle { assertEquals(listOf("stop"), calls) }
    }

    @Test fun hardwareTabAndEnterReachRunWithoutSyntheticFocus() {
        val calls = mutableListOf<String>()
        compose.setContent { MaterialTheme {
            WorkbenchTopBar(null, "analysis.R", false, false, {}, {}, {}, { calls += "run" })
        } }
        val run = compose.onNodeWithContentDescription("Run entire script")
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        var reached = false
        for (attempt in 0 until 9) {
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_TAB)
            compose.waitForIdle()
            reached = try { run.assertIsFocused(); true } catch (_: AssertionError) { false }
            if (reached) break
        }
        assertTrue("Hardware Tab must reach Run", reached)
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ENTER)
        compose.runOnIdle { assertEquals(listOf("run"), calls) }
    }
}
