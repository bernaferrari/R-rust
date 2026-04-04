package com.rport.sample

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.Bundle
import android.os.IBinder
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import coil.compose.AsyncImage
import kotlinx.coroutines.flow.collectLatest

class MainActivity : ComponentActivity() {
    private var runtimeService: RRuntimeService? = null
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
                    }
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
    var codeInput by remember { mutableStateOf("""
        # Example plot
        x <- seq(0, 2*pi, length=100)
        y <- sin(x)
        plot(x, y, type="l", col="blue", lwd=2)
        title("Sine Wave Plot")
        print("Plot generated successfully")
    """.trimIndent()) }

    val consoleOutput by service.consoleOutput.collectAsState()
    val isRunning by service.sessionState.collectAsState()
    val progress by service.progress.collectAsState()

    val scrollState = rememberScrollState()

    LaunchedEffect(consoleOutput) {
        scrollState.animateScrollTo(scrollState.maxValue)
    }

    Column(modifier = Modifier
        .fillMaxSize()
        .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp)
    ) {
        OutlinedTextField(
            value = codeInput,
            onValueChange = { codeInput = it },
            modifier = Modifier.fillMaxWidth().height(120.dp),
            label = { Text("R Code") }
        )

        if (isRunning == RRuntimeService.SessionState.RUNNING) {
            LinearProgressIndicator(
                progress = { progress.toFloat() },
                modifier = Modifier.fillMaxWidth()
            )
        }

        Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(
                onClick = { service.evaluateCode(codeInput) },
                enabled = isRunning == RRuntimeService.SessionState.IDLE,
                modifier = Modifier.weight(1f)
            ) {
                Text(if (isRunning == RRuntimeService.SessionState.RUNNING) "Running..." else "Run R Code")
            }

            Button(
                onClick = { service.cancelExecution() },
                enabled = isRunning == RRuntimeService.SessionState.RUNNING,
                modifier = Modifier.weight(1f)
            ) {
                Text("Cancel")
            }

            Button(onClick = { service.evaluateCode("") }, modifier = Modifier.weight(1f)) {
                Text("Clear Console")
            }
        }

        Card(modifier = Modifier.weight(1f).fillMaxWidth()) {
            Text(
                text = consoleOutput,
                modifier = Modifier
                    .padding(8.dp)
                    .fillMaxSize()
                    .verticalScroll(scrollState),
                style = MaterialTheme.typography.bodySmall
            )
        }
    }
}

