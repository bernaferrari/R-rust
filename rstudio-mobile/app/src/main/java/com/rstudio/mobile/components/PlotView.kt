package com.rstudio.mobile.components

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import com.rstudio.mobile.runtime.PlotImage
import kotlin.math.roundToInt

@Composable
fun PlotView(plot: PlotImage?, isRunning: Boolean, onRender: () -> Unit) {
    var scale by remember { mutableFloatStateOf(1f) }
    var offset by remember { mutableStateOf(IntOffset.Zero) }

    Column(Modifier.fillMaxSize()) {
        androidx.compose.foundation.layout.Row(
            modifier = Modifier.fillMaxWidth().padding(10.dp),
            horizontalArrangement = androidx.compose.foundation.layout.Arrangement.SpaceBetween,
        ) {
            Text("Plots", style = MaterialTheme.typography.titleMedium)
            FilledTonalButton(onClick = onRender, enabled = !isRunning) { Text("Render") }
        }
        Box(
            modifier = Modifier
                .fillMaxSize()
                .pointerInput(Unit) {
                    detectTransformGestures { _, pan, zoom, _ ->
                        scale *= zoom
                        offset = IntOffset(
                            x = (offset.x + pan.x).roundToInt(),
                            y = (offset.y + pan.y).roundToInt()
                        )
                    }
                },
            contentAlignment = Alignment.Center
        ) {
            val bitmap = remember(plot?.pngBytes) {
                plot?.pngBytes?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }
            }
            if (bitmap != null) {
                Image(
                    bitmap = bitmap.asImageBitmap(),
                    contentDescription = "Rendered R plot",
                    modifier = Modifier
                        .fillMaxSize(0.95f)
                        .graphicsLayer(
                            scaleX = scale,
                            scaleY = scale,
                            translationX = offset.x.toFloat(),
                            translationY = offset.y.toFloat()
                        ),
                    contentScale = ContentScale.Fit,
                )
            } else {
                Text(
                    text = "Run plot(...) code or tap Render.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}
