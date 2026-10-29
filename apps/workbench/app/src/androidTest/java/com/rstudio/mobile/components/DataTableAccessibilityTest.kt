package com.rstudio.mobile.components

import android.view.accessibility.AccessibilityNodeInfo
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertHasNoClickAction
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.rstudio.mobile.runtime.DataTable
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@Suppress("DEPRECATION")
@RunWith(AndroidJUnit4::class)
class DataTableAccessibilityTest {
    @get:Rule
    val compose = createComposeRule()

    private fun sortActionNode(): AccessibilityNodeInfo? {
        fun find(node: AccessibilityNodeInfo?): AccessibilityNodeInfo? {
            if (node == null) return null
            if (node.contentDescription?.toString() == "Sort by value") return node
            for (index in 0 until node.childCount) {
                val found = find(node.getChild(index))
                if (found != null) { node.recycle(); return found }
            }
            node.recycle()
            return null
        }
        var node = find(InstrumentationRegistry.getInstrumentation().uiAutomation.rootInActiveWindow)
        while (node != null && !node.isClickable) {
            val parent = node.parent
            node.recycle()
            node = parent
        }
        return node
    }

    @Test
    fun sortingHasUsableTargetsStateAndNativeNextAction() {
        val table = DataTable("numbers", listOf("value"), listOf(listOf("20"), listOf("3")), 2)
        compose.setContent { MaterialTheme { DataTableView(table) } }
        val header = compose.onNodeWithContentDescription("Sort by value")
        header.assertHeightIsAtLeast(48.dp).assert(
            SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Not sorted")
        )
        compose.onNodeWithContentDescription("Row number").assertHasNoClickAction()
        header.performClick()
        header.assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Sorted ascending"))
        assertTrue(compose.onNodeWithText("3").getUnclippedBoundsInRoot().top.value < compose.onNodeWithText("20").getUnclippedBoundsInRoot().top.value)
        compose.waitUntil(timeoutMillis = 5_000) {
            val node = sortActionNode()
            val found = node != null
            node?.recycle()
            found
        }
        val action = sortActionNode()
        assertNotNull(action)
        assertTrue(action!!.actionList.any { it.id == AccessibilityNodeInfo.ACTION_CLICK && it.label?.toString() == "Sort value descending" })
        assertTrue(action.performAction(AccessibilityNodeInfo.ACTION_CLICK))
        action.recycle()
        header.assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Sorted descending"))
        assertTrue(compose.onNodeWithText("20").getUnclippedBoundsInRoot().top.value < compose.onNodeWithText("3").getUnclippedBoundsInRoot().top.value)
    }

    @Test
    fun loadingFilteringAndExportCallbacksKeepTheirBehavior() {
        val table = mutableStateOf(DataTable("preview", listOf("value"), listOf(listOf("seven")), 2))
        var exports = 0
        var loads = 0
        compose.setContent {
            MaterialTheme {
                DataTableView(table.value, { exports++ }, {
                    loads++
                    table.value = table.value.copy(rows = listOf(listOf("seven"), listOf("eight")))
                })
            }
        }
        compose.onNodeWithText("Loaded 1 of 2 rows").assertExists()
        compose.onNodeWithText("Export CSV").performClick()
        compose.onNodeWithText("Load more rows").performClick()
        compose.onNodeWithText("2 rows").assertExists()
        compose.onNodeWithText("Load more rows").assertDoesNotExist()
        compose.onNode(hasSetTextAction()).performClick().performTextInput("EIGHT")
        compose.onNodeWithText("eight").assertExists()
        compose.onNodeWithText("seven").assertDoesNotExist()
        compose.runOnIdle { assertEquals(1, exports); assertEquals(1, loads) }
    }

    @Test
    fun emptyGuidanceAndSingularZeroCountsAreLocalizable() {
        val table = mutableStateOf<DataTable?>(null)
        compose.setContent { MaterialTheme { DataTableView(table.value) } }
        compose.onNodeWithText("No table result").assertExists()
        compose.onNodeWithText("Import a CSV or evaluate a data.frame to inspect rows and columns here.").assertExists()
        compose.runOnIdle { table.value = DataTable("single", listOf("value"), listOf(listOf("7")), 1) }
        compose.onNodeWithText("1 row").assertExists()
        compose.runOnIdle { table.value = DataTable("zero", listOf("value"), emptyList(), 0) }
        compose.onNodeWithText("0 rows").assertExists()
    }
}
