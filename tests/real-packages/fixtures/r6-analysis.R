suppressPackageStartupMessages(library(R6))
stopifnot(as.character(packageVersion("R6")) == "2.6.1")
Queue <- R6Class("Queue", public = list(
    items = numeric(),
    add = function(value) self$items <- c(self$items, value),
    mean = function() mean(self$items)
))
queue <- Queue$new()
for (value in c(3, 7, 11)) queue$add(value)
stopifnot(is.R6(queue), identical(queue$items, c(3, 7, 11)), queue$mean() == 7)
cat("Loaded R6 2.6.1; analyzed observations with mean 7\n")
