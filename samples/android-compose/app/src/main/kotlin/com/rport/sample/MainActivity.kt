package com.rport.sample

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.graphics.BitmapFactory
import android.os.Bundle
import android.os.IBinder
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.ElevatedButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp

class MainActivity : ComponentActivity() {
    private var runtimeService by mutableStateOf<RRuntimeService?>(null)

    private val serviceConnection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
            val binder = service as RRuntimeService.RRuntimeBinder
            runtimeService = binder.getService()
        }

        override fun onServiceDisconnected(name: ComponentName?) {
            runtimeService = null
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        RRuntimeService.startService(this)
        bindService(
            Intent(this, RRuntimeService::class.java),
            serviceConnection,
            Context.BIND_AUTO_CREATE
        )

        setContent {
            MaterialTheme {
                Surface(modifier = Modifier.fillMaxSize()) {
                    runtimeService?.let { service ->
                        MainScreen(service)
                    } ?: Text(
                        text = "Starting R runtime...",
                        modifier = Modifier.padding(16.dp),
                    )
                }
            }
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        unbindService(serviceConnection)
    }
}

@Composable
fun MainScreen(service: RRuntimeService) {
    var codeInput by remember {
        mutableStateOf(
            """
            c(1, 2, 3) * 2
            sum(c(10, 20, 30))
            """.trimIndent()
        )
    }

    val tabs by service.tabs.collectAsState()
    val activeTabIndex by service.activeTabIndex.collectAsState()
    val activeTab = tabs[activeTabIndex]
    val scrollState = rememberScrollState()

    LaunchedEffect(activeTab.console) {
        scrollState.animateScrollTo(scrollState.maxValue)
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            tabs.forEachIndexed { index, tab ->
                FilterChip(
                    selected = index == activeTabIndex,
                    onClick = { service.selectTab(index) },
                    label = { Text(tab.name) },
                )
            }
        }

        OutlinedTextField(
            value = codeInput,
            onValueChange = { codeInput = it },
            modifier = Modifier
                .fillMaxWidth()
                .height(120.dp),
            label = { Text("R code") },
            textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
        )

        if (activeTab.isRunning) {
            LinearProgressIndicator(
                progress = { activeTab.progress.toFloat() },
                modifier = Modifier.fillMaxWidth()
            )
        }

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            Button(
                onClick = { service.evaluateCode(codeInput) },
                enabled = !activeTab.isRunning,
                modifier = Modifier.weight(1f)
            ) {
                Text("Run")
            }

            ElevatedButton(
                onClick = {
                    service.renderPlot(
                        code = """plot(c(1, 2, 3, 4), c(1, 4, 9, 16), type = "l", col = "blue", lwd = 2, main = "Android plot", xlab = "x", ylab = "x^2")""",
                        width = 720,
                        height = 480,
                    )
                },
                enabled = !activeTab.isRunning,
                modifier = Modifier.weight(1f)
            ) {
                Text("Plot")
            }

            TextButton(
                onClick = { service.cancelExecution() },
                enabled = activeTab.isRunning,
                modifier = Modifier.weight(1f)
            ) {
                Text("Cancel")
            }
        }

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            TextButton(
                onClick = { service.loadDemoPackage() },
                enabled = !activeTab.isRunning,
                modifier = Modifier.weight(1f)
            ) {
                Text("Load package")
            }

            TextButton(
                onClick = { service.runShowcase() },
                enabled = tabs.none { it.isRunning },
                modifier = Modifier.weight(1f)
            ) {
                Text("Showcase")
            }

            TextButton(
                onClick = { service.startLongRunningEval() },
                enabled = !activeTab.isRunning,
                modifier = Modifier.weight(1f)
            ) {
                Text("Long eval")
            }
        }

        RuntimeStatus(activeTab)

        activeTab.lastPlot?.let { plot ->
            PlotPreview(plot)
        }

        Card(
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth()
        ) {
            Text(
                text = activeTab.console,
                modifier = Modifier
                    .padding(10.dp)
                    .fillMaxSize()
                    .verticalScroll(scrollState),
                style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace)
            )
        }

        TextButton(onClick = { service.clearConsole() }) {
            Text("Clear console")
        }
    }
}

@Composable
private fun RuntimeStatus(tab: RuntimeTabUiState) {
    val installed = tab.installedPackages.joinToString(", ").ifBlank { "none" }
    val loaded = tab.loadedPackages.joinToString(", ").ifBlank { "none" }
    Text(
        text = "Value: ${tab.lastValueKind}  Packages: $installed  Loaded: $loaded",
        style = MaterialTheme.typography.bodySmall,
    )
}

@Composable
private fun PlotPreview(plot: PlotImage) {
    val bitmap = remember(plot.pngBytes) {
        BitmapFactory.decodeByteArray(plot.pngBytes, 0, plot.pngBytes.size)
    }
    if (bitmap != null) {
        Card(
            modifier = Modifier
                .fillMaxWidth()
                .height(190.dp)
        ) {
            Image(
                bitmap = bitmap.asImageBitmap(),
                contentDescription = "Rendered R plot",
                modifier = Modifier
                    .fillMaxWidth()
                    .widthIn(max = 720.dp),
                contentScale = ContentScale.Fit,
            )
        }
    }
}
