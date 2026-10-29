package com.rstudio.mobile.components

import android.view.accessibility.AccessibilityNodeInfo
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.rstudio.mobile.runtime.EnvEntry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@Suppress("DEPRECATION")
@RunWith(AndroidJUnit4::class)
class EnvironmentBrowserAccessibilityTest {
    @get:Rule
    val compose = createComposeRule()

    private val alpha = EnvEntry("alpha", "integer", "1 2 3")
    private val beta = EnvEntry("beta", "data.frame", "3 rows, 2 columns")

    private fun nativeNode(predicate: (AccessibilityNodeInfo) -> Boolean): AccessibilityNodeInfo? {
        fun find(node: AccessibilityNodeInfo?): AccessibilityNodeInfo? {
            if (node == null) return null
            if (predicate(node)) return node
            for (index in 0 until node.childCount) {
                val found = find(node.getChild(index))
                if (found != null) { node.recycle(); return found }
            }
            node.recycle()
            return null
        }
        return find(InstrumentationRegistry.getInstrumentation().uiAutomation.rootInActiveWindow)
    }

    // Accessibility services receive the unmerged tree. A control's spoken
    // label can be a child of its actionable node rather than that node itself.
    private fun nativeActionForLabel(label: String): AccessibilityNodeInfo? {
        var node = nativeNode { it.contentDescription?.toString() == label }
        while (node != null && !node.isClickable) {
            val parent = node.parent
            node.recycle()
            node = parent
        }
        return node
    }

    private fun nativeTreeDescription(): String {
        val nodes = mutableListOf<String>()
        nativeNode { node ->
            if (node.isClickable || node.contentDescription != null || node.text != null) {
                nodes += "class=${node.className}; text=${node.text}; description=${node.contentDescription}; clickable=${node.isClickable}; actions=${node.actionList}"
            }
            false
        }
        return nodes.joinToString("\n")
    }

    @Test
    fun contextualButtonsAndNativeCardActionsTargetTheNamedObject() {
        val entries = mutableStateOf(listOf(alpha, beta))
        val opened = mutableListOf<String>()
        val removed = mutableListOf<String>()
        compose.setContent {
            MaterialTheme {
                EnvironmentBrowser(entries.value, {}, { opened.add(it) }, {
                    removed.add(it)
                    entries.value = entries.value.filterNot { entry -> entry.name == it }
                })
            }
        }
        compose.onNodeWithText("2 objects").assertExists()
        compose.onNodeWithContentDescription("Open beta").performClick()
        compose.onNodeWithText("alpha").assert(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Button))
        compose.waitUntil(timeoutMillis = 5_000) {
            val node = nativeNode { info -> info.actionList.any { it.id == AccessibilityNodeInfo.ACTION_CLICK && it.label?.toString() == "Open alpha" } }
            val found = node != null
            node?.recycle()
            found
        }
        val card = nativeNode { info -> info.actionList.any { it.id == AccessibilityNodeInfo.ACTION_CLICK && it.label?.toString() == "Open alpha" } }
        assertNotNull(card)
        assertTrue(card!!.isClickable)
        assertTrue(card.performAction(AccessibilityNodeInfo.ACTION_CLICK))
        card.recycle()
        val openButton = nativeActionForLabel("Open beta")
        assertNotNull("Named open control must be a native clickable target: ${nativeTreeDescription()}", openButton)
        openButton?.recycle()
        val remove = nativeActionForLabel("Remove beta")
        assertNotNull("Named remove control must be a native clickable target: ${nativeTreeDescription()}", remove)
        assertTrue(remove!!.performAction(AccessibilityNodeInfo.ACTION_CLICK))
        remove.recycle()
        compose.onNodeWithText("1 object").assertExists()
        compose.onNode(isDialog()).assertDoesNotExist()
        compose.runOnIdle {
            assertEquals(listOf("beta", "alpha"), opened)
            assertEquals(listOf("beta"), removed)
        }
    }

    @Test
    fun existingRemoveStillFiresImmediatelyWithoutOpeningTheObject() {
        val removed = mutableListOf<String>()
        var opens = 0
        compose.setContent {
            MaterialTheme { EnvironmentBrowser(listOf(alpha), {}, { opens++ }, { removed.add(it) }) }
        }
        compose.onNodeWithText("Remove").performClick()
        compose.onNode(isDialog()).assertDoesNotExist()
        compose.runOnIdle {
            assertEquals(listOf("alpha"), removed)
            assertEquals(0, opens)
        }
    }

    @Test
    fun labelledFilterKeepsNameAndKindMatchingAndRefreshBehavior() {
        var refreshes = 0
        compose.setContent {
            MaterialTheme { EnvironmentBrowser(listOf(beta), { refreshes++ }, {}, {}) }
        }
        compose.onNodeWithText("1 object").assertExists()
        val search = compose.onNode(hasSetTextAction())
        search.performClick().performTextInput("DATA.FRAME")
        compose.onNodeWithContentDescription("Open beta").assertExists()
        search.performTextReplacement("missing")
        compose.onNodeWithText("No objects match your search.").assertExists()
        compose.onNodeWithContentDescription("Open beta").assertDoesNotExist()
        compose.onNodeWithText("Refresh").performClick()
        compose.runOnIdle { assertEquals(1, refreshes) }
    }

    @Test
    fun emptyEnvironmentKeepsExistingGuidanceAndZeroCount() {
        compose.setContent {
            MaterialTheme { EnvironmentBrowser(emptyList(), {}, {}, {}) }
        }
        compose.onNodeWithText("0 objects").assertExists()
        compose.onNodeWithText("Find an object").assertExists()
        compose.onNodeWithText("Run code to create variables. They will appear here.").assertExists()
    }
}
