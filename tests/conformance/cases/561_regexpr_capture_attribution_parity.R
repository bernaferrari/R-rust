# regexpr(perl=TRUE) capture-group attribution — the grep.c do_regexpr
# port contract that praise's replace_one_template depends on:
# capture.start/capture.length matrices + capture.names, substring() over
# those attributes, and sub(fixed=TRUE) reassembly.
template_pattern <- "\\$\\{([^\\}]+)\\}"
template <- "You are ${adjective}"

m <- regexpr(template_pattern, template, perl = TRUE)
cat("pos:", as.integer(m), "len:", as.integer(attr(m, "match.length")), "\n")
cat("capture.start:", as.integer(attr(m, "capture.start")), "\n")
cat("capture.length:", as.integer(attr(m, "capture.length")), "\n")
cat("capture.names:", paste(attr(m, "capture.names"), collapse = ","), "\n")
cat("dim:", paste(dim(attr(m, "capture.start")), collapse = ","), "\n")
cat("attr order:", paste(names(attributes(m)), collapse = ","), "\n")

# praise's replace_one_template, verbatim
template1 <- substring(template, m, m + attr(m, "match.length") - 1L)
part <- substring(
  template,
  attr(m, "capture.start"),
  attr(m, "capture.start") + attr(m, "capture.length") - 1L
)
cat("template1:", template1, "\n")
cat("part:", part, "\n")
cat("sub:", sub(template1, "pioneering", template, fixed = TRUE), "\n")

# multi-element: non-participating group -> 0/0, no-match row -> -1/-1,
# NA row -> NA (grep.c PR#16484 keeps the NA initialization)
m2 <- regexpr("(a)|(b)", c("b", "a", "c", NA), perl = TRUE)
cat("m2:", paste(as.integer(m2), collapse = ","), "\n")
cat("m2.start:", paste(as.integer(attr(m2, "capture.start")), collapse = ","), "\n")
cat("m2.length:", paste(as.integer(attr(m2, "capture.length")), collapse = ","), "\n")

# named groups feed capture.names and the matrix dimnames
m3 <- regexpr("(?<word>[a-z]+)([0-9]*)", c("ab12", "zz"), perl = TRUE)
cat("m3.names:", paste(attr(m3, "capture.names"), collapse = ","), "\n")
print(attr(m3, "capture.start"))
print(attr(m3, "capture.length"))

# default (TRE) and fixed engines never attach capture attributes
cat("ere null:", is.null(attr(regexpr("(a)", "a"), "capture.start")), "\n")
cat("fixed null:", is.null(attr(regexpr("(a)", "a", fixed = TRUE), "capture.start")), "\n")

# full praise() flow with a deterministic word list
praise_parts <- list(adjective = "pioneering")
cat("praise:", sub(template1, praise_parts[[tolower(part)]], template, fixed = TRUE), "\n")
