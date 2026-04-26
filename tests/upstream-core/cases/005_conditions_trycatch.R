## Curated from r-source/tests/conditions.R:
## condition objects, tryCatch handlers, warning suppression, and messages.
msg <- tryCatch(stop("bar"), error = function(e) e$message)
print(msg)

warn <- suppressWarnings({
    warning("careful")
    1
})
print(warn)

print(tryCatch(1 + 1, error = function(e) 0))
