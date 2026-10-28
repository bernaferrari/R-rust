package com.rstudio.mobile.components

import android.graphics.Bitmap
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.InputMode
import androidx.compose.ui.input.InputModeManager
import androidx.compose.ui.platform.LocalInputModeManager
import androidx.compose.ui.unit.dp
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertWidthIsAtLeast
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.pressKey
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.rstudio.mobile.runtime.PlotImage
import java.io.ByteArrayOutputStream
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class PlotViewAccessibilityTest {
    @get:Rule
    val compose = createComposeRule()

    private fun plot(id: Long): PlotImage {
        val bitmap = Bitmap.createBitmap(2, 3, Bitmap.Config.ARGB_8888)
        val bytes = ByteArrayOutputStream()
        bitmap.compress(Bitmap.CompressFormat.PNG, 100, bytes)
        bitmap.recycle()
        return PlotImage(id = id, width = 2, height = 3, pngBytes = bytes.toByteArray())
    }

    @OptIn(ExperimentalTestApi::class, ExperimentalComposeUiApi::class)
    @Test
    fun plotZoomSupportsKeyboardActivationBoundsAndReset() {
        val image = plot(1)
        lateinit var inputModeManager: InputModeManager
        compose.setContent {
            inputModeManager = LocalInputModeManager.current
            MaterialTheme {
                PlotView(image, listOf(image), false, {}, {}, {}, {})
            }
        }
        compose.onNodeWithText("1 plot in this session").assertExists()
        val zoomIn = compose.onNodeWithContentDescription("Zoom in on plot")
        val zoomOut = compose.onNodeWithContentDescription("Zoom out of plot")
        zoomIn.assertHeightIsAtLeast(48.dp).assertWidthIsAtLeast(48.dp)
        zoomOut.assertHeightIsAtLeast(48.dp).assertWidthIsAtLeast(48.dp)
        compose.runOnIdle { inputModeManager.requestInputMode(InputMode.Keyboard) }
        zoomIn.performSemanticsAction(SemanticsActions.RequestFocus)
        zoomIn.assertIsFocused()
        zoomIn.performKeyInput { pressKey(Key.Enter) }
        zoomIn.assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "125%"))
        repeat(9) { zoomIn.performClick() }
        compose.onNodeWithText("800%").assertExists()
        zoomIn.assertIsNotEnabled()
        compose.onNodeWithContentDescription("Reset plot zoom").performClick()
        compose.onNodeWithText("100%").assertExists()
        repeat(4) { zoomOut.performClick() }
        compose.onNodeWithText("50%").assertExists()
        zoomOut.assertIsNotEnabled()
        zoomIn.assertIsEnabled()
    }

    @Test
    fun plotSelectionResetsZoomAndPreservesHistoryAndExportActions() {
        val first = plot(1)
        val second = plot(2)
        val selected = mutableStateOf(first)
        var exports = 0
        var shares = 0
        compose.setContent {
            MaterialTheme {
                PlotView(
                    selected.value, listOf(first, second), false, {},
                    { selected.value = if (it == first.id) first else second },
                    { exports++ }, { shares++ },
                )
            }
        }
        compose.onNodeWithContentDescription("Zoom in on plot").performClick()
        compose.onNodeWithText("125%").assertExists()
        compose.onNodeWithText("Plot 2").performClick()
        compose.onNodeWithText("100%").assertExists()
        compose.onNodeWithContentDescription("R plot, 2 by 3 pixels").assertExists()
        compose.onNodeWithText("2 plots in this session").assertExists()
        compose.onNodeWithContentDescription("Export plot as PNG").performClick()
        compose.onNodeWithContentDescription("Share plot").performClick()
        compose.runOnIdle {
            assertEquals(second.id, selected.value.id)
            assertEquals(1, exports)
            assertEquals(1, shares)
        }
    }

    @Test
    fun emptyPlotDisablesPlotActionsAndExplainsRendering() {
        compose.setContent {
            MaterialTheme { PlotView(null, emptyList(), true, {}, {}, {}, {}) }
        }
        for (label in listOf("Zoom in on plot", "Zoom out of plot", "Reset plot zoom", "Export plot as PNG", "Share plot")) {
            compose.onNodeWithContentDescription(label).assertIsNotEnabled()
        }
        compose.onNodeWithText("Render").assertIsNotEnabled()
        compose.onNodeWithText("0 plots in this session").assertExists()
        compose.onNodeWithText("Run plotting code or choose Render.").assertExists()
    }
}
