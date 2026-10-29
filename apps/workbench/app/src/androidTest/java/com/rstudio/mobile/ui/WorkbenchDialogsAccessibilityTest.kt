package com.rstudio.mobile.ui

import android.view.KeyEvent
import android.view.accessibility.AccessibilityNodeInfo
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.rport.uniffi.RuntimeInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@Suppress("DEPRECATION")
@RunWith(AndroidJUnit4::class)
class WorkbenchDialogsAccessibilityTest {
    @get:Rule val compose = createComposeRule()

    private fun nativeButton(label: String): AccessibilityNodeInfo? {
        fun find(node: AccessibilityNodeInfo?): AccessibilityNodeInfo? {
            if (node == null) return null
            if (node.text?.toString() == label || node.contentDescription?.toString() == label) return node
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

    private fun activate(label: String) {
        compose.waitUntil(5000) { nativeButton(label)?.let { it.recycle(); true } ?: false }
        nativeButton(label)!!.let { assertTrue(it.performAction(AccessibilityNodeInfo.ACTION_CLICK)); it.recycle() }
    }

    @Test fun longRuntimePathsScrollWhileResetRemainsReachable() {
        val open = mutableStateOf(true)
        var resets = 0; var dismisses = 0
        val paths = (1..40).map { "/library/path/$it" }
        compose.setContent { MaterialTheme {
            if (open.value) RuntimeInformationDialog(RuntimeInfo(true, paths, "/temporary"),
                { dismisses++; open.value = false }, { resets++; open.value = false })
        } }
        compose.onNodeWithText("Active").assertExists()
        compose.onNodeWithText("Temporary directory: /temporary").assertExists()
        compose.onNodeWithText("/library/path/40").performScrollTo().assertIsDisplayed()
        compose.onNodeWithText("Reset session").assertHeightIsAtLeast(48.dp).assertIsDisplayed()
        compose.onNodeWithText("Close").assertHeightIsAtLeast(48.dp).assertIsDisplayed()
        activate("Reset session")
        compose.runOnIdle { assertEquals(1, resets); assertEquals(0, dismisses) }
        compose.onNodeWithText("R runtime").assertDoesNotExist()
    }

    @Test fun unavailableCloseAndKeyboardBackDismissWithoutResetAndRestoreFocus() {
        val open = mutableStateOf(true)
        var resets = 0; var dismisses = 0
        compose.setContent { MaterialTheme {
            TextButton(onClick = { open.value = true }) { Text("Show runtime") }
            if (open.value) RuntimeInformationDialog(null,
                { dismisses++; open.value = false }, { resets++; open.value = false })
        } }
        compose.onNodeWithText("Unavailable").assertExists()
        compose.onNodeWithText("Temporary directory: unknown").assertExists()
        activate("Close")
        val trigger = compose.onNodeWithText("Show runtime")
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        var reached = false
        for (attempt in 0 until 6) {
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_TAB)
            compose.waitForIdle()
            reached = try { trigger.assertIsFocused(); true } catch (_: AssertionError) { false }
            if (reached) break
        }
        assertTrue("Keyboard must reach dialog trigger", reached)
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ENTER)
        compose.onNodeWithText("Unavailable").assertExists()
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
        compose.onNodeWithText("R runtime").assertDoesNotExist()
        trigger.assertIsFocused()
        compose.runOnIdle { assertEquals(2, dismisses); assertEquals(0, resets) }
    }

    @Test fun contextualDiscardAndHardwareCancelKeepCallbacksSeparate() {
        val open = mutableStateOf(true)
        val discarded = mutableListOf<String>()
        var dismisses = 0
        compose.setContent { MaterialTheme {
            if (open.value) DiscardChangesDialog("analysis.R",
                { dismisses++; open.value = false }, { discarded += "document-42"; open.value = false })
        } }
        compose.onNodeWithText("analysis.R has unsaved changes.").assertExists()
        compose.onNodeWithContentDescription("Discard changes to analysis.R").assertHeightIsAtLeast(48.dp)
        activate("Discard changes to analysis.R")
        compose.runOnIdle { assertEquals(listOf("document-42"), discarded); open.value = true }
        val cancel = compose.onNodeWithText("Cancel")
        cancel.assertHeightIsAtLeast(48.dp)
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        var reached = false
        for (attempt in 0 until 6) {
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_TAB)
            compose.waitForIdle()
            reached = try { cancel.assertIsFocused(); true } catch (_: AssertionError) { false }
            if (reached) break
        }
        assertTrue("Keyboard must reach Cancel", reached)
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ENTER)
        compose.onNodeWithText("Discard changes?").assertDoesNotExist()
        compose.runOnIdle { assertEquals(1, dismisses); assertEquals(listOf("document-42"), discarded) }
    }
}
