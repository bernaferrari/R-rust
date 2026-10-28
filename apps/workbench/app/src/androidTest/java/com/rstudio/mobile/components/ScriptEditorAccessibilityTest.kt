package com.rstudio.mobile.components

import android.view.accessibility.AccessibilityNodeInfo
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.pressKey
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.rstudio.mobile.runtime.Diagnostic
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ScriptEditorAccessibilityTest {
    @get:Rule
    val compose = createComposeRule()

    @OptIn(ExperimentalTestApi::class)
    @Test
    fun touchFocusedEditorShortcutsRespectRunningStateAndRecover() {
        val running = mutableStateOf(false)
        val commands = mutableListOf<String>()
        val edits = mutableListOf<String>()
        var runs = 0
        var saves = 0
        compose.setContent {
            MaterialTheme {
                ScriptEditor(
                    "1+1", "script.R", false, running.value, "Ready", { edits.add(it) },
                    { commands.add(it) }, { runs++ }, {}, {}, {}, { saves++ }, {},
                )
            }
        }
        // Touch the actual editable control, then dispatch keyboard events to it.
        // No test focus manager or synthetic RequestFocus action is involved.
        val editor = compose.onNode(hasSetTextAction())
        editor.performClick().assertIsFocused()
        editor.performKeyInput {
            keyDown(Key.CtrlLeft)
            pressKey(Key.Enter)
            keyDown(Key.ShiftLeft)
            pressKey(Key.Enter)
            keyUp(Key.ShiftLeft)
            pressKey(Key.S)
            keyUp(Key.CtrlLeft)
        }
        compose.runOnIdle {
            assertEquals(listOf("1+1"), commands)
            assertEquals(1, runs)
            assertEquals(1, saves)
            running.value = true
        }
        editor.performClick().assertIsFocused()
        editor.performKeyInput {
            keyDown(Key.CtrlLeft)
            pressKey(Key.Enter)
            keyDown(Key.ShiftLeft)
            pressKey(Key.Enter)
            keyUp(Key.ShiftLeft)
            pressKey(Key.S)
            keyUp(Key.CtrlLeft)
        }
        compose.runOnIdle {
            assertEquals(listOf("1+1"), commands)
            assertEquals(1, runs)
            assertEquals(1, saves)
        }
        compose.onNodeWithText("Run").assertIsNotEnabled()
        compose.onNodeWithContentDescription("Save script (Ctrl+S)").assertIsNotEnabled()
        compose.onNodeWithContentDescription("More editor actions").performClick()
        compose.onNodeWithText("Run entire script").assertIsNotEnabled()
        compose.runOnIdle { running.value = false }
        compose.onNodeWithText("Run entire script").performClick()
        editor.performClick().assertIsFocused()
        editor.performKeyInput {
            keyDown(Key.CtrlLeft)
            pressKey(Key.Enter)
            pressKey(Key.S)
            keyUp(Key.CtrlLeft)
        }
        compose.runOnIdle {
            assertEquals(listOf("1+1", "1+1"), commands)
            assertEquals(2, runs)
            assertEquals(2, saves)
            assertEquals(emptyList<String>(), edits)
        }
    }

    private data class AccessibleNode(val label: String?, val text: String?, val editable: Boolean, val liveRegion: Int)

    private fun accessibilityNodes(): List<AccessibleNode> {
        val result = mutableListOf<AccessibleNode>()
        fun visit(node: AccessibilityNodeInfo?) {
            if (node == null) return
            result += AccessibleNode(node.contentDescription?.toString(), node.text?.toString(), node.isEditable, node.liveRegion)
            for (index in 0 until node.childCount) visit(node.getChild(index))
            @Suppress("DEPRECATION")
            node.recycle()
        }
        visit(InstrumentationRegistry.getInstrumentation().uiAutomation.rootInActiveWindow)
        return result
    }

    @Test
    fun editorLabelAndGutterReachAndroidAccessibilityBridgeCorrectly() {
        compose.setContent {
            MaterialTheme {
                ScriptEditor(
                    "1+1\n2+2", "script.R", true, false, "Ready", {}, {}, {}, {}, {}, {}, {}, {},
                    diagnostics = listOf(Diagnostic("unexpected symbol", 2)),
                )
            }
        }
        compose.onNodeWithContentDescription("R script editor").assert(hasSetTextAction())
        compose.onAllNodesWithText("1\n2", useUnmergedTree = true).assertCountEquals(0)
        compose.onNodeWithText("Line 2: unexpected symbol").assert(
            SemanticsMatcher.expectValue(SemanticsProperties.LiveRegion, LiveRegionMode.Polite)
        )
        compose.onNodeWithText("script.R •").assert(
            SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Unsaved changes")
        )
        compose.waitUntil(timeoutMillis = 5_000) {
            accessibilityNodes().any { it.label == "R script editor" && it.editable }
        }
        val nodes = accessibilityNodes()
        assertTrue(nodes.any { it.label == "R script editor" && it.editable && it.text == "1+1\n2+2" })
        assertTrue(nodes.none { it.text == "1\n2" })
        assertTrue(nodes.any { it.text == "Line 2: unexpected symbol" && it.liveRegion == android.view.View.ACCESSIBILITY_LIVE_REGION_POLITE })
    }

    @Test
    fun labelledFindReplaceControlsPreserveEditing() {
        val changed = mutableListOf<String>()
        compose.setContent {
            MaterialTheme {
                ScriptEditor("one + one", "script.R", true, false, "Ready", { changed.add(it) }, {}, {}, {}, {}, {}, {}, {})
            }
        }
        compose.onNodeWithContentDescription("More editor actions").performClick()
        compose.onNodeWithText("Find and replace").performClick()
        compose.onNodeWithText("Find").performClick().performTextInput("one")
        compose.onNodeWithText("Replace with").performClick().performTextInput("two")
        compose.onNodeWithText("Replace all").performClick()
        compose.onNodeWithContentDescription("R script editor").assert(
            SemanticsMatcher("editor contains replacement text") {
                it.config[SemanticsProperties.EditableText].text == "two + two"
            }
        )
        compose.onNodeWithText("Close").performClick()
        compose.onNodeWithText("Find").assertDoesNotExist()
        compose.runOnIdle { assertEquals(listOf("two + two"), changed) }
    }
}
