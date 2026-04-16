package com.rstudio.mobile.components

import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.IntOffset
import kotlin.math.roundToInt

@Composable
fun PlotView() {
    var scale by remember { mutableFloatStateOf(1f) }
    var offset by remember { mutableStateOf(IntOffset.Zero) }
    val onSurface = MaterialTheme.colorScheme.onSurface
    val outline = MaterialTheme.colorScheme.outline.copy(alpha = 0.2f)
    val primary = MaterialTheme.colorScheme.primary

    Box(
        modifier = Modifier
            .fillMaxSize()
            .pointerInput(Unit) {
                detectTransformGestures { centroid, pan, zoom, rotation ->
                    scale *= zoom
                    offset = IntOffset(
                        x = (offset.x + pan.x).roundToInt(),
                        y = (offset.y + pan.y).roundToInt()
                    )
                }
            },
        contentAlignment = Alignment.Center
    ) {
        androidx.compose.foundation.Canvas(
            modifier = Modifier
                .fillMaxSize(0.95f)
                .graphicsLayer(
                    scaleX = scale,
                    scaleY = scale,
                    translationX = offset.x.toFloat(),
                    translationY = offset.y.toFloat()
                )
        ) {
            // Sample plot rendering
            val canvasWidth = size.width
            val canvasHeight = size.height
            val padding = 40f

            // Axes
            drawLine(
                color = onSurface,
                start = androidx.compose.ui.geometry.Offset(padding, canvasHeight - padding),
                end = androidx.compose.ui.geometry.Offset(canvasWidth - padding, canvasHeight - padding),
                strokeWidth = 2f
            )
            drawLine(
                color = onSurface,
                start = androidx.compose.ui.geometry.Offset(padding, padding),
                end = androidx.compose.ui.geometry.Offset(padding, canvasHeight - padding),
                strokeWidth = 2f
            )

            // Grid lines
            for (i in 0..4) {
                val y = padding + (canvasHeight - 2 * padding) * i / 4
                drawLine(
                    color = outline,
                    start = androidx.compose.ui.geometry.Offset(padding, y),
                    end = androidx.compose.ui.geometry.Offset(canvasWidth - padding, y),
                    strokeWidth = 1f
                )
            }

            // Sample points
            val points = listOf(
                0.1f, 0.3f, 0.45f, 0.6f, 0.75f, 0.8f, 0.65f, 0.5f, 0.35f, 0.2f
            )

            points.forEachIndexed { index, value ->
                val x = padding + (canvasWidth - 2 * padding) * index / (points.size - 1)
                val y = canvasHeight - padding - (canvasHeight - 2 * padding) * value
                drawCircle(
                    color = primary,
                    radius = 6f,
                    center = androidx.compose.ui.geometry.Offset(x, y)
                )
            }
        }
    }
}
