# Stock message-fidelity probes: `[[` out-of-bounds wording, parse-error
# wording when input runs out after `if`, ptukey stderr silence, the gc()
# table shape (limit column), and coercion-warning call attribution for
# the complex() constructor.

# do_subset2 out-of-bounds: plain stock wording, no "(dimension N)" suffix
x <- 1:3
cat("oob-vec:", tryCatch({x[[5]]; "ok"}, error = function(e) conditionMessage(e)), "\n")
m <- matrix(1:4, 2)
cat("oob-mat:", tryCatch({m[[7]]; "ok"}, error = function(e) conditionMessage(e)), "\n")

# Parse errors report the input running out ("end of input"), never the
# trailing newline ("end of line"), for operator/incomplete-header inputs
pf <- function(s) {
  tryCatch(parse(text = s), error = function(e)
    paste("input:", grepl("unexpected end of input", conditionMessage(e)),
          "line:", grepl("unexpected end of line", conditionMessage(e))))
}
cat("parse-if:", pf("i2 <- if"), "\n")
cat("parse-if-nl:", pf("i2 <- if\n"), "\n")
cat("parse-plus:", pf("1 +"), "\n")
cat("parse-if-cond:", pf("if(1)"), "\n")

# ptukey/qtukey: converged inputs stay silent on stderr and match stock
print(qtukey(0.95, 2, 10))
print(qtukey(0.5, 3, 6))
print(ptukey(2.0, 2, 10))

# complex() coercion warnings attribute to the complex(...) call, and the
# invalid length.out error still fires after the warning
cat("cx-len:", tryCatch(complex("x"), error = function(e) conditionMessage(e)), "\n")
print(complex(real = "x"))

# gc() table: stock 2-row layout (Ncells/Vcells rows). The COLUMN set is
# build-configuration dependent (the `limit (Mb)` column exists only when
# the vector-size ceiling is enabled), so pin only the stable contract:
# two rows, the leading columns, and the row names.
g <- gc()
print(nrow(g))
print(colnames(g)[1:4])
print(rownames(g))
print(g[1, 1] >= 0, g[2, 1] >= 0)
