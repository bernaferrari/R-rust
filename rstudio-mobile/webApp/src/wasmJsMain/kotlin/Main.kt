import com.rstudio.shared.EvaluationResult
import com.rstudio.shared.EnvironmentEntryModel
import com.rstudio.shared.PackageModel
import com.rstudio.shared.RSessionBackend
import com.rstudio.shared.WorkbenchCapabilities
import com.rstudio.shared.WorkbenchState
import kotlinx.browser.document
import kotlinx.browser.window
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.await
import kotlinx.coroutines.launch
import kotlin.js.JsAny
import kotlin.js.JsString
import kotlin.js.js
import org.w3c.dom.HTMLButtonElement
import org.w3c.dom.HTMLInputElement
import org.w3c.dom.HTMLTextAreaElement

private fun postMessageWebROptions(): JsAny = js("({ channelType: 3 })")

private class BrowserSessionBackend : RSessionBackend {
    private val webR = WebR(postMessageWebROptions())
    private var initialized = false

    override val capabilities = WorkbenchCapabilities(
        canExecuteR = true,
        canPersistFiles = true,
        canInstallPackages = true,
        runtimeLabel = "WebR WASM R runtime",
    )

    override suspend fun evaluate(code: String): EvaluationResult = try {
        ensureInitialized()
        val wrapped = "paste(capture.output(eval(parse(text = ${quoteR(code)}))), collapse = \"\\n\")"
        EvaluationResult(output = webR.evalRString(wrapped).await<JsString>().toString())
    } catch (error: Throwable) {
        EvaluationResult(error = error.toString())
    }

    override suspend fun inspect(name: String): EvaluationResult = try {
        ensureInitialized()
        val expression = "paste(capture.output(str(get(${quoteR(name)}, envir = .GlobalEnv))), collapse = \"\\n\")"
        EvaluationResult(output = webR.evalRString(expression).await<JsString>().toString())
    } catch (error: Throwable) {
        EvaluationResult(error = error.toString())
    }

    override suspend fun environment(): List<EnvironmentEntryModel> = try {
        ensureInitialized()
        val names = webR.evalRString("paste(ls(all.names = TRUE), collapse = \"\\n\")").await<JsString>().toString()
        names.lines().filter(String::isNotBlank).map { EnvironmentEntryModel(it, "object", "") }
    } catch (_: Throwable) {
        emptyList()
    }

    override suspend fun packages(): List<PackageModel> = try {
        ensureInitialized()
        val table = webR.evalRString("paste(capture.output(write.table(installed.packages()[, c(\"Package\", \"Version\", \"Title\", \"NeedsCompilation\")], sep = \"\\t\", row.names = FALSE, quote = FALSE)), collapse = \"\\n\")").await<JsString>().toString()
        table.lineSequence().drop(1).mapNotNull { line ->
            val fields = line.split('\t')
            fields.getOrNull(0)?.takeIf(String::isNotBlank)?.let {
                PackageModel(it, fields.getOrNull(1).orEmpty(), fields.getOrNull(2).orEmpty(), fields.getOrNull(3).equals("yes", true))
            }
        }.toList()
    } catch (_: Throwable) {
        emptyList()
    }

    override suspend fun installPackages(names: List<String>): EvaluationResult = try {
        ensureInitialized()
        webR.installPackages(names.joinToString(","))
            .await<JsAny>()
        EvaluationResult(output = "Installed: ${names.joinToString(", ")}")
    } catch (error: Throwable) {
        EvaluationResult(error = error.toString())
    }

    override suspend fun renderPlot(code: String): EvaluationResult = try {
        ensureInitialized()
        val wrapped = "svg(filename = \"/tmp/rport-plot.svg\", width = 8, height = 6); $code; dev.off(); paste(readLines(\"/tmp/rport-plot.svg\"), collapse = \"\\n\")"
        EvaluationResult(plotSvg = webR.evalRString(wrapped).await<JsString>().toString())
    } catch (error: Throwable) {
        EvaluationResult(error = error.toString())
    }
    override fun cancel() = Unit

    private suspend fun ensureInitialized() {
        if (!initialized) {
            webR.init().await<JsAny>()
            initialized = true
        }
    }
}

private fun quoteR(value: String): String = "\"" + value
    .replace("\\", "\\\\")
    .replace("\"", "\\\"")
    .replace("\n", "\\n") + "\""

private fun encodeUri(value: String): String = js("encodeURIComponent(value)")

fun main() {
    val backend = BrowserSessionBackend()
    val scope = MainScope()
    var state = WorkbenchState()
    val root = document.getElementById("root") ?: document.body
    root?.innerHTML = """
        <main class="shell">
          <header class="topbar"><div><strong>R Workbench</strong><span class="runtime">${backend.capabilities.runtimeLabel}</span></div><button id="run">Run</button></header>
          <section class="notice"><strong>Browser R runtime connected.</strong> WebR runs R in a dedicated WebAssembly worker. Drafts persist locally in this browser.</section>
          <section class="workspace-grid">
            <section class="workspace">
              <label for="editor">Editor</label>
              <textarea id="editor" spellcheck="false"></textarea>
              <div class="console-label">Console</div>
              <pre id="console"></pre>
              <div class="inline"><button id="plot">Render plot</button></div>
              <img id="plot-image" alt="Rendered R plot" hidden>
            </section>
            <aside class="side-panel">
              <div class="console-label">Environment</div>
              <pre id="environment">Loading…</pre>
              <label for="inspect-name">Inspect object</label>
              <div class="inline"><input id="inspect-name" placeholder="data frame or variable"><button id="inspect">Inspect</button></div>
              <pre id="inspection">Select an object to inspect it.</pre>
              <div class="console-label">Packages</div>
              <pre id="packages">Loading…</pre>
              <label for="install-name">Install WebR package</label>
              <div class="inline"><input id="install-name" placeholder="e.g. jsonlite"><button id="install">Install</button></div>
            </aside>
          </section>
        </main>
    """.trimIndent()

    val editor = document.getElementById("editor") as HTMLTextAreaElement
    val console = document.getElementById("console")!!
    val environment = document.getElementById("environment")!!
    val inspection = document.getElementById("inspection")!!
    val packages = document.getElementById("packages")!!
    val plotImage = document.getElementById("plot-image")!!
    editor.value = window.localStorage.getItem("r-workbench-code") ?: state.documents.first().code
    console.textContent = state.console
    val run = document.getElementById("run") as HTMLButtonElement
    val inspect = document.getElementById("inspect") as HTMLButtonElement
    val inspectName = document.getElementById("inspect-name") as HTMLInputElement
    val install = document.getElementById("install") as HTMLButtonElement
    val installName = document.getElementById("install-name") as HTMLInputElement
    val plot = document.getElementById("plot") as HTMLButtonElement
    editor.addEventListener("input", { window.localStorage.setItem("r-workbench-code", editor.value) })
    fun refreshSidePanels() {
        scope.launch {
            val objects = backend.environment()
            environment.textContent = if (objects.isEmpty()) "No objects" else objects.joinToString("\n") { "${it.name}  ${it.kind}" }
            val installed = backend.packages()
            packages.textContent = if (installed.isEmpty()) "No packages" else installed.joinToString("\n") { "${it.name} ${it.version}" }
        }
    }
    refreshSidePanels()
    run.addEventListener("click", {
        state = state.copy(isRunning = true, status = "Running…")
        console.textContent = "> ${editor.value}\nRunning…"
        scope.launch {
            val result = backend.evaluate(editor.value)
            console.textContent = "> ${editor.value}\n${result.output}${result.error?.let { "\nError: $it" } ?: ""}"
            state = state.copy(isRunning = false, status = "Ready")
            refreshSidePanels()
        }
    })
    inspect.addEventListener("click", {
        val name = inspectName.value.trim()
        if (name.isNotEmpty()) scope.launch { inspection.textContent = backend.inspect(name).output }
    })
    install.addEventListener("click", {
        val name = installName.value.trim()
        if (name.isNotEmpty()) scope.launch {
            val result = backend.installPackages(listOf(name))
            console.textContent = result.output + (result.error?.let { "\nError: $it" } ?: "")
            refreshSidePanels()
        }
    })
    plot.addEventListener("click", {
        scope.launch {
            val result = backend.renderPlot(editor.value)
            val svg = result.plotSvg
            if (svg != null) {
                plotImage.setAttribute("src", "data:image/svg+xml;charset=utf-8,${encodeUri(svg)}")
                plotImage.removeAttribute("hidden")
            }
            if (result.error != null) console.textContent = "Error: ${result.error}"
        }
    })
}
