## Filter and mapply naming semantics (upstream fidelity)

## Filter: upstream is x[vapply(x, f, logical(1))] — element types AND
## names of kept elements are preserved; zero matches give an empty list,
## never NULL (R6's get_functions relies on the NULL contract separately).
invisible(Sys.setlocale("LC_COLLATE", "C"))
f1 <- Filter(function(v) v > 1, list(a = 1, b = 2, c = 3))
cat(paste(names(f1), unlist(f1), sep = "="), "\n", sep = "")
f2 <- Filter(is.function, list(x = 1, g = function() 42))
cat(length(f2), names(f2), f2$g(), "\n", sep = "|")
f3 <- Filter(function(v) v > 99, list(a = 1, b = 2))
cat(typeof(f3), length(f3), "\n", sep = "|")
f4 <- Filter(function(v) v == "b", c("a", "b", "c"))
cat(f4, "\n", sep = "")
f5 <- Filter(function(v) v %% 2 == 0, c(1L, 2L, 3L, 4L))
cat(paste(f5, collapse = ","), "\n", sep = "")

## mapply: USE.NAMES (default TRUE) takes result names from the first
## varying argument — its names attribute when present, else the VALUES
## of a character first argument (R6's clone passes names(copies)).
m1 <- mapply(function(n, v) v, c("a", "b"), list(10, 20), SIMPLIFY = FALSE)
cat(paste(names(m1), unlist(m1), sep = "="), "\n", sep = "")
m2 <- mapply(function(x) x * 2, c(k = 1, m = 2))
cat(paste(names(m2), m2), "\n", sep = "")
m3 <- mapply(function(a, b) a + b, 1:2, 3:4, SIMPLIFY = FALSE)
cat(length(m3), is.null(names(m3)), "\n", sep = "|")
