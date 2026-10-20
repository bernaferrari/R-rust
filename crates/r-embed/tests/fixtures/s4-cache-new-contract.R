local({
    namespace <- asNamespace("methods")
    cache <- get("C_R_getClassFromCache", namespace)
    construct <- get("C_new_object", namespace)
    message_of <- function(f) tryCatch(f(), error = conditionMessage)
    cat("null-table: ", message_of(function() .Call(cache, "AbsentCacheProbe", NULL)), "\n", sep = "")
    cat("invalid-class: ", message_of(function() .Call(cache, 1L, NULL)), "\n", sep = "")
    cat("null-definition: ", message_of(function() .Call(construct, NULL)), "\n", sep = "")
    definition <- getClassDef("classRepresentation")
    table <- new.env(parent = emptyenv())
    assign("found", definition, envir = table)
    stopifnot(is.null(.Call(cache, character(), NULL)),
              is.null(.Call(cache, "absent", table)),
              identical(.Call(cache, "found", table), definition),
              identical(.Call(cache, definition, NULL), definition))
    setClass("S4PrototypeProbe", slots = c(x = "numeric"),
             prototype = prototype(x = c(1, 2)))
    class_definition <- getClassDef("S4PrototypeProbe")
    first <- .Call(construct, class_definition)
    first@x[1L] <- 99
    second <- .Call(construct, class_definition)
    named <- new("S4PrototypeProbe", x = 4:6)
    stopifnot(isS4(first), isS4(second),
              identical(second@x, c(1, 2)),
              identical(as.numeric(named@x), c(4, 5, 6)))
    setClass("NamedSlotsProbe", representation(x = "numeric", y = "numeric"))
    both <- new("NamedSlotsProbe", x = 1, y = 2)
    stopifnot(identical(c(both@x, both@y), c(1, 2)))
    make_class <- get("C_R_methods_test_MAKE_CLASS", namespace)
    new_class <- get("C_R_methods_test_NEW", namespace)
    original_get_class <- get("getClass", namespace)
    was_locked <- bindingIsLocked("getClass", namespace)
    if (was_locked) unlockBinding("getClass", namespace)
    assign("getClass", function(...) {
        gc()
        original_get_class(...)
    }, envir = namespace)
    on.exit({
        if (bindingIsLocked("getClass", namespace)) unlockBinding("getClass", namespace)
        assign("getClass", original_get_class, envir = namespace)
        if (was_locked) lockBinding("getClass", namespace)
    }, add = TRUE)
    stopifnot(identical(.Call(make_class, "S4PrototypeProbe"), class_definition),
              identical(.Call(new_class, "S4PrototypeProbe")@x, c(1, 2)))
    absent <- message_of(function() .Call(make_class, "AbsentClassProbe"))
    stopifnot(is.character(absent), length(absent) == 1L,
              grepl("AbsentClassProbe", absent, fixed = TRUE),
              grepl("not a defined class", absent, fixed = TRUE))
    cat("undefined-class: TRUE\n")
    cat("cache/prototype/named-slot controls: TRUE\n")
})
