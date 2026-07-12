package com.rstudio.shared

/** Platform-neutral session result. The Android and browser adapters implement this contract. */
data class EvaluationResult(
    val output: String = "",
    val valueSummary: String = "No result yet",
    val error: String? = null,
    val plotSvg: String? = null,
)

data class DataTableModel(
    val title: String,
    val columns: List<String>,
    val rows: List<List<String>>,
    val totalRows: Int,
    val rowOffset: Int = 0,
)

data class EnvironmentEntryModel(
    val name: String,
    val kind: String,
    val summary: String,
)

data class PackageModel(
    val name: String,
    val version: String,
    val title: String = "",
    val needsCompilation: Boolean = false,
)

data class WorkbenchCapabilities(
    val canExecuteR: Boolean,
    val canPersistFiles: Boolean,
    val canInstallPackages: Boolean,
    val runtimeLabel: String,
)

interface RSessionBackend {
    val capabilities: WorkbenchCapabilities

    suspend fun evaluate(code: String): EvaluationResult

    suspend fun inspect(name: String): EvaluationResult

    suspend fun environment(): List<EnvironmentEntryModel>

    suspend fun packages(): List<PackageModel>

    suspend fun installPackages(names: List<String>): EvaluationResult =
        EvaluationResult(error = "Package installation is not supported by this backend")

    suspend fun renderPlot(code: String): EvaluationResult =
        EvaluationResult(error = "Plot rendering is not supported by this backend")

    fun cancel()
}

data class WorkbenchDocument(
    val id: String,
    val name: String,
    val code: String,
    val isDirty: Boolean = false,
)

data class WorkbenchState(
    val documents: List<WorkbenchDocument> = listOf(
        WorkbenchDocument("untitled.R", "untitled.R", "# R Workbench\n")
    ),
    val activeDocumentId: String = "untitled.R",
    val console: String = "R Workbench\n",
    val status: String = "Ready",
    val isRunning: Boolean = false,
    val errorMessage: String? = null,
)
