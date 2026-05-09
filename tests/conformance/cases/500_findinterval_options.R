vec <- c(1, 2, 4)
x <- c(-Inf, 0, 1, 1.5, 2, 4, 5, Inf, NA, NaN)
show_case <- function(rightmost.closed, all.inside, left.open) {
  cat(paste(findInterval(
    x,
    vec,
    rightmost.closed = rightmost.closed,
    all.inside = all.inside,
    left.open = left.open
  ), collapse = "|"), "\n", sep = "")
}
show_case(FALSE, FALSE, FALSE)
show_case(TRUE, FALSE, FALSE)
show_case(FALSE, TRUE, FALSE)
show_case(FALSE, FALSE, TRUE)
show_case(TRUE, FALSE, TRUE)
show_case(TRUE, TRUE, TRUE)
