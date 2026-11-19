condition <- tryCatch(stop(), error = identity)
stopifnot(identical(sprintf("%s", condition), sprintf("%s", as.character(condition))))

as.character.sprintf_label <- function(x, ...) paste0("label ", unclass(x)[1L])
as.double.sprintf_cost <- function(x, ...) {
    gc()
    as.double(unclass(x)[1L])
}
label <- structure(list("A", "B"), class = "sprintf_label")
cost <- structure(as.list(c(1.25, 2.5)), class = "sprintf_cost")
stopifnot(identical(sprintf("%s", label), "label A"),
          identical(sprintf("%s: %.2f", label, cost), "label A: 1.25"),
          identical(sprintf("%s/%d", label, 1:2), c("label A/1", "label A/2")))

as.character.sprintf_pair <- function(x, ...) c("first", "second")
pair <- structure(as.list(1:6), class = "sprintf_pair")
err <- tryCatch(sprintf(rep("%s", 3), pair), error = conditionMessage)
stopifnot(identical(err, "arguments cannot be recycled to the same length"),
          identical(sprintf("%s %d", "recovered", 42L), "recovered 42"))
cat("sprintf recalculates recycling after S3 coercion\n")
