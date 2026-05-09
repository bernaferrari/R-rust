print(path.expand("abc"))
print(startsWith(path.expand("~/abc"), path.expand("~")))
print(endsWith(path.expand("~/abc"), "/abc"))
print(length(path.expand(c("~", "~/abc", "abc"))))
