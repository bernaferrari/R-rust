package com.rstudio.mobile.components

import android.view.KeyEvent
import android.view.accessibility.AccessibilityNodeInfo
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
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
class HelpViewerAccessibilityTest {
    @get:Rule val compose = createComposeRule()

    private fun nativeTopic(): AccessibilityNodeInfo? {
        fun find(node: AccessibilityNodeInfo?): AccessibilityNodeInfo? {
            if (node == null) return null
            if (node.text?.toString() == "c()") return node
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

    @Test fun nativeTopicActionHasContextAndLoadingNamesItsTopic() {
        val loading = mutableStateOf(false)
        val result = mutableStateOf<String?>(null)
        val lookups = mutableListOf<String>()
        var cleared = 0
        compose.setContent { MaterialTheme {
            HelpViewer(result.value, loading.value, { lookups += it; loading.value = true }, { cleared++ })
        } }
        compose.onNodeWithText("c()").assertHeightIsAtLeast(48.dp).assert(
            SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Button))
        compose.waitUntil(5000) { nativeTopic()?.let { it.recycle(); true } ?: false }
        val topic = nativeTopic()!!
        assertEquals("View help for c", topic.actionList.first { it.id == AccessibilityNodeInfo.ACTION_CLICK }.label.toString())
        assertTrue(topic.performAction(AccessibilityNodeInfo.ACTION_CLICK)); topic.recycle()
        compose.onNodeWithContentDescription("Loading help for c").assertExists()
        compose.runOnIdle { loading.value = false; result.value = "Documentation for combine" }
        compose.onNodeWithText("Documentation for combine").assertExists()
        compose.onNodeWithContentDescription("Back to help topics").assertHeightIsAtLeast(48.dp).performClick()
        compose.onNodeWithText("Search functions…").assertExists()
        compose.runOnIdle { assertEquals(listOf("c"), lookups); assertEquals(1, cleared) }
    }

    @Test fun searchPluralAndCategoryPersistAcrossDetailNavigation() {
        val lookups = mutableListOf<String>()
        compose.setContent { MaterialTheme { HelpViewer(null, false, { lookups += it }, {}) } }
        compose.onNode(
            hasText("stats") and SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Checkbox)
        ).performClick().assertIsSelected()
        compose.onNodeWithText("Search functions…").performClick().performTextInput("Arithmetic mean")
        compose.onNodeWithText("1 topic").assertExists()
        compose.onNodeWithText("mean()").performClick()
        compose.onNodeWithText("Tap a topic to view its R documentation.").assertExists()
        compose.onNodeWithContentDescription("Back to help topics").assertHeightIsAtLeast(48.dp).performClick()
        compose.onNodeWithText("1 topic").assertExists()
        compose.onNode(SemanticsMatcher.expectValue(SemanticsProperties.Selected, true)).assertTextContains("stats")
        compose.onNodeWithText("Search functions…").performClick().performTextInput(" missing")
        compose.onNodeWithText("0 topics").assertExists()
        compose.runOnIdle { assertEquals(listOf("mean"), lookups) }
    }

    @Test fun hardwareTabAndEnterReachTopicWithoutSyntheticFocus() {
        val lookups = mutableListOf<String>()
        compose.setContent { MaterialTheme { HelpViewer(null, false, { lookups += it }, {}) } }
        val topic = compose.onNodeWithText("c()")
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        var reached = false
        for (attempt in 0 until 16) {
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_TAB)
            compose.waitForIdle()
            reached = try { topic.assertIsFocused(); true } catch (_: AssertionError) { false }
            if (reached) break
        }
        assertTrue("Hardware Tab must reach the help topic", reached)
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ENTER)
        compose.onNodeWithContentDescription("Back to help topics").assertExists()
        compose.runOnIdle { assertEquals(listOf("c"), lookups) }
    }
}
