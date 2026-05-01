print(paste(toupper(c("aB", "mix")), collapse = "|"))
print(paste(tolower(c("Ab", "MIX")), collapse = "|"))
print(paste(chartr("a", "x", c("abc", "banana")), collapse = "|"))

print(is.na(toupper(c("aB", NA, "mix"))))
print(is.na(tolower(c("Ab", NA))))
print(is.na(chartr("a", "x", c("abc", NA))))

print(length(toupper(NULL)))
print(length(chartr("a", "x", character(0))))
print(is.na(toupper(NA)))
print(is.na(chartr("A", "x", NA)))
