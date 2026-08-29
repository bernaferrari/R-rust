# => pipe-bind and @/slot stock error behavior (R 4.7 pipebind, gated by
# _R_USE_PIPEBIND_; slot errors per main/attrib.c R_do_slot/do_AT).

p <- function(e) {
  r <- tryCatch(paste("OK:", paste(deparse({v <- e; v}), collapse=" ")),
                error = function(err) paste("ERR:", conditionMessage(err)))
  cat(deparse(substitute(e)), "=>", r, "\n")
}
r <- function(s) {
  rr <- tryCatch({ v2 <- parse(text = s); paste("PARSED:", paste(deparse(v2), collapse="; ")) },
                 error = function(e) paste("ERR:", conditionMessage(e)))
  cat(sprintf("%-30s => %s\n", s, rr))
}

# --- '=>' is disabled by default ---
r("1 |> y => log(y)")
r("a => b")

# --- slot miss errors (messages must match stock R exactly) ---
setClass("Foo", representation(x = "numeric"))
f <- new("Foo", x = 1)
p(f@x)
p(f@missing)
p(slot(f, "missing"))
p(slot(1, "x"))
p(1@x)
p("s"@x)
p(NULL@x)
p(slot(matrix(1, 1), "zz"))
p(slot(list(a = 1), "a"))
p(f@.Data)
p(slot(f, "names"))
p(slot(f, NA))
p(slot(f, 5))
p(f@x + 1)

