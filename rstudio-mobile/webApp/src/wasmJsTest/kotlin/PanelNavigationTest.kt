import kotlinx.browser.document
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class PanelNavigationTest {
    @AfterTest
    fun cleanUp() {
        document.body?.innerHTML = ""
    }

    @Test
    fun selectingEachTabLeavesExactlyOneVisiblePanel() {
        installFixture()

        val navigation = WorkbenchPanelNavigation()
        WorkbenchPanelNavigation.PANEL_NAMES.forEach { selected ->
            assertEquals(selected, navigation.select(selected))
            val visible = WorkbenchPanelNavigation.PANEL_NAMES.filter { name ->
                !document.getElementById("panel-$name")!!.hasAttribute("hidden")
            }
            assertEquals(listOf(selected), visible)

            val selectedTab = document.querySelector("[data-panel=\"$selected\"]")!!
            assertEquals("true", selectedTab.getAttribute("aria-selected"))
            assertEquals("0", selectedTab.getAttribute("tabindex"))
            assertTrue(selectedTab.classList.contains("active"))
        }
    }

    @Test
    fun unknownPanelFallsBackToEnvironment() {
        installFixture()

        assertEquals("environment", WorkbenchPanelNavigation().select("missing"))
        assertFalse(document.getElementById("panel-environment")!!.hasAttribute("hidden"))
        assertTrue(document.getElementById("panel-data")!!.hasAttribute("hidden"))
    }

    @Test
    fun arrowAndBoundaryKeysWrapPredictably() {
        assertEquals(4, WorkbenchPanelNavigation.keyboardTarget(0, 4, "ArrowLeft"))
        assertEquals(0, WorkbenchPanelNavigation.keyboardTarget(4, 4, "ArrowRight"))
        assertEquals(0, WorkbenchPanelNavigation.keyboardTarget(3, 4, "Home"))
        assertEquals(4, WorkbenchPanelNavigation.keyboardTarget(1, 4, "End"))
        assertEquals(null, WorkbenchPanelNavigation.keyboardTarget(1, 4, "Enter"))
    }

    private fun installFixture() {
        document.body!!.innerHTML = WorkbenchPanelNavigation.PANEL_NAMES
            .joinToString("") { name ->
                "<button data-panel=\"$name\"></button><section id=\"panel-$name\"></section>"
            }
    }
}
