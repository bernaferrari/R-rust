out <- character()

f <- function() {
  on.exit(out <<- c(out, "first"))
  on.exit(out <<- c(out, "second"), add = TRUE)
  on.exit(out <<- c(out, "zero"), add = TRUE, after = FALSE)
  "body"
}

print(f())
print(paste(out, collapse = "|"))

g <- function() {
  out <<- character()
  on.exit(out <<- c(out, "kept"))
  on.exit(NULL)
  "body"
}

print(g())
print(out)
