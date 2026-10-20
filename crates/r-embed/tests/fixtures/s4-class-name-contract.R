local({
    # Isolate class-name behavior from separately tracked default quoting.
    quoting <- options(useFancyQuotes = FALSE)
    on.exit(options(quoting), add = TRUE)
    ns <- asNamespace("methods")
    make_class <- get("C_R_methods_test_MAKE_CLASS", ns)
    new_class <- get("C_R_methods_test_NEW", ns)
    setClass("ClassNameBase", contains = "VIRTUAL")
    setClass("ClassNameChild", contains = "ClassNameBase", slots = c(x = "numeric"),
             prototype = prototype(x = 7))
    definition <- getClassDef("ClassNameChild")
    stopifnot(is.null(getClassDef("MissingClassNameProbe")),
              isVirtualClass("ClassNameBase"), !isVirtualClass("ClassNameChild"),
              extends("ClassNameChild", "ClassNameBase"),
              !extends("ClassNameBase", "ClassNameChild"))
    for (name in list("ClassNameChild", c("ClassNameChild", "ignored"),
                      as.name("ClassNameChild"))) {
        stopifnot(identical(.Call(make_class, name), definition),
                  identical(.Call(new_class, name)@x, 7))
    }
    for (name in list(NULL, character(), NA_character_, 1L, TRUE, raw(1), list(1), list("ClassNameChild"),
                      1 / 3, 1 + 2i, NA_real_, factor("label"), function() NULL)) {
        for (routine in list(make_class, new_class)) {
            result <- tryCatch(.Call(routine, name), error = conditionMessage)
            print(result)
        }
    }
    old <- options(digits = 4, OutDec = ",")
    on.exit(options(old), add = TRUE)
    print(tryCatch(.Call(make_class, 1 / 3), error = conditionMessage))
    cat("class definition/inheritance/name controls: TRUE\n")
})
