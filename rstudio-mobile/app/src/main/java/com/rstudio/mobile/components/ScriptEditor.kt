package com.rstudio.mobile.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.FileOpen
import androidx.compose.material.icons.filled.Image
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Save
import androidx.compose.material.icons.filled.UploadFile
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.VerticalDivider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.rstudio.mobile.util.RSyntaxHighlighter
import androidx.compose.ui.text.input.OffsetMapping
import androidx.compose.ui.text.input.TransformedText
import androidx.compose.ui.text.input.VisualTransformation

@Composable
fun ScriptEditor(
    code: String,
    fileName: String,
    isRunning: Boolean,
    status: String,
    onCodeChange: (String) -> Unit,
    onRun: () -> Unit,
    onRenderPlot: () -> Unit,
    onImportCsv: () -> Unit,
    onOpenScript: () -> Unit,
    onSaveScript: () -> Unit,
    onExportScript: () -> Unit,
) {
    val lineCount = code.split('\n').size
    val listState = rememberLazyListState()

    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Column {
                Text(fileName, style = MaterialTheme.typography.titleSmall)
                Text(status, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                OutlinedButton(onClick = onOpenScript, enabled = !isRunning) {
                    Icon(Icons.Default.UploadFile, contentDescription = "Open")
                    Text("Open")
                }
                OutlinedButton(onClick = onSaveScript, enabled = !isRunning) {
                    Icon(Icons.Default.Save, contentDescription = "Save")
                    Text("Save")
                }
                OutlinedButton(onClick = onImportCsv, enabled = !isRunning) {
                    Icon(Icons.Default.FileOpen, contentDescription = "Import CSV")
                    Text("CSV")
                }
                OutlinedButton(onClick = onExportScript, enabled = !isRunning) {
                    Icon(Icons.Default.Save, contentDescription = "Export")
                    Text("Export")
                }
                OutlinedButton(onClick = onRenderPlot, enabled = !isRunning) {
                    Icon(Icons.Default.Image, contentDescription = "Plot")
                    Text("Plot")
                }
                FilledTonalButton(onClick = onRun, enabled = !isRunning) {
                    Icon(Icons.Default.PlayArrow, contentDescription = "Run")
                    Text("Run")
                }
            }
        }

        HorizontalDivider()

        Row(Modifier.fillMaxSize()) {
            // Line numbers
            LazyColumn(state = listState, modifier = Modifier.width(48.dp).background(MaterialTheme.colorScheme.surfaceVariant)) {
                items(lineCount) { line ->
                    Text(
                        text = "${line + 1}",
                        modifier = Modifier.padding(horizontal = 8.dp, vertical = 2.dp),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        fontSize = 13.sp
                    )
                }
            }

            VerticalDivider()

            // Code editor
            BasicTextField(
                value = code,
                onValueChange = onCodeChange,
                modifier = Modifier.fillMaxSize().padding(8.dp),
                textStyle = MaterialTheme.typography.bodyMedium.copy(
                    fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                    fontSize = 14.sp,
                    lineHeight = 20.sp
                ),
                visualTransformation = VisualTransformation { text ->
                    TransformedText(
                        text = RSyntaxHighlighter.highlight(text),
                        offsetMapping = OffsetMapping.Identity,
                    )
                }
            )
        }
    }
}
