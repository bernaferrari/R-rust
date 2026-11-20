local({
    db <- rbind(
        c("App", "Base (>= 2.0), Data", "Link (== 1.1)", "Missing"),
        c("Base", "R (>= 4.4), Leaf", NA, "Helper"),
        c("Data", "Leaf", "Base", NA),
        c("Leaf", NA, "", ""),
        c("Link", NA, "Leaf", ""),
        c("Helper", "Leaf", NA, "")
    )
    colnames(db) <- c("Package", "Depends", "Imports", "Suggests")
    db <- cbind(db, LinkingTo = rep("", nrow(db)))

    show_dependencies <- function(result) {
        for (i in seq_along(result)) {
            dependencies <- result[[i]]
            value <- if (is.null(dependencies)) "<NULL>" else paste(dependencies, collapse = ",")
            cat(names(result)[[i]], "=", value, "\n", sep = "")
        }
    }
    show_dependencies(tools::package_dependencies(c("App", "Unknown", "App"), db, which = "strong"))
    show_dependencies(tools::package_dependencies("App", db, which = c("Depends", "Imports"), recursive = TRUE))
    show_dependencies(tools::package_dependencies("Leaf", db, which = "strong", recursive = TRUE, reverse = TRUE))
    show_dependencies(tools::package_dependencies("App", db, which = "Suggests", recursive = "strong"))

    scanned <- .Call(tools:::C_package_dependencies_scan,
                     c(NA_character_, "R (>= 4.4)", "A.pkg (>= 0.1), Base", "Base"))
    cat(paste(scanned, collapse = ","), "\n", sep = "")
    cat(tryCatch(.Call(tools:::C_package_dependencies_scan, 42L), error = conditionMessage), "\n", sep = "")
})
