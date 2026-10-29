package com.rstudio.mobile.components

import android.view.View
import android.view.accessibility.AccessibilityNodeInfo
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertHasNoClickAction
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.pressKey
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ConsoleViewAccessibilityTest {
    @get:Rule
    val compose = createComposeRule()

    private fun assertInput(expected: String) {
        compose.onNode(hasSetTextAction()).assert(SemanticsMatcher("console input equals $expected") {
            it.config[SemanticsProperties.EditableText].text == expected
        })
    }

    @OptIn(ExperimentalTestApi::class)
    @Test
    fun touchFocusedHistoryHandlesBoundedReplacementAndRestoresDraft() {
        val history = mutableStateOf(listOf("older()", "latest()"))
        val evaluated = mutableListOf<String>()
        compose.setContent {
            MaterialTheme {
                ConsoleView("", history.value, "No result yet", null, false, "Ready", {
                    evaluated.add(it)
                    history.value = listOf("latest()", it)
                }, {}, {})
            }
        }
        val input = compose.onNode(hasSetTextAction())
        input.performClick().assertIsFocused().performTextInput("new()")
        compose.onNodeWithContentDescription("Run console command").performClick()
        assertInput("")
        input.performClick().assertIsFocused().performKeyInput { pressKey(Key.DirectionUp) }
        assertInput("new()")
        input.performKeyInput { pressKey(Key.DirectionDown) }
        assertInput("")
        input.performTextInput("draft()")
        input.performKeyInput { pressKey(Key.DirectionUp); pressKey(Key.DirectionUp); pressKey(Key.DirectionUp) }
        assertInput("latest()")
        input.performKeyInput { pressKey(Key.DirectionDown); pressKey(Key.DirectionDown) }
        assertInput("draft()")
        compose.runOnIdle { assertEquals(listOf("new()"), evaluated) }
    }

    @Test
    fun labelledHistoryButtonsRespectBoundsAndPreserveDraft() {
        var evaluated = 0
        compose.setContent {
            MaterialTheme {
                ConsoleView("", listOf("older()", "latest()"), "No result yet", null, false, "Ready", { evaluated++ }, {}, {})
            }
        }
        compose.onNode(hasSetTextAction()).performClick().performTextInput("draft()")
        val previous = compose.onNodeWithText("Previous command")
        val next = compose.onNodeWithText("Next command")
        next.assertIsNotEnabled()
        previous.performClick()
        assertInput("latest()")
        previous.assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Command 2 of 2"))
        previous.performClick()
        assertInput("older()")
        previous.assertIsNotEnabled()
        next.performClick()
        assertInput("latest()")
        next.performClick()
        assertInput("draft()")
        next.assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Current draft"))
        next.assertIsNotEnabled()
        compose.runOnIdle { assertEquals(0, evaluated) }
    }

    private data class AccessibleNode(val text: String?, val label: String?, val clickable: Boolean, val liveRegion: Int)

    private fun accessibilityNodes(): List<AccessibleNode> {
        val result = mutableListOf<AccessibleNode>()
        fun visit(node: AccessibilityNodeInfo?) {
            if (node == null) return
            result += AccessibleNode(node.text?.toString(), node.contentDescription?.toString(), node.isClickable, node.liveRegion)
            for (index in 0 until node.childCount) visit(node.getChild(index))
            @Suppress("DEPRECATION")
            node.recycle()
        }
        visit(InstrumentationRegistry.getInstrumentation().uiAutomation.rootInActiveWindow)
        return result
    }

    @Test
    fun runningConsoleExportsHonestStatusErrorAndOutputSemantics() {
        var cancellations = 0
        var clears = 0
        compose.setContent {
            MaterialTheme {
                ConsoleView("first line\nsecond line", listOf("older()"), "No result yet", "calculation failed", true, "Running", {}, { clears++ }, { cancellations++ })
            }
        }
        compose.onNodeWithText("Running").assertHasNoClickAction().assert(
            SemanticsMatcher.expectValue(SemanticsProperties.LiveRegion, LiveRegionMode.Polite)
        )
        compose.onNodeWithText("calculation failed").assert(
            SemanticsMatcher.expectValue(SemanticsProperties.LiveRegion, LiveRegionMode.Polite)
        )
        compose.onNodeWithText("R console").assertIsNotEnabled()
        compose.onNodeWithContentDescription("Run console command").assertIsNotEnabled()
        compose.onNodeWithText("Previous command").assertIsNotEnabled()
        compose.onNodeWithText("Next command").assertIsNotEnabled()
        compose.onNodeWithText("Stop").performClick()
        compose.onNodeWithText("Clear").performClick()
        compose.waitUntil(timeoutMillis = 5_000) {
            accessibilityNodes().any { it.label == "Console output" }
        }
        val nodes = accessibilityNodes()
        assertTrue(nodes.any { it.text == "Running" && !it.clickable && it.liveRegion == View.ACCESSIBILITY_LIVE_REGION_POLITE })
        assertTrue(nodes.any { it.text == "calculation failed" && it.liveRegion == View.ACCESSIBILITY_LIVE_REGION_POLITE })
        assertTrue(nodes.any { it.text == "first line" })
        assertTrue(nodes.any { it.text == "second line" })
        compose.runOnIdle {
            assertEquals(1, cancellations)
            assertEquals(1, clears)
        }
    }
}
