# Top-level auto-print semantics: EVERY visible top-level expression
# auto-prints (not just the final one), print() side effects and
# auto-printed values interleave in statement order, invisible returns
# (assignments, invisible(), print()) stay silent, and deferred warnings
# flush after the statement that raised them.
1 + 1
x <- 5
x * 2
print("side effect")
1:3
invisible(42)
paste("a", "b")
warning("deferred warn")
z <- c(1, 2)
z[3] <- 7
print(z)
4 * 4
