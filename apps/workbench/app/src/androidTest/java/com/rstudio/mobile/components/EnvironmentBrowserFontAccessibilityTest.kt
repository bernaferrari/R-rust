package com.rstudio.mobile.components

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.CompositionLocalProvider
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
import com.rstudio.mobile.runtime.EnvEntry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class EnvironmentBrowserFontAccessibilityTest {
    @get:Rule val compose = createComposeRule()

    private fun assertInsideViewport(node: SemanticsNodeInteraction) {
        node.assertIsDisplayed()
        val bounds = node.getUnclippedBoundsInRoot()
        val root = compose.onRoot().getUnclippedBoundsInRoot()
        assertTrue("Label/control has no width: $bounds", bounds.right > bounds.left)
        assertTrue("Label/control outside viewport: $bounds vs $root", bounds.left >= root.left && bounds.right <= root.right && bounds.top >= root.top && bounds.bottom <= root.bottom)
    }

    private fun exercise(fontScale: Float, direction: LayoutDirection) {
        val calls = mutableListOf<String>()
        val name = "observations_by_treatment_group"
        val entry = EnvEntry(name, "data.frame", "3 rows, 2 columns")
        compose.setContent {
            val density = LocalDensity.current.density
            CompositionLocalProvider(LocalDensity provides Density(density, fontScale), LocalLayoutDirection provides direction) {
                MaterialTheme { EnvironmentBrowser(listOf(entry), { calls += "refresh" }, { calls += "open:$it" }, { calls += "remove:$it" }) }
            }
        }
        assertInsideViewport(compose.onNodeWithText("data.frame", useUnmergedTree = true))
        assertInsideViewport(compose.onNodeWithText(name, useUnmergedTree = true))
        val refresh = compose.onNodeWithText("Refresh")
        assertInsideViewport(refresh)
        refresh.assertWidthIsAtLeast(48.dp).assertHeightIsAtLeast(48.dp).performTouchInput { click() }
        val open = compose.onNodeWithContentDescription("Open $name")
        open.performScrollTo()
        assertInsideViewport(open)
        open.assertWidthIsAtLeast(48.dp).assertHeightIsAtLeast(48.dp).performTouchInput { click() }
        val remove = compose.onNodeWithContentDescription("Remove $name")
        remove.performScrollTo()
        assertInsideViewport(remove)
        remove.assertWidthIsAtLeast(48.dp).assertHeightIsAtLeast(48.dp).performTouchInput { click() }
        compose.runOnIdle { assertEquals(listOf("refresh", "open:$name", "remove:$name"), calls) }
    }

    @Test fun doubledFontKeepsObjectTypeAndActionsVisible() = exercise(2f, LayoutDirection.Ltr)
    @Test fun doubledFontRtlKeepsObjectTypeAndActionsVisible() = exercise(2f, LayoutDirection.Rtl)
    @Test fun defaultFontKeepsObjectTypeAndActionsVisible() = exercise(1f, LayoutDirection.Ltr)
}
