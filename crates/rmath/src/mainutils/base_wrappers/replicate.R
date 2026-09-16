# GNU R base/R/sapply.R; GPL-2.0-or-later, R Core Team.
function(n, expr, simplify = "array")
    sapply(integer(n), eval.parent(substitute(function(...) expr)),
           simplify = simplify)
