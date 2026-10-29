package com.rstudio.mobile.ui

import android.view.KeyEvent
import android.view.accessibility.AccessibilityNodeInfo
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.assertWidthIsAtLeast
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performScrollTo
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
class WorkbenchNavigationAccessibilityTest {
    @get:Rule val compose = createComposeRule()

    private fun nativeTab(label: String): AccessibilityNodeInfo? {
        fun find(node: AccessibilityNodeInfo?): AccessibilityNodeInfo? {
            if (node == null) return null
            if (node.text?.toString() == label) return node
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
        compose.waitUntil(5000) { nativeTab(label)?.let { it.recycle(); true } ?: false }
        nativeTab(label)!!.let { assertTrue(it.performAction(AccessibilityNodeInfo.ACTION_CLICK)); it.recycle() }
    }

    @Test fun compactDestinationsKeepNativeNamesSelectionAndExactCallbacks() {
        val selected = mutableStateOf(Destination.Editor)
        val calls = mutableListOf<Destination>()
        compose.setContent { MaterialTheme {
            WorkbenchNavigationBar(selected.value) { calls += it; selected.value = it }
        } }
        compose.onNodeWithText("Editor").assertIsSelected()
        for ((label, destination) in listOf("Console" to Destination.Console, "Inspect" to Destination.Inspect, "Files" to Destination.Files, "Editor" to Destination.Editor)) {
            compose.onNodeWithText(label).assertHeightIsAtLeast(48.dp).assertWidthIsAtLeast(48.dp)
                .assert(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Tab))
            activate(label)
            compose.onNodeWithText(label).assertIsSelected()
            compose.runOnIdle { assertEquals(destination, selected.value) }
        }
        compose.runOnIdle { assertEquals(listOf(Destination.Console, Destination.Inspect, Destination.Files, Destination.Editor), calls) }
    }

    @Test fun inspectorCanScrollToHelpThenKeyboardNavigateToPackages() {
        val selected = mutableIntStateOf(0)
        val calls = mutableListOf<Int>()
        compose.setContent { MaterialTheme { InspectorTabs(selected.intValue) { calls += it; selected.intValue = it } } }
        compose.onNodeWithText("Data").assertIsSelected()
        compose.onNodeWithText("Help").performScrollTo().assertHeightIsAtLeast(48.dp).assertWidthIsAtLeast(48.dp)
        activate("Help")
        compose.onNodeWithText("Help").assertIsSelected()
        val packages = compose.onNodeWithText("Packages")
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        var reached = false
        for (attempt in 0 until 12) {
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_TAB)
            compose.waitForIdle()
            reached = try { packages.assertIsFocused(); true } catch (_: AssertionError) { false }
            if (reached) break
        }
        assertTrue("Hardware Tab must reach inspector Packages", reached)
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ENTER)
        packages.assertIsSelected()
        compose.runOnIdle { assertEquals(listOf(4, 3), calls) }
    }

    @Test fun expandedModeTabsPreserveBooleanRoutingAndSelection() {
        val files = mutableStateOf(false)
        val calls = mutableListOf<Boolean>()
        compose.setContent { MaterialTheme { WorkspaceModeTabs(files.value) { calls += it; files.value = it } } }
        compose.onNodeWithText("Inspect").assertIsSelected().assertHeightIsAtLeast(48.dp)
        activate("Files")
        compose.onNodeWithText("Files").assertIsSelected().assertHeightIsAtLeast(48.dp)
        activate("Inspect")
        compose.onNodeWithText("Inspect").assertIsSelected()
        compose.runOnIdle { assertEquals(listOf(true, false), calls) }
    }
}
