package com.rstudio.mobile.components

import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class WorkbenchComponentsTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun dataViewerExplainsHowToPopulateIt() {
        compose.setContent { MaterialTheme { DataTableView(table = null) } }
        compose.onNodeWithText("No table result").assertIsDisplayed()
        compose.onNodeWithText("Import a CSV or evaluate a data.frame to inspect rows and columns here.")
            .assertIsDisplayed()
    }

    @Test
    fun packageBrowserHasHonestEmptyState() {
        compose.setContent {
            MaterialTheme {
                PackageBrowser(packages = emptyList(), loaded = emptySet(), onRefresh = {}, onLoad = {})
            }
        }
        compose.onNodeWithText("Pure-R packages supported").assertIsDisplayed()
        compose.onNodeWithText("No installed pure-R packages were found.").assertIsDisplayed()
    }
}
