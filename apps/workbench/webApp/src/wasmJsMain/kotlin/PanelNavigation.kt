import kotlinx.browser.document
import org.w3c.dom.Document
import org.w3c.dom.Element
import org.w3c.dom.HTMLElement
import org.w3c.dom.events.KeyboardEvent

/** Owns the selection, ARIA, visibility, and keyboard invariants of the workbench tab set. */
internal class WorkbenchPanelNavigation(
    private val root: Document = document,
) {
    fun select(requestedPanel: String): String {
        val panel = requestedPanel.takeIf(PANEL_NAMES::contains) ?: PANEL_NAMES.first()
        PANEL_NAMES.forEach { name ->
            val isSelected = name == panel
            root.getElementById("panel-$name")?.let { content ->
                if (isSelected) content.removeAttribute("hidden") else content.setAttribute("hidden", "")
            }
            root.querySelector("[data-panel=\"$name\"]")?.let { tab ->
                tab.setAttribute("aria-selected", isSelected.toString())
                tab.setAttribute("tabindex", if (isSelected) "0" else "-1")
                tab.classList.toggle("active", isSelected)
            }
        }
        return panel
    }

    fun bind() {
        val tabs = root.querySelectorAll("[data-panel]").asElementList()
        tabs.forEachIndexed { index, tab ->
            tab.addEventListener("click", { select(panelName(tab)) })
            tab.addEventListener("keydown", { rawEvent ->
                val event = rawEvent as KeyboardEvent
                val targetIndex = keyboardTarget(index, tabs.lastIndex, event.key)
                    ?: return@addEventListener
                event.preventDefault()
                val target = tabs[targetIndex]
                select(panelName(target))
                (target as? HTMLElement)?.focus()
            })
        }
    }

    private fun panelName(tab: Element): String =
        tab.getAttribute("data-panel")?.takeIf(PANEL_NAMES::contains) ?: PANEL_NAMES.first()

    companion object {
        internal val PANEL_NAMES = listOf("environment", "data", "plots", "packages", "help")

        internal fun keyboardTarget(current: Int, last: Int, key: String): Int? = when (key) {
            "ArrowLeft" -> (current - 1 + last + 1) % (last + 1)
            "ArrowRight" -> (current + 1) % (last + 1)
            "Home" -> 0
            "End" -> last
            else -> null
        }
    }
}

private fun org.w3c.dom.NodeList.asElementList(): List<Element> =
    (0 until length).mapNotNull { item(it) as? Element }
