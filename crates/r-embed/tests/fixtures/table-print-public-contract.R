local({
    old <- options(width = 60, digits = 7)
    on.exit(options(old))
    stopifnot(!is.primitive(base::print.table),
              identical(environment(base::print.table), asNamespace("base")))
    x <- table(factor(c("a", "b", "a", NA)), useNA = "ifany")
    cat("before\n")
    result <- withVisible(print(x))
    cat("after\n")
    stopifnot(!result$visible, identical(result$value, x))
    print(capture.output({cat("capture before\n"); print(x); cat("capture after\n")}))
    print(x, zero.print = ".", quote = TRUE, right = FALSE)
    y <- structure(matrix(c(0, 2, NA, 7), 2, dimnames = list(row = c("one", "two"), col = c("left", "right"))), class = "table")
    print(y, zero.print = ".", na.print = "missing")
    print(structure(array(1:8, c(2,2,2), dimnames = list(a=c("a1","a2"), b=c("b1","b2"), c=c("c1","c2"))), class="table"))
    print(table(character()))
    z <- structure(c(1.23456, 100.2345), dim = 2L, dimnames = list(c("low", "high")), class = "table")
    print(z, digits = 3)
    for (title in c("", NA_character_, "NA", "title")) {
        a <- array(c("a", "bb"), 2L, dimnames = setNames(list(c("one", "two")), title))
        print(a, quote = FALSE)
    }
    named <- setNames(c(1L, 2L), c(NA_character_, "NA"))
    print(named)
    local({
        print.table <- function(x, ...) { gc(); cat("custom table callback\n"); invisible(x) }
        value <- withVisible(print(x))
        stopifnot(!value$visible, identical(value$value, x))
    })
    TRUE
})
