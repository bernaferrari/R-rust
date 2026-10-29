package com.rstudio.mobile.components

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertWidthIsAtLeast
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.click
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.rport.uniffi.PackageInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class PackageBrowserFontAccessibilityTest {
    @get:Rule val compose = createComposeRule()

    private fun assertInsideViewport(node: SemanticsNodeInteraction) {
        node.assertIsDisplayed().assertWidthIsAtLeast(48.dp).assertHeightIsAtLeast(48.dp)
        val bounds = node.getUnclippedBoundsInRoot()
        val root = compose.onRoot().getUnclippedBoundsInRoot()
        assertTrue("Control extends left of viewport: $bounds vs $root", bounds.left >= root.left)
        assertTrue("Control extends right of viewport: $bounds vs $root", bounds.right <= root.right)
        assertTrue("Control extends below viewport: $bounds vs $root", bounds.bottom <= root.bottom)
    }

    private fun exercise(fontScale: Float, direction: LayoutDirection) {
        val calls = mutableListOf<String>()
        val loaded = mutableStateOf(emptySet<String>())
        val pkg = PackageInfo("analysis.tools", "1.0", "Tools for statistical analysis", "", "MIT", "", "", "", false, "/packages/analysis.tools", "/home/R/library")
        compose.setContent {
            val density = LocalDensity.current.density
            CompositionLocalProvider(LocalDensity provides Density(density, fontScale), LocalLayoutDirection provides direction) {
                MaterialTheme { PackageBrowser(listOf(pkg), loaded.value,
                    { calls += "refresh" }, { calls += it; loaded.value += it }, { calls += "install" }, { calls += "remove:${it.name}" }) }
            }
        }
        val install = compose.onNodeWithText("Install ZIP")
        val refresh = compose.onNodeWithText("Refresh")
        assertInsideViewport(install); assertInsideViewport(refresh)
        install.performTouchInput { click() }
        refresh.performTouchInput { click() }
        val load = compose.onNodeWithContentDescription("Load analysis.tools")
        load.performScrollTo()
        assertInsideViewport(load)
        compose.onNodeWithText("analysis.tools").assertIsDisplayed()
        load.performTouchInput { click() }
        val remove = compose.onNodeWithContentDescription("Remove analysis.tools")
        remove.performScrollTo()
        assertInsideViewport(remove)
        remove.performTouchInput { click() }
        compose.runOnIdle { assertEquals(listOf("install", "refresh", "analysis.tools", "remove:analysis.tools"), calls) }
    }

    @Test fun doubledFontKeepsEveryActionVisibleAndTouchable() = exercise(2f, LayoutDirection.Ltr)
    @Test fun doubledFontRtlKeepsEveryActionVisibleAndTouchable() = exercise(2f, LayoutDirection.Rtl)
    @Test fun defaultFontKeepsExistingActionsTouchable() = exercise(1f, LayoutDirection.Ltr)
}
