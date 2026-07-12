import com.rstudio.shared.DataFrameModel
import com.rstudio.shared.EvaluationResult
import com.rstudio.shared.EnvironmentEntryModel
import com.rstudio.shared.PackageModel
import com.rstudio.shared.RSessionBackend
import com.rstudio.shared.WorkbenchCapabilities
import kotlinx.browser.document
import kotlinx.browser.window
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.await
import kotlinx.coroutines.launch
import kotlin.js.JsAny
import kotlin.js.JsString
import kotlin.js.Promise
import kotlin.js.js
import org.w3c.dom.Element
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

    override suspend fun evaluate(code: String): EvaluationResult = runR(
        "paste(capture.output(eval(parse(text = ${quoteR(code)}))), collapse = \"\\n\")"
    )

    override suspend fun inspect(name: String): EvaluationResult = runR(
        "paste(capture.output(str(get(${quoteR(name)}, envir = .GlobalEnv))), collapse = \"\\n\")"
    )

    override suspend fun environment(): List<EnvironmentEntryModel> = try {
        val text = evalString(
            "paste(vapply(ls(all.names = TRUE), function(n) paste(n, typeof(get(n, envir = .GlobalEnv)), sep = \"\\t\"), character(1)), collapse = \"\\n\")"
        )
        text.lines().filter(String::isNotBlank).map { line ->
            val fields = line.split('\t', limit = 2)
            EnvironmentEntryModel(fields.first(), fields.getOrNull(1) ?: "object", "")
        }
    } catch (_: Throwable) {
        emptyList()
    }

    override suspend fun packages(): List<PackageModel> = try {
        val table = evalString(
            "paste(capture.output(write.table(installed.packages()[, c(\"Package\", \"Version\", \"Title\", \"NeedsCompilation\")], sep = \"\\t\", row.names = FALSE, quote = FALSE)), collapse = \"\\n\")"
        )
        table.lineSequence().drop(1).mapNotNull { line ->
            val fields = line.split('\t')
            fields.firstOrNull()?.takeIf(String::isNotBlank)?.let {
                PackageModel(it, fields.getOrNull(1).orEmpty(), fields.getOrNull(2).orEmpty(), fields.getOrNull(3).equals("yes", true))
            }
        }.toList()
    } catch (_: Throwable) {
        emptyList()
    }

    override suspend fun installPackages(names: List<String>): EvaluationResult = try {
        ensureInitialized()
        webR.installPackages(names.joinToString(",")).await<JsAny>()
        EvaluationResult(output = "Installed: ${names.joinToString(", ")}")
    } catch (error: Throwable) {
        EvaluationResult(error = error.toString())
    }

    override suspend fun loadPackage(name: String): EvaluationResult = runR(
        "paste(capture.output(suppressPackageStartupMessages(library(${quoteR(name)}))), collapse = \"\\n\")"
    )

    override suspend fun help(topic: String): EvaluationResult = runR(
        "paste(c(\"Matching R topics:\", utils::apropos(${quoteR(topic)})), collapse = \"\\n\")"
    )

    override suspend fun dataFrame(name: String, offset: Int, limit: Int): DataFrameModel? {
        return try {
            val text = evalString(
                "local({ df <- as.data.frame(get(${quoteR(name)}, envir = .GlobalEnv)); n <- nrow(df); start <- ${offset.coerceAtLeast(0)}; end <- min(n, start + ${limit.coerceIn(1, 500)}); rows <- if (n == 0 || start >= n) character() else vapply(seq.int(start + 1, end), function(i) { values <- unlist(df[i, , drop = FALSE], use.names = FALSE); values[is.na(values)] <- \"\"; paste(gsub(\"[\\t\\r\\n]\", \" \", as.character(values)), collapse = \"\\t\") }, character(1)); paste(c(as.character(n), paste(names(df), collapse = \"\\t\"), rows), collapse = \"\\n\") })"
            )
            val lines = text.lines()
            if (lines.size < 2) return null
            val total = lines.first().toIntOrNull() ?: return null
            val columns = lines[1].split('\t')
            val rows = lines.drop(2).filter(String::isNotBlank).map { it.split('\t') }
            DataFrameModel(name, columns, rows, total, offset)
        } catch (_: Throwable) {
            null
        }
    }

    override suspend fun renderPlot(code: String): EvaluationResult = runR(
        "svg(filename = \"/tmp/rport-plot.svg\", width = 8, height = 6); ${code}; dev.off(); paste(readLines(\"/tmp/rport-plot.svg\"), collapse = \"\\n\")",
        asPlot = true,
    )

    override fun cancel() = Unit

    private suspend fun runR(expression: String, asPlot: Boolean = false): EvaluationResult = try {
        val value = evalString(expression)
        if (asPlot) EvaluationResult(plotSvg = value) else EvaluationResult(output = value)
    } catch (error: Throwable) {
        EvaluationResult(error = error.toString())
    }

    private suspend fun evalString(expression: String): String {
        ensureInitialized()
        return webR.evalRString(expression).await<JsString>().toString()
    }

    private suspend fun ensureInitialized() {
        if (!initialized) {
            webR.init().await<JsAny>()
            initialized = true
        }
    }
}

private data class BrowserDocument(
    val id: String,
    val name: String,
    val code: String,
    val dirty: Boolean = false,
)

private fun quoteR(value: String): String = "\"" + value
    .replace("\\", "\\\\")
    .replace("\"", "\\\"")
    .replace("\r", "\\r")
    .replace("\n", "\\n") + "\""

private fun escapeHtml(value: String): String = value
    .replace("&", "&amp;")
    .replace("<", "&lt;")
    .replace(">", "&gt;")
    .replace("\"", "&quot;")
    .replace("'", "&#39;")

private fun readFileText(file: JsAny): Promise<JsString> = js("file.text()")

private fun downloadText(name: String, content: String, mime: String) {
    js("""(() => { const blob = new Blob([content], {type: mime}); const url = URL.createObjectURL(blob); const a = document.createElement('a'); a.href = url; a.download = name; a.click(); URL.revokeObjectURL(url); })()""")
}

private fun newDocumentId(): String = js("'doc-' + Date.now().toString() + '-' + Math.random().toString().substring(2)")

private fun restoredDocuments(): MutableList<BrowserDocument> {
    val index = window.localStorage.getItem("r-workbench-doc-index")
    if (index.isNullOrBlank()) return mutableListOf(BrowserDocument("untitled", "untitled.R", "# R Workbench\n"))
    val restored = index.split('\u0001').mapNotNull { item ->
        val fields = item.split('\u0002', limit = 2)
        if (fields.size != 2) return@mapNotNull null
        BrowserDocument(
            id = fields[0],
            name = fields[1],
            code = window.localStorage.getItem("r-workbench-doc-${fields[0]}") ?: "",
        )
    }.toMutableList()
    return restored.ifEmpty { mutableListOf(BrowserDocument("untitled", "untitled.R", "# R Workbench\n")) }
}

private fun persistDocuments(documents: List<BrowserDocument>) {
    window.localStorage.setItem(
        "r-workbench-doc-index",
        documents.joinToString("\u0001") { "${it.id}\u0002${it.name.replace('\u0001', '_').replace('\u0002', '_')}" },
    )
    documents.forEach { window.localStorage.setItem("r-workbench-doc-${it.id}", it.code) }
}

fun main() {
    val backend = BrowserSessionBackend()
    val scope = MainScope()
    val documents = restoredDocuments()
    var activeDocumentId = documents.first().id
    var consoleHistory = window.localStorage.getItem("r-workbench-history")?.split('\u0001').orEmpty().toMutableList()
    var historyCursor = consoleHistory.size
    var lastPlotSvg: String? = null

    val root = document.getElementById("root") ?: document.body
    root?.innerHTML = """
        <main class="shell">
          <header class="topbar">
            <div class="brand"><strong>R Workbench</strong><span class="runtime">${backend.capabilities.runtimeLabel}</span></div>
            <div class="toolbar"><button id="new-document">New</button><button id="open-document">Open</button><button id="save-document">Save</button><button id="run" class="primary">Run</button><button id="stop" disabled>Stop</button></div>
          </header>
          <section class="notice"><strong>Browser R runtime connected.</strong> WebR runs R in a dedicated WebAssembly worker. Scripts, history, and project metadata persist in this browser.</section>
          <input id="file-input" type="file" accept=".R,.r,.Rmd,.txt,.csv,.tsv,.json" hidden>
          <section class="workspace-grid">
            <section class="workspace">
              <nav id="document-tabs" class="document-tabs" aria-label="Open documents"></nav>
              <div class="editor-toolbar"><span id="status">Ready</span><span class="spacer"></span><button id="run-selection">Run selection</button><button id="run-file">Run file</button><button id="render-plot">Plot</button><button id="report">Report</button></div>
              <textarea id="editor" spellcheck="false" aria-label="R script editor"></textarea>
              <section class="console-section"><div class="section-heading"><strong>Console</strong><button id="clear-console">Clear</button></div><pre id="console"></pre><div class="console-input"><span>&gt;</span><input id="console-command" placeholder="Type an R command and press Enter" autocomplete="off"><button id="console-run">Run</button></div></section>
            </section>
            <aside class="side-panel">
              <nav class="panel-tabs" aria-label="Workbench panels"><button data-panel="environment" class="active">Environment</button><button data-panel="data">Data</button><button data-panel="plots">Plots</button><button data-panel="packages">Packages</button><button data-panel="help">Help</button></nav>
              <section id="panel-environment" class="panel"><div class="section-heading"><strong>Environment</strong><button id="refresh-environment">Refresh</button></div><div id="environment-list" class="list">Loading…</div><div class="inline"><input id="inspect-name" placeholder="Object name"><button id="inspect">Inspect</button></div><pre id="inspection">Select an object to inspect it.</pre></section>
              <section id="panel-data" class="panel" hidden><div class="section-heading"><strong id="data-title">Data viewer</strong><button id="refresh-data">Refresh</button></div><div id="data-viewer" class="table-wrap">Select a data frame in Environment.</div><div class="inline"><button id="previous-page">Previous</button><button id="next-page">Next</button></div></section>
              <section id="panel-plots" class="panel" hidden><div class="section-heading"><strong>Plots</strong><button id="download-plot">Download</button></div><div id="plot-gallery" class="plot-gallery">Run plotting code or press Plot.</div></section>
              <section id="panel-packages" class="panel" hidden><div class="section-heading"><strong>Packages</strong><button id="refresh-packages">Refresh</button></div><div class="inline"><input id="install-name" placeholder="Package name"><button id="install">Install</button></div><div id="packages-list" class="list">Loading…</div></section>
              <section id="panel-help" class="panel" hidden><div class="inline"><input id="help-topic" placeholder="Function or topic"><button id="help">Search</button></div><pre id="help-result">Search R documentation.</pre></section>
            </aside>
          </section>
          <footer class="statusbar"><span id="runtime-status">R session ready</span><span>Local browser workspace</span></footer>
        </main>
    """.trimIndent()

    val editor = document.getElementById("editor") as HTMLTextAreaElement
    val console = document.getElementById("console")!!
    val status = document.getElementById("status")!!
    val runtimeStatus = document.getElementById("runtime-status")!!
    val tabs = document.getElementById("document-tabs")!!
    val fileInput = document.getElementById("file-input") as HTMLInputElement
    val run = document.getElementById("run") as HTMLButtonElement
    val stop = document.getElementById("stop") as HTMLButtonElement
    val command = document.getElementById("console-command") as HTMLInputElement
    val inspection = document.getElementById("inspection")!!
    val environmentList = document.getElementById("environment-list")!!
    val dataViewer = document.getElementById("data-viewer")!!
    val plotGallery = document.getElementById("plot-gallery")!!
    val packagesList = document.getElementById("packages-list")!!
    val helpResult = document.getElementById("help-result")!!
    var selectedDataName: String? = null
    var dataOffset = 0

    fun activeDocument(): BrowserDocument = documents.first { it.id == activeDocumentId }
    lateinit var renderTabs: () -> Unit
    lateinit var inspectObject: (String) -> Unit
    fun syncActiveDocument(markDirty: Boolean = activeDocument().dirty) {
        val current = activeDocument()
        val updated = current.copy(code = editor.value, dirty = markDirty)
        documents[documents.indexOfFirst { it.id == activeDocumentId }] = updated
        persistDocuments(documents)
        renderTabs()
    }
    renderTabs = {
        tabs.innerHTML = documents.joinToString("") { doc ->
            "<button class=\"tab ${if (doc.id == activeDocumentId) "selected" else ""}\" data-document=\"${escapeHtml(doc.id)}\">${escapeHtml(doc.name)}${if (doc.dirty) " •" else ""}</button>"
        }
        tabs.querySelectorAll("[data-document]").asElements().forEach { element ->
            element.addEventListener("click", {
                val id = element.getAttribute("data-document") ?: return@addEventListener
                syncActiveDocument()
                activeDocumentId = id
                editor.value = activeDocument().code
                status.textContent = "Editing ${activeDocument().name}"
                renderTabs()
            })
        }
    }
    fun appendConsole(text: String) {
        val existing = console.textContent.orEmpty()
        console.textContent = (if (existing.isBlank()) text else "$existing\n$text").takeLast(20000)
        console.scrollTop = console.scrollHeight.toDouble()
    }
    fun setBusy(busy: Boolean, message: String) {
        run.disabled = busy
        stop.disabled = !busy
        status.textContent = message
        runtimeStatus.textContent = message
    }
    fun showPanel(panel: String) {
        listOf("environment", "data", "plots", "packages", "help").forEach { name ->
            document.getElementById("panel-$name")?.setAttribute("hidden", if (name == panel) "false" else "true")
        }
        document.querySelectorAll("[data-panel]").asElements().forEach { button ->
            if (button.getAttribute("data-panel") == panel) button.classList.add("active") else button.classList.remove("active")
        }
    }
    fun renderEnvironment(items: List<EnvironmentEntryModel>) {
        environmentList.innerHTML = if (items.isEmpty()) "<span class=\"muted\">No objects</span>" else items.joinToString("") { item ->
            "<button class=\"object-row\" data-object=\"${escapeHtml(item.name)}\"><strong>${escapeHtml(item.name)}</strong><span>${escapeHtml(item.kind)}</span></button>"
        }
        environmentList.querySelectorAll("[data-object]").asElements().forEach { row ->
            row.addEventListener("click", {
                val name = row.getAttribute("data-object") ?: return@addEventListener
                inspectObject(name)
            })
        }
    }
    fun renderPackages(items: List<PackageModel>) {
        packagesList.innerHTML = if (items.isEmpty()) "<span class=\"muted\">No packages found</span>" else items.joinToString("") { item ->
            "<div class=\"package-row\"><div><strong>${escapeHtml(item.name)}</strong><span>${escapeHtml(item.version)}</span></div><button data-load-package=\"${escapeHtml(item.name)}\">Load</button></div>"
        }
        packagesList.querySelectorAll("[data-load-package]").asElements().forEach { button ->
            button.addEventListener("click", {
                val name = button.getAttribute("data-load-package") ?: return@addEventListener
                scope.launch {
                    setBusy(true, "Loading $name…")
                    val result = backend.loadPackage(name)
                    appendConsole(result.output.ifBlank { "Loaded $name" } + (result.error?.let { "\nError: $it" } ?: ""))
                    setBusy(false, "Ready")
                }
            })
        }
    }
    fun renderData(model: DataFrameModel?) {
        if (model == null) {
            dataViewer.textContent = "Select a data frame in Environment."
            return
        }
        document.getElementById("data-title")?.textContent = "${model.name} · ${model.totalRows} rows"
        val head = model.columns.joinToString("") { "<th>${escapeHtml(it)}</th>" }
        val rows = model.rows.joinToString("") { row -> "<tr>${row.joinToString("") { "<td>${escapeHtml(it)}</td>" }}</tr>" }
        dataViewer.innerHTML = "<table><thead><tr>$head</tr></thead><tbody>$rows</tbody></table>"
    }
    inspectObject = { name: String ->
        scope.launch {
            setBusy(true, "Inspecting $name…")
            val result = backend.inspect(name)
            inspection.textContent = result.output.ifBlank { result.error ?: "No output" }
            selectedDataName = name
            dataOffset = 0
            renderData(backend.dataFrame(name, 0, 100))
            showPanel("environment")
            setBusy(false, "Ready")
        }
    }
    fun refreshEnvironment() = scope.launch { renderEnvironment(backend.environment()) }
    fun refreshPackages() = scope.launch { renderPackages(backend.packages()) }
    fun runCode(code: String, label: String = "Running…") {
        if (code.isBlank()) return
        syncActiveDocument()
        scope.launch {
            setBusy(true, label)
            appendConsole("> ${code.lineSequence().firstOrNull().orEmpty()}")
            val result = backend.evaluate(code)
            appendConsole(result.output + (result.error?.let { "Error: $it" } ?: ""))
            setBusy(false, if (result.error == null) "Ready" else "Error")
            refreshEnvironment()
        }
    }

    editor.value = activeDocument().code
    renderTabs()
    console.textContent = "R Workbench\n"
    refreshEnvironment()
    refreshPackages()

    editor.addEventListener("input", { syncActiveDocument(true) })
    run.addEventListener("click", { runCode(editor.value, "Running script…") })
    document.getElementById("run-file")?.addEventListener("click", { runCode(editor.value, "Running file…") })
    document.getElementById("run-selection")?.addEventListener("click", {
        val start = editor.selectionStart ?: 0
        val end = editor.selectionEnd ?: start
        runCode(editor.value.substring(start, end), "Running selection…")
    })
    stop.addEventListener("click", { backend.cancel(); setBusy(false, "Ready") })
    document.getElementById("new-document")?.addEventListener("click", {
        syncActiveDocument()
        val id = newDocumentId()
        documents += BrowserDocument(id, "untitled.R", "")
        activeDocumentId = id
        editor.value = ""
        persistDocuments(documents)
        renderTabs()
    })
    document.getElementById("open-document")?.addEventListener("click", { fileInput.click() })
    fileInput.addEventListener("change", {
        val file = fileInput.files?.item(0) ?: return@addEventListener
        scope.launch {
            val name = file.name
            val text = readFileText(file as JsAny).await<JsString>().toString()
            if (name.substringAfterLast('.', "").lowercase() in setOf("csv", "tsv")) {
                val variable = name.substringBeforeLast('.').replace(Regex("[^A-Za-z0-9_]"), "_").ifBlank { "data" }
                runCode("$variable <- read.csv(text = ${quoteR(text)}, stringsAsFactors = FALSE, sep = ${quoteR(if (name.endsWith(".tsv", true)) "\\t" else ",")})\n$variable", "Importing $name…")
            } else {
                syncActiveDocument()
                val id = newDocumentId()
                documents += BrowserDocument(id, name, text)
                activeDocumentId = id
                editor.value = text
                persistDocuments(documents)
                renderTabs()
                status.textContent = "Opened $name"
            }
        }
    })
    document.getElementById("save-document")?.addEventListener("click", {
        syncActiveDocument(false)
        val current = activeDocument()
        downloadText(current.name, current.code, "text/plain")
        status.textContent = "Downloaded ${current.name}"
    })
    fun submitConsoleCommand() {
        val value = command.value.trim()
        if (value.isNotEmpty()) {
            consoleHistory = (consoleHistory + value).takeLast(100).toMutableList()
            historyCursor = consoleHistory.size
            window.localStorage.setItem("r-workbench-history", consoleHistory.joinToString("\u0001"))
            command.value = ""
            runCode(value, "Running console command…")
        }
    }
    document.getElementById("console-run")?.addEventListener("click", { submitConsoleCommand() })
    command.addEventListener("keydown", { event ->
        val key = (event as org.w3c.dom.events.KeyboardEvent).key
        if (key == "Enter") submitConsoleCommand()
        if (key == "ArrowUp" && historyCursor > 0) { historyCursor -= 1; command.value = consoleHistory[historyCursor] }
        if (key == "ArrowDown" && historyCursor < consoleHistory.size - 1) { historyCursor += 1; command.value = consoleHistory[historyCursor] }
    })
    document.getElementById("clear-console")?.addEventListener("click", { console.textContent = "" })
    document.getElementById("inspect")?.addEventListener("click", {
        val name = (document.getElementById("inspect-name") as HTMLInputElement).value.trim()
        if (name.isNotEmpty()) inspectObject(name)
    })
    document.getElementById("refresh-environment")?.addEventListener("click", { refreshEnvironment() })
    document.getElementById("refresh-data")?.addEventListener("click", { selectedDataName?.let { scope.launch { renderData(backend.dataFrame(it, dataOffset, 100)) } } })
    document.getElementById("previous-page")?.addEventListener("click", { if (dataOffset >= 100) { dataOffset -= 100; selectedDataName?.let { scope.launch { renderData(backend.dataFrame(it, dataOffset, 100)) } } } })
    document.getElementById("next-page")?.addEventListener("click", { selectedDataName?.let { name -> scope.launch { val model = backend.dataFrame(name, dataOffset + 100, 100); if (model?.rows?.isNotEmpty() == true) { dataOffset += 100; renderData(model) } } } })
    document.getElementById("render-plot")?.addEventListener("click", {
        scope.launch {
            setBusy(true, "Rendering plot…")
            val result = backend.renderPlot(editor.value)
            result.plotSvg?.let { svg ->
                lastPlotSvg = svg
                plotGallery.innerHTML = "<img class=\"plot\" alt=\"Rendered R plot\" src=\"data:image/svg+xml;charset=utf-8,${encodeUri(svg)}\">"
                showPanel("plots")
            }
            if (result.error != null) appendConsole("Error: ${result.error}")
            setBusy(false, "Ready")
        }
    })
    document.getElementById("download-plot")?.addEventListener("click", { lastPlotSvg?.let { downloadText("Rplot.svg", it, "image/svg+xml") } })
    document.getElementById("report")?.addEventListener("click", {
        val current = activeDocument()
        val html = "<html><body><h1>${escapeHtml(current.name)}</h1><pre>${escapeHtml(current.code)}</pre><h2>Console</h2><pre>${escapeHtml(console.textContent.orEmpty())}</pre></body></html>"
        downloadText("${current.name.substringBeforeLast('.')}.html", html, "text/html")
    })
    document.getElementById("install")?.addEventListener("click", {
        val input = document.getElementById("install-name") as HTMLInputElement
        val name = input.value.trim()
        if (name.isNotEmpty()) scope.launch {
            setBusy(true, "Installing $name…")
            val result = backend.installPackages(listOf(name))
            appendConsole(result.output + (result.error?.let { "\nError: $it" } ?: ""))
            input.value = ""
            refreshPackages()
            setBusy(false, if (result.error == null) "Ready" else "Error")
        }
    })
    document.getElementById("refresh-packages")?.addEventListener("click", { refreshPackages() })
    document.getElementById("help")?.addEventListener("click", {
        val input = document.getElementById("help-topic") as HTMLInputElement
        val topic = input.value.trim()
        if (topic.isNotEmpty()) scope.launch { setBusy(true, "Searching help…"); val result = backend.help(topic); helpResult.textContent = result.output.ifBlank { result.error ?: "No documentation found" }; showPanel("help"); setBusy(false, "Ready") }
    })
    document.querySelectorAll("[data-panel]").asElements().forEach { button -> button.addEventListener("click", { showPanel(button.getAttribute("data-panel") ?: "environment") }) }
}

private fun encodeUri(value: String): String = js("encodeURIComponent(value)")

private fun org.w3c.dom.NodeList.asElements(): List<Element> = (0 until length).mapNotNull { item(it) as? Element }
