package com.rstudio.mobile.components

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.FileOpen
import androidx.compose.material.icons.filled.Image
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Save
import androidx.compose.material.icons.filled.UploadFile
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.OffsetMapping
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.input.TransformedText
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.rstudio.mobile.util.RSyntaxHighlighter
import com.rstudio.mobile.util.executionTarget

@Composable
fun ScriptEditor(
    code: String,
    fileName: String,
    isDirty: Boolean,
    isRunning: Boolean,
    status: String,
    onCodeChange: (String) -> Unit,
    onRunCode: (String) -> Unit,
    onRunFile: () -> Unit,
    onRenderPlot: () -> Unit,
    onImportCsv: () -> Unit,
    onOpenScript: () -> Unit,
    onSaveScript: () -> Unit,
    onExportScript: () -> Unit,
) {
    var editor by remember { mutableStateOf(TextFieldValue(code, TextRange(code.length))) }
    var menuExpanded by remember { mutableStateOf(false) }
    val verticalScroll = rememberScrollState()
    val horizontalScroll = rememberScrollState()

    LaunchedEffect(code) {
        if (code != editor.text) {
            editor = TextFieldValue(code, TextRange(code.length))
        }
    }

    fun runTarget() {
        val selection = editor.selection
        onRunCode(executionTarget(editor.text, selection.start, selection.end))
    }

    Column(Modifier.fillMaxSize()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(start = 12.dp, end = 4.dp, top = 4.dp, bottom = 4.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text(
                    text = fileName + if (isDirty) " •" else "",
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 1,
                )
                Text(status, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            IconButton(onClick = onSaveScript, enabled = !isRunning) {
                Icon(Icons.Default.Save, contentDescription = "Save script (Ctrl+S)")
            }
            FilledTonalButton(onClick = ::runTarget, enabled = !isRunning) {
                Icon(Icons.Default.PlayArrow, contentDescription = null)
                Text("Run")
            }
            Box {
                IconButton(onClick = { menuExpanded = true }) {
                    Icon(Icons.Default.MoreVert, contentDescription = "More editor actions")
                }
                DropdownMenu(expanded = menuExpanded, onDismissRequest = { menuExpanded = false }) {
                    DropdownMenuItem(
                        text = { Text("Run entire script") },
                        leadingIcon = { Icon(Icons.Default.PlayArrow, contentDescription = null) },
                        onClick = { menuExpanded = false; onRunFile() },
                    )
                    DropdownMenuItem(
                        text = { Text("Open script") },
                        leadingIcon = { Icon(Icons.Default.UploadFile, contentDescription = null) },
                        onClick = { menuExpanded = false; onOpenScript() },
                    )
                    DropdownMenuItem(
                        text = { Text("Import data") },
                        leadingIcon = { Icon(Icons.Default.FileOpen, contentDescription = null) },
                        onClick = { menuExpanded = false; onImportCsv() },
                    )
                    DropdownMenuItem(
                        text = { Text("Export script") },
                        leadingIcon = { Icon(Icons.Default.Save, contentDescription = null) },
                        onClick = { menuExpanded = false; onExportScript() },
                    )
                    DropdownMenuItem(
                        text = { Text("Render plot") },
                        leadingIcon = { Icon(Icons.Default.Image, contentDescription = null) },
                        onClick = { menuExpanded = false; onRenderPlot() },
                    )
                }
            }
        }

        HorizontalDivider()

        Row(
            Modifier
                .fillMaxSize()
                .onPreviewKeyEvent { event ->
                    if (event.type != KeyEventType.KeyDown || !event.isCtrlPressed) return@onPreviewKeyEvent false
                    when {
                        event.key == Key.S -> { onSaveScript(); true }
                        event.key == Key.Enter && event.isShiftPressed -> { onRunFile(); true }
                        event.key == Key.Enter -> { runTarget(); true }
                        else -> false
                    }
                },
        ) {
            Text(
                text = (1..editor.text.lineSequence().count()).joinToString("\n"),
                modifier = Modifier
                    .width(48.dp)
                    .fillMaxHeight()
                    .background(MaterialTheme.colorScheme.surfaceVariant)
                    .verticalScroll(verticalScroll, enabled = false)
                    .padding(horizontal = 8.dp, vertical = 10.dp),
                style = MaterialTheme.typography.bodySmall.copy(
                    fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                    fontSize = 13.sp,
                    lineHeight = 20.sp,
                ),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            BasicTextField(
                value = editor,
                onValueChange = { next -> editor = next; onCodeChange(next.text) },
                modifier = Modifier
                    .fillMaxSize()
                    .verticalScroll(verticalScroll)
                    .horizontalScroll(horizontalScroll)
                    .padding(10.dp),
                textStyle = MaterialTheme.typography.bodyMedium.copy(
                    color = MaterialTheme.colorScheme.onSurface,
                    fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                    fontSize = 15.sp,
                    lineHeight = 20.sp,
                ),
                visualTransformation = VisualTransformation { text ->
                    TransformedText(RSyntaxHighlighter.highlight(text), OffsetMapping.Identity)
                },
            )
        }
    }
}
