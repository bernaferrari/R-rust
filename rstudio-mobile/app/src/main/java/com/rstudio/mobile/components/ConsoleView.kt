package com.rstudio.mobile.components

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.rstudio.mobile.util.AnsiParser

@Composable
fun ConsoleView() {
    val consoleLines = listOf(
        "\u001B[32mR version 4.3.1 (2023-06-16) -- \"Beagle Scouts\"\u001B[0m",
        "Copyright (C) 2023 The R Foundation for Statistical Computing",
        "Platform: aarch64-apple-darwin20 (64-bit)",
        "",
        "R is free software and comes with ABSOLUTELY NO WARRANTY.",
        "You are welcome to redistribute it under certain conditions.",
        "Type 'license()' or 'licence()' for distribution details.",
        "",
        "  Natural language support but running in an English locale",
        "",
        "R is a collaborative project with many contributors.",
        "Type 'contributors()' for more information and",
        "'citation()' on how to cite R or R packages in publications.",
        "",
        "Type 'demo()' for some demos, 'help()' for on-line help, or",
        "'help.start()' for an HTML browser interface to help.",
        "Type 'q()' to quit R.",
        "",
        "\u001B[34m> \u001B[0m"
    )

    val listState = rememberLazyListState()

    LaunchedEffect(consoleLines.size) {
        listState.animateScrollToItem(consoleLines.size - 1)
    }

    Box(Modifier.fillMaxSize()) {
        LazyColumn(
            state = listState,
            modifier = Modifier.fillMaxSize().padding(8.dp)
        ) {
            items(consoleLines) { line ->
                Text(
                    text = AnsiParser.parse(line),
                    style = MaterialTheme.typography.bodyMedium.copy(
                        fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                        fontSize = 13.sp,
                        lineHeight = 18.sp
                    )
                )
            }
        }
    }
}
