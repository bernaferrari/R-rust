if (!exists("rownames_to_column")) {
  rownames_to_column <- function(x, var = "rowname") {
    x[[var]] <- row.names(x)
    x[c(var, setdiff(names(x), var))]
  }
}

if (!exists("column_to_rownames")) {
  column_to_rownames <- function(x, var = "rowname") {
    row.names(x) <- x[[var]]
    x[[var]] <- NULL
    x
  }
}

if (!exists("relocate")) {
  relocate <- function(x, cols, .before = NULL, .after = NULL) {
    nm <- names(x)
    cols <- intersect(cols, nm)
    rest <- setdiff(nm, cols)
    if (!is.null(.before)) {
      out <- append(rest, cols, after = match(.before, rest) - 1)
    } else if (!is.null(.after)) {
      out <- append(rest, cols, after = match(.after, rest))
    } else {
      out <- c(cols, rest)
    }
    x[out]
  }
}

d <- data.frame(a = 1:2, b = 3:4)
d2 <- rownames_to_column(d, "id")
print(paste(names(d2), collapse = "|"))
print(paste(d2$id, collapse = "|"))

d3 <- column_to_rownames(d2, "id")
print(paste(names(d3), collapse = "|"))
print(paste(row.names(d3), collapse = "|"))

d4 <- relocate(d2, "b", .before = "id")
print(paste(names(d4), collapse = "|"))
