# Stock `?` help operator: prefix and binary forms parse to `?` calls at the
# lowest precedence tier; incomplete forms are stock parse errors.

q <- function(s) deparse(parse(text = s)[[1]])
cat("prefix:", q("?x"), "\n")
cat("binary:", q("x ? y"), "\n")
cat("double-prefix:", q("??x"), "\n")
cat("prefix-binary:", q("? a ? b"), "\n")
cat("left-assoc:", q("a ? b ? c"), "\n")
cat("rhs-prec:", q("a ? b + c"), "\n")
cat("lhs-prec:", q("1 + 2 ? foo"), "\n")
cat("unary-lhs:", q("-1 ? x"), "\n")
cat("prefix-after-op:", q("?\nTRUE"), "\n")
cat("nl-after-op:", q("1 ?\n2"), "\n")
cat("rhs-prefix:", q("a ? ?b"), "\n")
cat("in-call:", q("f(?x)"), "\n")
cat("prefix-fn:", q("?function(x) x"), "\n")
pf <- function(s) tryCatch({parse(text = s); "ok"},
  error = function(e)
    paste("unexpected-end-of-input:", grepl("unexpected end of input",
      conditionMessage(e))))
cat("err-?", pf("?"), "\n")
cat("err-1?", pf("1 ?"), "\n")
cat("err-x?", pf("x ? "), "\n")
