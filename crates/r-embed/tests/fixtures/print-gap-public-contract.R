for (gap in c(0, 1, 2, 4)) {
    cat("gap", gap, "\n")
    for (value in list(1:4, c(1.25, -2.5, NA, Inf), c(TRUE, FALSE, NA, TRUE),
                       c(1+2i, -3+4i, NA_complex_, 0i), c("a", "bbb", "c", NA_character_))) {
        print.default(value, print.gap=gap)
        print.default(matrix(value, 2), print.gap=gap)
        m <- matrix(value, 2, dimnames=list(c("", "s.e."), c("short", "longer")))
        print.default(m, print.gap=gap)
        print.default(m, print.gap=gap, width=24)
    }
}
for (value in list(-1, NA, 1025, numeric())) {
    print(tryCatch(print.default(matrix(1:4, 2), print.gap=value),
                   error=function(e) conditionMessage(e)))
    print(matrix(1:4, 2))
}
print.default(matrix(1:4, 2), print.gap=1.2)
print.default(matrix(1:4, 2), print.gap="2")
