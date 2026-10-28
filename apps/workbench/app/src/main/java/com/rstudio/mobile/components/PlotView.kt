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
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.FileDownload
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.ZoomIn
import androidx.compose.material.icons.filled.ZoomOut
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
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import com.rstudio.mobile.R
import com.rstudio.mobile.runtime.PlotImage
import kotlin.math.roundToInt

private const val MIN_PLOT_SCALE = 0.5f
private const val MAX_PLOT_SCALE = 8f
private const val PLOT_ZOOM_STEP = 1.25f

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
    val zoomDescription = stringResource(R.string.plot_zoom_percent, (scale * 100).roundToInt())
    val actionModifier = Modifier.sizeIn(minWidth = 48.dp, minHeight = 48.dp)

    Column(Modifier.fillMaxSize()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 10.dp, vertical = 6.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f).padding(end = 8.dp)) {
                Text(stringResource(R.string.plots_title), style = MaterialTheme.typography.titleMedium)
                Text(
                    pluralStringResource(R.plurals.plot_session_count, plots.size, plots.size),
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            FilledTonalButton(onClick = onRender, enabled = !isRunning) {
                Text(stringResource(R.string.plot_render))
            }
        }
        Row(
            modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())
                .padding(horizontal = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(
                onClick = { scale = (scale / PLOT_ZOOM_STEP).coerceAtLeast(MIN_PLOT_SCALE) },
                enabled = plot != null && scale > MIN_PLOT_SCALE,
                modifier = actionModifier.semantics { stateDescription = zoomDescription },
            ) {
                Icon(Icons.Default.ZoomOut, contentDescription = stringResource(R.string.plot_zoom_out))
            }
            Text(zoomDescription, style = MaterialTheme.typography.labelMedium)
            IconButton(
                onClick = { scale = (scale * PLOT_ZOOM_STEP).coerceAtMost(MAX_PLOT_SCALE) },
                enabled = plot != null && scale < MAX_PLOT_SCALE,
                modifier = actionModifier.semantics { stateDescription = zoomDescription },
            ) {
                Icon(Icons.Default.ZoomIn, contentDescription = stringResource(R.string.plot_zoom_in))
            }
            IconButton(onClick = ::resetViewport, enabled = plot != null, modifier = actionModifier) {
                Icon(Icons.Default.Refresh, contentDescription = stringResource(R.string.plot_reset_zoom))
            }
            IconButton(onClick = onExport, enabled = plot != null, modifier = actionModifier) {
                Icon(Icons.Default.FileDownload, contentDescription = stringResource(R.string.plot_export_png))
            }
            IconButton(onClick = onShare, enabled = plot != null, modifier = actionModifier) {
                Icon(Icons.Default.Share, contentDescription = stringResource(R.string.plot_share))
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
                        label = { Text(stringResource(R.string.plot_history_item, index + 1)) },
                    )
                }
            }
        }
        Box(
            modifier = Modifier.fillMaxSize().pointerInput(plot?.id) {
                detectTransformGestures { _, pan, zoom, _ ->
                    scale = (scale * zoom).coerceIn(MIN_PLOT_SCALE, MAX_PLOT_SCALE)
                    offset = IntOffset((offset.x + pan.x).roundToInt(), (offset.y + pan.y).roundToInt())
                }
            },
            contentAlignment = Alignment.Center,
        ) {
            val bitmap = remember(plot?.id) { plot?.pngBytes?.let { BitmapFactory.decodeByteArray(it, 0, it.size) } }
            if (bitmap != null) {
                Image(
                    bitmap = bitmap.asImageBitmap(),
                    contentDescription = stringResource(
                        R.string.plot_image_description,
                        plot?.width ?: bitmap.width,
                        plot?.height ?: bitmap.height,
                    ),
                    modifier = Modifier.fillMaxSize(0.95f).graphicsLayer(
                        scaleX = scale,
                        scaleY = scale,
                        translationX = offset.x.toFloat(),
                        translationY = offset.y.toFloat(),
                    ),
                    contentScale = ContentScale.Fit,
                )
            } else {
                Text(stringResource(R.string.plot_empty_state), color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}
