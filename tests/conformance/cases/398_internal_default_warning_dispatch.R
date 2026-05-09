x <- .Internal(.dfltWarn("direct warning", NULL))
print(.Internal(printDeferredWarnings()))
print("after warning")
