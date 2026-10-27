use r_embed::RSession;

#[test]
fn empty_subscript_syntax_preserves_missing_arguments_for_reads_and_writes() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                r#"
                ast_one <- as.list(quote(x[]))
                ast_two <- as.list(quote(x[[]]))
                ast_pair <- as.list(quote(x[,]))
                stopifnot(
                    length(ast_one) == 3L,
                    identical(ast_one[[3L]], quote(expr=)),
                    length(ast_two) == 3L,
                    identical(ast_two[[3L]], quote(expr=)),
                    length(ast_pair) == 4L,
                    identical(ast_pair[[3L]], quote(expr=)),
                    identical(ast_pair[[4L]], quote(expr=))
                )

                nargs_default <- function(x, y = 1L, z) nargs()
                nargs_dots <- function(...) nargs()
                stopifnot(
                    nargs_default(1L) == 1L,
                    nargs_default(1L, , ) == 3L,
                    nargs_dots(1L, 2L) == 2L,
                    nargs_dots(... = list(3L, 4L)) == 1L
                )

                x <- 1:3
                stopifnot(identical(x[], x))
                stopifnot(identical(
                    tryCatch(x[[]], error = conditionMessage),
                    "missing subscript"
                ))
                stopifnot(identical(
                    tryCatch(x[,], error = conditionMessage),
                    "incorrect number of dimensions"
                ))

                `[.probe` <- function(x, i, j, ..., drop = TRUE) {
                    if (nargs() == 2L) {
                        stopifnot(missing(i), missing(j))
                    } else {
                        stopifnot(nargs() == 3L, missing(i), missing(j))
                    }
                    x
                }
                `[<-.probe` <- function(x, i, j, value) {
                    if (nargs() == 3L) {
                        stopifnot(missing(i), missing(j))
                    } else {
                        stopifnot(nargs() == 4L, missing(i), missing(j))
                    }
                    x
                }
                probe <- structure(1L, class = "probe")
                invisible(probe[])
                invisible(probe[,])
                invisible(probe[] <- 2L)
                invisible(probe[,] <- 2L)

                vector <- 1:3
                vector[] <- 4:6
                stopifnot(identical(vector, 4:6), identical(vector[], vector))

                matrix_value <- matrix(1:4, 2L)
                matrix_value[] <- 5:8
                stopifnot(
                    identical(matrix_value, matrix(5:8, 2L)),
                    identical(matrix_value[], matrix_value),
                    identical(matrix_value[,], matrix_value)
                )

                list_value <- list(a = 1L, b = 2L)
                list_value[] <- list(3L, 4L)
                stopifnot(identical(list_value, list(a = 3L, b = 4L)))

                df <- data.frame(x = 1:10, y = 11:20, row.names = letters[1:10])
                test <- df
                test[] <- lapply(df, factor)
                stopifnot(
                    identical(names(test), names(df)),
                    identical(row.names(test), row.names(df)),
                    all(vapply(test, is.factor, logical(1))),
                    identical(as.character(test$x), as.character(df$x)),
                    identical(as.character(test$y), as.character(df$y))
                )

                rhs_calls <- 0L
                rhs <- function() {
                    rhs_calls <<- rhs_calls + 1L
                    list(7L, 8L)
                }
                list_value[] <- rhs()
                stopifnot(rhs_calls == 1L, identical(list_value, list(a = 7L, b = 8L)))
                cat("empty-subscript-contract: PASS\n")
                "#
            )
            .unwrap(),
        "empty-subscript-contract: PASS\n"
    );
}
