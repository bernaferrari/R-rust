print(grepl("(?<=a)b", c("ab", "cb"), perl = TRUE))
print(grep("(?<=a)b", c("ab", "cb"), perl = TRUE))

r <- regexpr("(?<=a)b", "ab", perl = TRUE)
print(r)
print(attr(r, "match.length"))

rx <- regexec("(a)(b)", "ab", perl = TRUE)
print(rx[[1]])
print(attr(rx[[1]], "match.length"))

print(sub("(a)(b)", "\\2-\\1", "ab"))
print(gsub("(a)", "[\\1]", "banana"))
print(gsub("(?<=a)b", "B", "ab cb", perl = TRUE))
