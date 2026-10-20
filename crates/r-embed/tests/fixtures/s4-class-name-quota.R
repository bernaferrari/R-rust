local({
    ns <- asNamespace("methods")
    make <- get("C_R_methods_test_MAKE_CLASS", ns)
    saved <- get("getClass", ns)
    locked <- bindingIsLocked("getClass", ns)
    if (locked) unlockBinding("getClass", ns)
    assign("getClass", function(...) stop("lookup reached"), envir = ns)
    on.exit({
        assign("getClass", saved, envir = ns)
        if (locked) lockBinding("getClass", ns)
    }, add = TRUE)
    name <- strrep("n", 4000000L)
    padding <- vector("list", 100L)
    for (i in seq_len(100L)) {
        ok <- tryCatch({ padding[[i]] <- raw(1000000L); TRUE }, error = function(e) FALSE)
        if (!ok) break
    }
    tryCatch({ .Call(make, name); "unexpected success" }, error = conditionMessage)
})
