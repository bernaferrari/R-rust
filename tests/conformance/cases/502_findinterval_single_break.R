x <- c(0, 1, 2, NA)
show_case <- function(all.inside, rightmost.closed, left.open) {
  cat(paste(findInterval(
    x,
    1,
    all.inside = all.inside,
    rightmost.closed = rightmost.closed,
    left.open = left.open
  ), collapse = "|"), "\n", sep = "")
}
show_case(FALSE, FALSE, FALSE)
show_case(FALSE, FALSE, TRUE)
show_case(FALSE, TRUE, FALSE)
show_case(FALSE, TRUE, TRUE)
show_case(TRUE, FALSE, FALSE)
show_case(TRUE, FALSE, TRUE)
show_case(TRUE, TRUE, FALSE)
show_case(TRUE, TRUE, TRUE)
