local({
    stopifnot(identical(functions$a(), "shared code literal"),
              identical(functions$b(), "shared code literal"))
    value <- functions$a()
    attr(value, "probe") <- 1L
    value[1L] <- "changed"
    stopifnot(identical(functions$b(), "shared code literal"),
              is.null(attr(functions$a(), "probe")))
    copy <- functions$a
    body(copy) <- quote("changed body")
    stopifnot(identical(copy(), "changed body"),
              identical(functions$a(), "shared code literal"),
              identical(functions$b(), "shared code literal"))
    gc()
    TRUE
})
