import com.rstudio.shared.EvaluationResult
import com.rstudio.shared.EnvironmentEntryModel
import com.rstudio.shared.PackageModel
import com.rstudio.shared.RSessionBackend
import com.rstudio.shared.WorkbenchCapabilities
import com.rstudio.shared.WorkbenchState
import kotlinx.browser.document
import kotlinx.browser.window
import org.w3c.dom.HTMLButtonElement
import org.w3c.dom.HTMLTextAreaElement

private class BrowserSessionBackend : RSessionBackend {
    override val capabilities = WorkbenchCapabilities(
        canExecuteR = false,
        canPersistFiles = true,
        canInstallPackages = false,
        runtimeLabel = "WASM browser shell",
    )

    override suspend fun evaluate(code: String): EvaluationResult = EvaluationResult(
        valueSummary = "Browser R runtime is not linked yet",
        error = "The shared UI is ready, but full r-embed WASM execution is a separate backend milestone.",
    )

    override suspend fun inspect(name: String): EvaluationResult = evaluate(name)
    override suspend fun environment(): List<EnvironmentEntryModel> = emptyList()
    override suspend fun packages(): List<PackageModel> = emptyList()
    override fun cancel() = Unit
}

fun main() {
    val backend = BrowserSessionBackend()
    var state = WorkbenchState()
    val root = document.getElementById("root") ?: document.body
    root?.innerHTML = """
        <main class="shell">
          <header class="topbar"><div><strong>R Workbench</strong><span class="runtime">${backend.capabilities.runtimeLabel}</span></div><button id="run">Run</button></header>
          <section class="notice"><strong>Browser target connected.</strong> The shared workbench contract and local draft persistence are active. Full local R execution will attach through the WASM session adapter.</section>
          <section class="workspace">
            <label for="editor">Editor</label>
            <textarea id="editor" spellcheck="false"></textarea>
            <div class="console-label">Console</div>
            <pre id="console"></pre>
          </section>
        </main>
    """.trimIndent()

    val editor = document.getElementById("editor") as HTMLTextAreaElement
    val console = document.getElementById("console")!!
    editor.value = window.localStorage.getItem("r-workbench-code") ?: state.documents.first().code
    console.textContent = state.console
    val run = document.getElementById("run") as HTMLButtonElement
    editor.addEventListener("input", { window.localStorage.setItem("r-workbench-code", editor.value) })
    run.addEventListener("click", {
        state = state.copy(isRunning = true, status = "Running…")
        console.textContent = "> ${editor.value}\nRunning…"
        window.setTimeout({
            console.textContent = "> ${editor.value}\n${backend.capabilities.runtimeLabel}: R evaluator not linked"
            state = state.copy(isRunning = false, status = "Ready")
            null
        }, 0)
    })
}
