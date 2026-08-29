# factor/ordered operands for arithmetic: stock Ops.factor / Ops.ordered
# warn with the method call and return logical NAs.
f <- factor("a")
fo <- ordered("a")
fv <- factor(c("a", "b"))
print(f + 1)
print(1 + fv)
print(fv - c(1, 2))
print(fo * 2)
print(-f)
print(-fo)
# unordered factor relops: == and != compare level strings; ordering warns
print(f == "a")
print(f == 1)
print(f == factor("b"))
print(factor(c("a", "b")) == factor(c("b", "a")))
print(f != NA_character_)
print(fv == c("a", "c"))
print(f < factor("b"))
