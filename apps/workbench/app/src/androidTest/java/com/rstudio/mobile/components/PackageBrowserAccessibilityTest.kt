package com.rstudio.mobile.components

import android.view.KeyEvent
import android.view.accessibility.AccessibilityNodeInfo
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertWidthIsAtLeast
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.rport.uniffi.PackageInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@Suppress("DEPRECATION")
@RunWith(AndroidJUnit4::class)
class PackageBrowserAccessibilityTest {
    @get:Rule val compose = createComposeRule()
    private fun pkg(name: String, title: String = "", removable: Boolean = true) = PackageInfo(
        name, "1.0", title, "", "MIT", "", "", "", false, "/packages/$name",
        if (removable) "/home/R/library" else "/bundled/library",
    )
    private val alpha = pkg("alpha", "Numeric tools")
    private val beta = pkg("beta", "Text tools")

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

    @Test fun nativeActionsIdentifyExactPackageAndLoadedButtonIsDisabled() {
        val loaded = mutableStateOf(setOf("alpha"))
        val loads = mutableListOf<String>()
        val removals = mutableListOf<PackageInfo>()
        compose.setContent { MaterialTheme {
            PackageBrowser(listOf(alpha, beta), loaded.value, {}, { loads += it; loaded.value += it }, onRemove = { removals += it })
        } }
        compose.onNodeWithContentDescription("alpha is loaded").assertIsNotEnabled()
        val load = compose.onNodeWithContentDescription("Load beta")
        load.assertHeightIsAtLeast(48.dp).assertWidthIsAtLeast(48.dp)
        compose.waitUntil(5000) { nativeButton("Load beta")?.let { it.recycle(); true } ?: false }
        nativeButton("Load beta")!!.let { assertTrue(it.performAction(AccessibilityNodeInfo.ACTION_CLICK)); it.recycle() }
        compose.onNodeWithContentDescription("beta is loaded").assertIsNotEnabled()
        compose.onNodeWithContentDescription("Remove beta").assertHeightIsAtLeast(48.dp).assertWidthIsAtLeast(48.dp)
        nativeButton("Remove beta")!!.let { assertTrue(it.performAction(AccessibilityNodeInfo.ACTION_CLICK)); it.recycle() }
        compose.runOnIdle { assertEquals(listOf("beta"), loads); assertEquals(listOf(beta), removals) }
    }

    @Test fun searchByTitleKeepsHeaderCallbacksAndRemovalEligibility() {
        var installs = 0; var refreshes = 0
        compose.setContent { MaterialTheme {
            PackageBrowser(listOf(alpha, pkg("stock", "Bundled tools", false)), emptySet(), { refreshes++ }, {}, { installs++ })
        } }
        compose.onNodeWithText("Install ZIP").performClick()
        compose.onNodeWithText("Refresh").performClick()
        compose.onNodeWithContentDescription("Remove stock").assertDoesNotExist()
        compose.onNodeWithText("Find a package").performClick().performTextInput("BUNDLED")
        compose.onNodeWithText("stock").assertExists()
        compose.onNodeWithText("alpha").assertDoesNotExist()
        compose.runOnIdle { assertEquals(1, installs); assertEquals(1, refreshes) }
    }

    @Test fun keyboardTabCanReachAndActivateContextualLoad() {
        val loads = mutableListOf<String>()
        compose.setContent { MaterialTheme { PackageBrowser(listOf(beta), emptySet(), {}, { loads += it }) } }
        val load = compose.onNodeWithContentDescription("Load beta")
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        var reached = false
        for (attempt in 0 until 12) {
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_TAB)
            compose.waitForIdle()
            reached = try { load.assertIsFocused(); true } catch (_: AssertionError) { false }
            if (reached) break
        }
        assertTrue("Hardware Tab navigation must reach package Load", reached)
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ENTER)
        compose.runOnIdle { assertEquals(listOf("beta"), loads) }
    }
}
