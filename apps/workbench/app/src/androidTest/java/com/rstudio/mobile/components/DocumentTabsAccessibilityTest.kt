package com.rstudio.mobile.components

import android.view.KeyEvent
import android.view.accessibility.AccessibilityNodeInfo
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.assertWidthIsAtLeast
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.rstudio.mobile.runtime.EditorDocument
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@Suppress("DEPRECATION")
@RunWith(AndroidJUnit4::class)
class DocumentTabsAccessibilityTest {
    @get:Rule val compose = createComposeRule()
    private val alpha = EditorDocument("a", "alpha.R", "1")
    private val beta = EditorDocument("b", "beta.R", "2", isDirty = true)

    private fun nativeClose(): AccessibilityNodeInfo? {
        fun find(node: AccessibilityNodeInfo?): AccessibilityNodeInfo? {
            if (node == null) return null
            if (node.contentDescription?.toString() == "Close beta.R") return node
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

    @Test fun tabsExposeSaveStateAndSelectionWithoutSpeakingDirtyBullet() {
        val docs = mutableStateOf(listOf(alpha, beta))
        val active = mutableStateOf("a")
        compose.setContent { MaterialTheme { DocumentTabs(docs.value, active.value, { active.value = it }, {}) } }
        compose.onNodeWithText("alpha.R").assertIsSelected().assert(
            SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Saved"))
        val dirty = compose.onNodeWithText("beta.R")
        dirty.assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Unsaved changes"))
        compose.onNodeWithText(" •").assertDoesNotExist()
        dirty.performClick().assertIsSelected()
        compose.runOnIdle { docs.value = listOf(alpha, beta.copy(isDirty = false)) }
        dirty.assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Saved"))
    }

    @Test fun nativeCloseNamesTheDocumentAndKeepsSelectionIndependent() {
        val closed = mutableListOf<String>()
        val selected = mutableListOf<String>()
        compose.setContent { MaterialTheme { DocumentTabs(listOf(alpha, beta), "a", { selected += it }, { closed += it.id }) } }
        compose.onNodeWithContentDescription("Close beta.R").assertHeightIsAtLeast(48.dp).assertWidthIsAtLeast(48.dp)
        compose.waitUntil(5000) { nativeClose()?.let { it.recycle(); true } ?: false }
        val close = nativeClose()!!
        assertTrue(close.performAction(AccessibilityNodeInfo.ACTION_CLICK)); close.recycle()
        compose.runOnIdle { assertEquals(listOf("b"), closed); assertTrue(selected.isEmpty()) }
    }

    @Test fun keyboardTraversalReachesCloseAndSingleDocumentHasNoClose() {
        val docs = mutableStateOf(listOf(alpha, beta))
        val closed = mutableListOf<String>()
        compose.setContent { MaterialTheme { DocumentTabs(docs.value, "a", {}, { closed += it.id; docs.value = listOf(alpha) }) } }
        val close = compose.onNodeWithContentDescription("Close beta.R")
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        var reached = false
        for (attempt in 0 until 12) {
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_TAB)
            compose.waitForIdle()
            reached = try { close.assertIsFocused(); true } catch (_: AssertionError) { false }
            if (reached) break
        }
        assertTrue("Keyboard Tab must reach the contextual close button", reached)
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ENTER)
        compose.runOnIdle { assertEquals(listOf("b"), closed) }
        compose.onNodeWithContentDescription("Close alpha.R").assertDoesNotExist()
        compose.onNodeWithText("alpha.R").assertExists()
        compose.runOnIdle { docs.value = emptyList() }
        compose.onNodeWithText("alpha.R").assertDoesNotExist()
    }
}
