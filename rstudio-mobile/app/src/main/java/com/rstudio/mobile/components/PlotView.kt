package com.rstudio.mobile.components

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.FileDownload
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import com.rstudio.mobile.runtime.PlotImage
import kotlin.math.roundToInt

@Composable
fun PlotView(
    plot: PlotImage?,
    plots: List<PlotImage>,
    isRunning: Boolean,
    onRender: () -> Unit,
    onSelect: (Long) -> Unit,
    onExport: () -> Unit,
    onShare: () -> Unit,
) {
    var scale by remember { mutableFloatStateOf(1f) }
    var offset by remember { mutableStateOf(IntOffset.Zero) }
    fun resetViewport() { scale = 1f; offset = IntOffset.Zero }
    LaunchedEffect(plot?.id) { resetViewport() }

    Column(Modifier.fillMaxSize()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 10.dp, vertical = 6.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column {
                Text("Plots", style = MaterialTheme.typography.titleMedium)
                Text("${plots.size} in this session", style = MaterialTheme.typography.bodySmall)
            }
            Row {
                IconButton(onClick = ::resetViewport, enabled = plot != null) {
                    Icon(Icons.Default.Refresh, contentDescription = "Reset plot zoom")
                }
                IconButton(onClick = onExport, enabled = plot != null) {
                    Icon(Icons.Default.FileDownload, contentDescription = "Export plot as PNG")
                }
                IconButton(onClick = onShare, enabled = plot != null) {
                    Icon(Icons.Default.Share, contentDescription = "Share plot")
                }
                FilledTonalButton(onClick = onRender, enabled = !isRunning) { Text("Render") }
            }
        }
        if (plots.size > 1) {
            Row(
                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 10.dp),
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                plots.forEachIndexed { index, item ->
                    androidx.compose.material3.FilterChip(
                        selected = item.id == plot?.id,
                        onClick = { onSelect(item.id) },
                        label = { Text("Plot ${index + 1}") },
                    )
                }
            }
        }
        Box(
            modifier = Modifier.fillMaxSize().pointerInput(plot?.id) {
                detectTransformGestures { _, pan, zoom, _ ->
                    scale = (scale * zoom).coerceIn(0.5f, 8f)
                    offset = IntOffset((offset.x + pan.x).roundToInt(), (offset.y + pan.y).roundToInt())
                }
            },
            contentAlignment = Alignment.Center,
        ) {
            val bitmap = remember(plot?.id) { plot?.pngBytes?.let { BitmapFactory.decodeByteArray(it, 0, it.size) } }
            if (bitmap != null) {
                Image(
                    bitmap = bitmap.asImageBitmap(),
                    contentDescription = "R plot, ${plot?.width ?: bitmap.width} by ${plot?.height ?: bitmap.height} pixels",
                    modifier = Modifier.fillMaxSize(0.95f).graphicsLayer(
                        scaleX = scale,
                        scaleY = scale,
                        translationX = offset.x.toFloat(),
                        translationY = offset.y.toFloat(),
                    ),
                    contentScale = ContentScale.Fit,
                )
            } else {
                Text("Run plotting code or choose Render.", color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}
