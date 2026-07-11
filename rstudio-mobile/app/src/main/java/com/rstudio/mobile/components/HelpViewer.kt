package com.rstudio.mobile.components

import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp

private data class HelpTopic(
    val name: String,
    val summary: String,
    val category: String,
)

private val ALL_CATEGORY = "All"

private val HELP_TOPICS = listOf(
    // ── base ──────────────────────────────────────────────────────
    HelpTopic("c", "Combine values into a vector or list", "base"),
    HelpTopic("length", "Get or set the length of an object", "base"),
    HelpTopic("seq", "Generate regular sequences", "base"),
    HelpTopic("rep", "Replicate elements of vectors and lists", "base"),
    HelpTopic("paste", "Concatenate strings", "base"),
    HelpTopic("paste0", "Concatenate strings without separator", "base"),
    HelpTopic("print", "Print values to the console", "base"),
    HelpTopic("cat", "Concatenate and print output", "base"),
    HelpTopic("sprintf", "Format strings C-style", "base"),
    HelpTopic("nchar", "Count number of characters in a string", "base"),
    HelpTopic("substr", "Extract or replace substrings", "base"),
    HelpTopic("gsub", "Pattern replacement in strings", "base"),
    HelpTopic("grep", "Pattern matching and searching", "base"),
    HelpTopic("grepl", "Pattern matching returning logical vector", "base"),
    HelpTopic("class", "Get or set the class of an object", "base"),
    HelpTopic("is.na", "Test for missing values", "base"),
    HelpTopic("is.null", "Test for NULL", "base"),
    HelpTopic("as.numeric", "Coerce to numeric", "base"),
    HelpTopic("as.character", "Coerce to character", "base"),
    HelpTopic("as.integer", "Coerce to integer", "base"),
    HelpTopic("which", "Which indices are TRUE?", "base"),
    HelpTopic("ifelse", "Conditional element selection", "base"),
    HelpTopic("switch", "Select among alternatives", "base"),
    HelpTopic("tryCatch", "Condition handling and error recovery", "base"),
    HelpTopic("stop", "Stop execution and signal an error", "base"),
    HelpTopic("warning", "Generate a warning message", "base"),
    HelpTopic("message", "Produce a diagnostic message", "base"),
    HelpTopic("Sys.time", "Get current date-time", "base"),
    HelpTopic("system.time", "Time the evaluation of an expression", "base"),
    HelpTopic("do.call", "Execute a function call from a list of arguments", "base"),
    HelpTopic("lapply", "Apply a function over a list", "base"),
    HelpTopic("sapply", "Apply a function and simplify result", "base"),
    HelpTopic("vapply", "Apply with a specified return type", "base"),
    HelpTopic("Map", "Apply a function to multiple lists", "base"),
    HelpTopic("Reduce", "Reduce a list to a single value", "base"),
    HelpTopic("Filter", "Filter elements satisfying a condition", "base"),
    HelpTopic("environment", "Get or set the environment of a function", "base"),
    HelpTopic("list", "Create a list", "base"),
    HelpTopic("data.frame", "Create a data frame", "base"),
    HelpTopic("matrix", "Create a matrix", "base"),
    HelpTopic("array", "Create a multi-dimensional array", "base"),
    HelpTopic("factor", "Create a factor (categorical variable)", "base"),
    HelpTopic("names", "Get or set names of an object", "base"),
    HelpTopic("dim", "Get or set dimensions of an object", "base"),
    HelpTopic("nrow", "Get number of rows", "base"),
    HelpTopic("ncol", "Get number of columns", "base"),
    HelpTopic("head", "Return the first parts of an object", "base"),
    HelpTopic("tail", "Return the last parts of an object", "base"),
    HelpTopic("str", "Display the structure of an object", "base"),
    HelpTopic("summary", "Produce result summaries", "base"),
    HelpTopic("typeof", "Get the internal type of an object", "base"),
    HelpTopic("exists", "Test if a variable exists", "base"),
    HelpTopic("rm", "Remove objects from the environment", "base"),
    HelpTopic("ls", "List objects in the environment", "base"),
    HelpTopic("get", "Get the value of a variable by name", "base"),
    HelpTopic("assign", "Assign a value to a variable by name", "base"),
    HelpTopic("match", "Value matching", "base"),
    HelpTopic("unique", "Extract unique elements", "base"),
    HelpTopic("duplicated", "Identify duplicate elements", "base"),
    HelpTopic("sort", "Sort a vector", "base"),
    HelpTopic("order", "Permutation to arrange elements", "base"),
    HelpTopic("rev", "Reverse elements", "base"),
    HelpTopic("append", "Append elements to a vector", "base"),
    HelpTopic("range", "Get the range of values", "base"),
    HelpTopic("diff", "Lagged differences", "base"),
    HelpTopic("cumsum", "Cumulative sums", "base"),
    HelpTopic("cumprod", "Cumulative products", "base"),
    HelpTopic("abs", "Absolute value", "base"),
    HelpTopic("sqrt", "Square root", "base"),
    HelpTopic("round", "Round to specified number of decimal places", "base"),
    HelpTopic("ceiling", "Round up to nearest integer", "base"),
    HelpTopic("floor", "Round down to nearest integer", "base"),
    HelpTopic("max", "Maximum value", "base"),
    HelpTopic("min", "Minimum value", "base"),
    HelpTopic("sum", "Sum of vector elements", "base"),
    HelpTopic("prod", "Product of vector elements", "base"),
    HelpTopic("log", "Natural logarithm", "base"),
    HelpTopic("exp", "Exponential function", "base"),
    HelpTopic("Vectorize", "Vectorize a scalar function", "base"),
    HelpTopic("function", "Function definition", "base"),
    HelpTopic("return", "Return a value from a function", "base"),
    HelpTopic("invisible", "Return a value invisibly", "base"),

    // ── stats ─────────────────────────────────────────────────────
    HelpTopic("mean", "Arithmetic mean", "stats"),
    HelpTopic("median", "Median value", "stats"),
    HelpTopic("sd", "Standard deviation", "stats"),
    HelpTopic("var", "Variance", "stats"),
    HelpTopic("cor", "Correlation", "stats"),
    HelpTopic("cov", "Covariance", "stats"),
    HelpTopic("lm", "Fit linear models", "stats"),
    HelpTopic("glm", "Fit generalized linear models", "stats"),
    HelpTopic("t.test", "Student's t-test", "stats"),
    HelpTopic("chisq.test", "Chi-squared test", "stats"),
    HelpTopic("wilcox.test", "Wilcoxon rank-sum and signed-rank tests", "stats"),
    HelpTopic("anova", "Analysis of variance", "stats"),
    HelpTopic("aov", "Fit an analysis of variance model", "stats"),
    HelpTopic("predict", "Model predictions", "stats"),
    HelpTopic("residuals", "Extract model residuals", "stats"),
    HelpTopic("fitted", "Extract fitted values", "stats"),
    HelpTopic("coef", "Extract model coefficients", "stats"),
    HelpTopic("confint", "Confidence intervals for model parameters", "stats"),
    HelpTopic("quantile", "Sample quantiles", "stats"),
    HelpTopic("rnorm", "Normal distribution random generation", "stats"),
    HelpTopic("dnorm", "Normal distribution density", "stats"),
    HelpTopic("pnorm", "Normal distribution CDF", "stats"),
    HelpTopic("qnorm", "Normal distribution quantile function", "stats"),
    HelpTopic("runif", "Uniform distribution random generation", "stats"),
    HelpTopic("rbinom", "Binomial distribution random generation", "stats"),
    HelpTopic("rpois", "Poisson distribution random generation", "stats"),
    HelpTopic("sample", "Random sampling", "stats"),
    HelpTopic("set.seed", "Set the random number seed", "stats"),
    HelpTopic("density", "Kernel density estimation", "stats"),
    HelpTopic("ecdf", "Empirical cumulative distribution function", "stats"),
    HelpTopic("kmeans", "K-means clustering", "stats"),
    HelpTopic("hclust", "Hierarchical clustering", "stats"),
    HelpTopic("prcomp", "Principal components analysis", "stats"),
    HelpTopic("optim", "General-purpose optimisation", "stats"),
    HelpTopic("nlm", "Non-linear minimisation", "stats"),
    HelpTopic("integrate", "Numerical integration", "stats"),
    HelpTopic("smooth.spline", "Fit a smoothing spline", "stats"),
    HelpTopic("lowess", "Locally weighted scatterplot smoothing", "stats"),
    HelpTopic("na.omit", "Remove NA values", "stats"),
    HelpTopic("complete.cases", "Find complete cases (no NAs)", "stats"),
    HelpTopic("scale", "Centre and scale a matrix", "stats"),
    HelpTopic("dist", "Distance matrix computation", "stats"),

    // ── utils ─────────────────────────────────────────────────────
    HelpTopic("help", "Display documentation for a topic", "utils"),
    HelpTopic("library", "Load and attach add-on packages", "utils"),
    HelpTopic("require", "Load a package, return FALSE on failure", "utils"),
    HelpTopic("installed.packages", "List installed packages", "utils"),
    HelpTopic("read.csv", "Read a CSV file into a data frame", "utils"),
    HelpTopic("write.csv", "Write a data frame to a CSV file", "utils"),
    HelpTopic("read.table", "Read a text table into a data frame", "utils"),
    HelpTopic("write.table", "Write a data frame to a text file", "utils"),
    HelpTopic("readLines", "Read text lines from a connection", "utils"),
    HelpTopic("writeLines", "Write text lines to a connection", "utils"),
    HelpTopic("source", "Read and evaluate R code from a file", "utils"),
    HelpTopic("save", "Save R objects to a file", "utils"),
    HelpTopic("load", "Reload saved datasets", "utils"),
    HelpTopic("saveRDS", "Save a single R object", "utils"),
    HelpTopic("readRDS", "Read a single R object", "utils"),
    HelpTopic("file.path", "Construct platform-independent file paths", "utils"),
    HelpTopic("getwd", "Get the working directory", "utils"),
    HelpTopic("setwd", "Set the working directory", "utils"),
    HelpTopic("list.files", "List files in a directory", "utils"),
    HelpTopic("file.exists", "Test if a file exists", "utils"),
    HelpTopic("download.file", "Download a file from the internet", "utils"),
    HelpTopic("capture.output", "Capture printed output as a character vector", "utils"),
    HelpTopic("Sys.getenv", "Get an environment variable", "utils"),
    HelpTopic("Sys.setenv", "Set an environment variable", "utils"),

    // ── graphics ──────────────────────────────────────────────────
    HelpTopic("plot", "Generic X-Y plotting", "graphics"),
    HelpTopic("barplot", "Bar plots", "graphics"),
    HelpTopic("hist", "Histograms", "graphics"),
    HelpTopic("boxplot", "Box-and-whisker plots", "graphics"),
    HelpTopic("pie", "Pie charts", "graphics"),
    HelpTopic("lines", "Add lines to a plot", "graphics"),
    HelpTopic("points", "Add points to a plot", "graphics"),
    HelpTopic("abline", "Add straight lines to a plot", "graphics"),
    HelpTopic("text", "Add text to a plot", "graphics"),
    HelpTopic("legend", "Add a legend to a plot", "graphics"),
    HelpTopic("title", "Add titles to a plot", "graphics"),
    HelpTopic("par", "Set or query graphical parameters", "graphics"),
    HelpTopic("curve", "Draw a function curve", "graphics"),
    HelpTopic("polygon", "Draw a polygon", "graphics"),
    HelpTopic("segments", "Draw line segments", "graphics"),
    HelpTopic("arrows", "Draw arrows", "graphics"),
    HelpTopic("rect", "Draw rectangles", "graphics"),
    HelpTopic("image", "Display a matrix as a colour image", "graphics"),
    HelpTopic("contour", "Contour plots", "graphics"),
    HelpTopic("persp", "3-D perspective plots", "graphics"),
    HelpTopic("pairs", "Scatterplot matrices", "graphics"),
    HelpTopic("mosaicplot", "Mosaic plots", "graphics"),
    HelpTopic("stripchart", "One-dimensional scatter plots", "graphics"),
    HelpTopic("dotchart", "Cleveland dot plots", "graphics"),
    HelpTopic("matplot", "Plot columns of matrices", "graphics"),
    HelpTopic("layout", "Specify complex plot arrangement", "graphics"),
    HelpTopic("mtext", "Write text in outer margins", "graphics"),
    HelpTopic("axis", "Add an axis to a plot", "graphics"),
    HelpTopic("rug", "Add a rug to a plot", "graphics"),
    HelpTopic("symbols", "Draw symbols on a plot", "graphics"),
)

private val CATEGORIES = listOf(ALL_CATEGORY) + HELP_TOPICS.map { it.category }.distinct().sorted()

@Composable
fun HelpViewer(
    helpResult: String?,
    helpLoading: Boolean,
    onLookupHelp: (String) -> Unit,
    onClearHelp: () -> Unit,
) {
    var query by remember { mutableStateOf("") }
    var selectedCategory by remember { mutableStateOf(ALL_CATEGORY) }
    var selectedTopic by remember { mutableStateOf<HelpTopic?>(null) }

    // Detail view — show the R help output for the selected topic
    if (selectedTopic != null) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = {
                    selectedTopic = null
                    onClearHelp()
                }) {
                    Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                }
                Column(Modifier.padding(start = 4.dp)) {
                    Text(
                        "${selectedTopic!!.name}()",
                        style = MaterialTheme.typography.titleMedium,
                    )
                    Text(
                        selectedTopic!!.summary,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }

            when {
                helpLoading -> {
                    Column(
                        modifier = Modifier.fillMaxSize(),
                        verticalArrangement = Arrangement.Center,
                        horizontalAlignment = Alignment.CenterHorizontally,
                    ) {
                        CircularProgressIndicator()
                        Text(
                            "Loading help…",
                            modifier = Modifier.padding(top = 8.dp),
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                }
                helpResult != null -> {
                    Text(
                        text = helpResult,
                        modifier = Modifier
                            .fillMaxSize()
                            .verticalScroll(rememberScrollState())
                            .padding(8.dp),
                        style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
                    )
                }
                else -> {
                    Text(
                        "Tap a topic to view its R documentation.",
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
            }
        }
        return
    }

    // List view — browse and search topics
    val filtered = HELP_TOPICS.filter { topic ->
        val matchesCategory = selectedCategory == ALL_CATEGORY || topic.category == selectedCategory
        val matchesQuery = query.isBlank() ||
            topic.name.contains(query, ignoreCase = true) ||
            topic.summary.contains(query, ignoreCase = true)
        matchesCategory && matchesQuery
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text("R Help", style = MaterialTheme.typography.titleMedium)

        OutlinedTextField(
            value = query,
            onValueChange = { query = it },
            modifier = Modifier.fillMaxWidth(),
            label = { Text("Search functions…") },
            singleLine = true,
        )

        Row(
            modifier = Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            CATEGORIES.forEach { category ->
                FilterChip(
                    selected = selectedCategory == category,
                    onClick = { selectedCategory = category },
                    label = { Text(category) },
                )
            }
        }

        Text(
            "${filtered.size} topics",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            items(filtered, key = { it.name }) { topic ->
                ElevatedCard(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clickable {
                            selectedTopic = topic
                            onLookupHelp(topic.name)
                        },
                ) {
                    Row(
                        modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(modifier = Modifier.weight(1f)) {
                            Text("${topic.name}()", style = MaterialTheme.typography.titleSmall)
                            Text(topic.summary, style = MaterialTheme.typography.bodySmall)
                        }
                        Text(
                            topic.category,
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.primary,
                        )
                    }
                }
            }
        }
    }
}
