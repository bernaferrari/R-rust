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
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.FileOpen
import androidx.compose.material.icons.filled.Image
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Save
import androidx.compose.material.icons.filled.UploadFile
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.OffsetMapping
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.input.TransformedText
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.rstudio.mobile.R
import com.rstudio.mobile.util.RSyntaxHighlighter
import com.rstudio.mobile.util.executionTarget
import com.rstudio.mobile.runtime.Diagnostic

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
    diagnostics: List<Diagnostic> = emptyList(),
    onExportReport: () -> Unit = {},
) {
    var editor by remember { mutableStateOf(TextFieldValue(code, TextRange(code.length))) }
    var menuExpanded by remember { mutableStateOf(false) }
    var searchVisible by remember { mutableStateOf(false) }
    var searchQuery by remember { mutableStateOf("") }
    var replacement by remember { mutableStateOf("") }
    val verticalScroll = rememberScrollState()
    val horizontalScroll = rememberScrollState()
    val editorLabel = stringResource(R.string.editor_code_label)
    val documentState = stringResource(if (isDirty) R.string.editor_unsaved_changes else R.string.editor_saved)
    val actionModifier = Modifier.sizeIn(minWidth = 48.dp, minHeight = 48.dp)

    LaunchedEffect(code) {
        if (code != editor.text) {
            editor = TextFieldValue(code, TextRange(code.length))
        }
    }

    fun runTarget() {
        if (isRunning) return
        val selection = editor.selection
        onRunCode(executionTarget(editor.text, selection.start, selection.end))
    }

    fun findNext() {
        if (searchQuery.isBlank()) return
        val start = editor.text.indexOf(searchQuery, (editor.selection.end + 1).coerceAtMost(editor.text.length), ignoreCase = false)
            .let { if (it >= 0) it else editor.text.indexOf(searchQuery, ignoreCase = false) }
        if (start >= 0) editor = editor.copy(selection = androidx.compose.ui.text.TextRange(start, start + searchQuery.length))
    }

    Column(Modifier.fillMaxSize()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(start = 12.dp, end = 4.dp, top = 4.dp, bottom = 4.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text(
                    text = if (isDirty) stringResource(R.string.editor_dirty_file_name, fileName) else fileName,
                    modifier = Modifier.semantics { stateDescription = documentState },
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 1,
                )
                Text(status, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            IconButton(onClick = onSaveScript, enabled = !isRunning, modifier = actionModifier) {
                Icon(Icons.Default.Save, contentDescription = stringResource(R.string.editor_save))
            }
            FilledTonalButton(onClick = ::runTarget, enabled = !isRunning, modifier = actionModifier) {
                Icon(Icons.Default.PlayArrow, contentDescription = null)
                Text(stringResource(R.string.editor_run))
            }
            Box {
                IconButton(onClick = { menuExpanded = true }, modifier = actionModifier) {
                    Icon(Icons.Default.MoreVert, contentDescription = stringResource(R.string.editor_more_actions))
                }
                DropdownMenu(expanded = menuExpanded, onDismissRequest = { menuExpanded = false }) {
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.editor_run_file)) },
                        leadingIcon = { Icon(Icons.Default.PlayArrow, contentDescription = null) },
                        enabled = !isRunning,
                        onClick = { menuExpanded = false; onRunFile() },
                    )
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.editor_open)) },
                        leadingIcon = { Icon(Icons.Default.UploadFile, contentDescription = null) },
                        onClick = { menuExpanded = false; onOpenScript() },
                    )
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.editor_import_data)) },
                        leadingIcon = { Icon(Icons.Default.FileOpen, contentDescription = null) },
                        onClick = { menuExpanded = false; onImportCsv() },
                    )
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.editor_export)) },
                        leadingIcon = { Icon(Icons.Default.Save, contentDescription = null) },
                        onClick = { menuExpanded = false; onExportScript() },
                    )
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.editor_render_plot)) },
                        leadingIcon = { Icon(Icons.Default.Image, contentDescription = null) },
                        enabled = !isRunning,
                        onClick = { menuExpanded = false; onRenderPlot() },
                    )
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.editor_find_replace)) },
                        leadingIcon = { Icon(Icons.Default.Search, contentDescription = null) },
                        onClick = { menuExpanded = false; searchVisible = true },
                    )
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.editor_export_report)) },
                        leadingIcon = { Icon(Icons.Default.FileOpen, contentDescription = null) },
                        onClick = { menuExpanded = false; onExportReport() },
                    )
                }
            }
        }

        HorizontalDivider()

        if (searchVisible) {
            Column(Modifier.fillMaxWidth().padding(horizontal = 10.dp, vertical = 4.dp)) {
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    OutlinedTextField(
                        value = searchQuery,
                        onValueChange = { searchQuery = it },
                        modifier = Modifier.weight(1f),
                        label = { Text(stringResource(R.string.editor_find)) },
                        singleLine = true,
                    )
                    OutlinedTextField(
                        value = replacement,
                        onValueChange = { replacement = it },
                        modifier = Modifier.weight(1f),
                        label = { Text(stringResource(R.string.editor_replace_with)) },
                        singleLine = true,
                    )
                }
                Row(
                    Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    TextButton(onClick = ::findNext, modifier = actionModifier) { Text(stringResource(R.string.editor_find_next)) }
                    TextButton(onClick = {
                        if (searchQuery.isNotEmpty()) {
                            val replaced = editor.text.replace(searchQuery, replacement, ignoreCase = false)
                            editor = TextFieldValue(replaced, TextRange(replaced.length))
                            onCodeChange(replaced)
                        }
                    }, modifier = actionModifier) { Text(stringResource(R.string.editor_replace_all)) }
                    TextButton(onClick = { searchVisible = false }, modifier = actionModifier) { Text(stringResource(R.string.editor_close_search)) }
                }
            }
        }

        if (diagnostics.isNotEmpty()) {
            Column(Modifier.fillMaxWidth().padding(horizontal = 10.dp, vertical = 2.dp)) {
                diagnostics.forEach { diagnostic ->
                    Text(
                        text = diagnostic.line?.let { line ->
                            stringResource(R.string.editor_diagnostic_line, line, diagnostic.message)
                        } ?: diagnostic.message,
                        modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
                        color = MaterialTheme.colorScheme.error,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
        }

        Row(
            Modifier
                .fillMaxSize()
                .onPreviewKeyEvent { event ->
                    if (event.type != KeyEventType.KeyDown || !event.isCtrlPressed) return@onPreviewKeyEvent false
                    when {
                        event.key == Key.S -> { if (!isRunning) onSaveScript(); true }
                        event.key == Key.Enter && event.isShiftPressed -> { if (!isRunning) onRunFile(); true }
                        event.key == Key.Enter -> { runTarget(); true }
                        else -> false
                    }
                },
        ) {
            Text(
                text = (1..editor.text.lineSequence().count()).joinToString("\n"),
                modifier = Modifier
                    .clearAndSetSemantics {}
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
                onValueChange = { next ->
                    val textChanged = next.text != editor.text
                    editor = next
                    if (textChanged) onCodeChange(next.text)
                },
                modifier = Modifier
                    .fillMaxSize()
                    .semantics { contentDescription = editorLabel }
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
