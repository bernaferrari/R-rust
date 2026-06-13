r <- regexec("(ab)(c+)", c("xxabccc", "nomatch"))
print(r[[1]])
print(attr(r[[1]], "match.length"))
print(r[[2]])
print(attr(r[[2]], "match.length"))

m <- regexpr("a+b", "xxaaabyy")
print(m)
print(attr(m, "match.length"))

g <- gregexpr("a+", "baaacaa")
print(g[[1]])
print(attr(g[[1]], "match.length"))

print(grepl("^(cat|dog)+$", c("catdog", "dogbird")))
