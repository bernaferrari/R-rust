# show.error.locations option plumbing: every value class stock R accepts
# round-trips through getOption, and condition payloads caught by tryCatch
# stay bare (the "(from #n)" marker belongs to top-level rendering only).
options(show.error.locations = TRUE)
a <- getOption("show.error.locations")
options(show.error.locations = "top")
b <- getOption("show.error.locations")
options(show.error.locations = 3)
cc <- getOption("show.error.locations")
cat(class(a), isTRUE(a), "\n")
cat(class(b), identical(b, "top"), "\n")
cat(class(cc), cc == 3, "\n")
msg <- tryCatch(1L + "x", error = function(e) conditionMessage(e))
cat(msg, "\n")
options(show.error.locations = NULL)
cat(is.null(getOption("show.error.locations")), "\n")
