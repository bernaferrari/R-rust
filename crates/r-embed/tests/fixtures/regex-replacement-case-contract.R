local({
cases <- list(
 list("(a)(Bc)", "aBc", "\\U\\1\\L\\2", TRUE, FALSE),
 list("(a)(Bc)", "aBc", "lit\\U\\1MiXeD\\2", TRUE, FALSE),
 list("(a)(Bc)", "aBc", "\\Uab\\1\\Ecd\\2", TRUE, FALSE),
 list("(a)(Bc)", "aBc", "\\U\\1\\L\\2\\E\\1", TRUE, FALSE),
 list("(a)", "a", "\\U\\9\\1", TRUE, FALSE),
 list("(a)?(b)", "b", "\\U\\1\\2", TRUE, FALSE),
 list("([a-z])", "ab", "\\U\\1:\\L\\1", TRUE, TRUE),
 list("(a)", "a", "\\\\U\\1", TRUE, FALSE),
 list("(a)", "a", "\\u\\1", TRUE, FALSE),
 list("(a)", "a", "x\\", TRUE, FALSE),
 list("(a)", "a", "x\\", FALSE, FALSE),
 list("(a)", "a", "\\U\\1\\E", FALSE, FALSE),
 list("(a)", "a", "\\0", TRUE, FALSE),
 list("\\b", "a b", "\\U\\1", TRUE, TRUE),
 list("\\b", "éa λb", "X", TRUE, TRUE),
 list("\b", "a\bb", "Z", TRUE, TRUE),
 list("(éßᾀ)", "éßᾀ", "\\U\\1", TRUE, FALSE),
 list("(İΣẞ)", "İΣẞ", "\\L\\1", TRUE, FALSE),
 list("(𐐨)", "𐐨", "\\U\\1", TRUE, FALSE)
)

expected <- c("Abc", "litAMiXeDBC", "abAcdBc", "Abca", "A", "B", "A:aB:b", "\\Ua", "ua", "x", "x", "UaE", "0", "a b", "éXaX λXbX", "aZb", "Éßᾈ", "iσß", "𐐀")

for (i in seq_along(cases)) {
    p <- cases[[i]]
    result <- if (p[[5L]]) gsub(p[[1L]], p[[3L]], p[[2L]], perl=p[[4L]]) else sub(p[[1L]], p[[3L]], p[[2L]], perl=p[[4L]])
    stopifnot(identical(result, expected[[i]]))
}
stopifnot(identical(gsub("([a-z])", "\\1\\U", "ab", perl=TRUE), "ab"))
stopifnot(identical(gsub("a", "\\Ufoo", "a", perl=TRUE, useBytes=TRUE), "foo"))
stopifnot(identical(charToRaw(sub("(é)", "\\U\\1", "é", perl=TRUE, useBytes=TRUE)), charToRaw("é")))
stopifnot(identical(charToRaw(gsub("(é)", "[\\U\\1]", "éé", perl=TRUE, useBytes=TRUE)), charToRaw("[é][é]")))
x <- setNames(c("éßᾀ", NA_character_), c("one", "two"))
stopifnot(identical(gsub("(.)", "\\U\\1", x, perl=TRUE), setNames(c("Éßᾈ", NA_character_), names(x))))
stopifnot(identical(charToRaw(sub("(.)", "\\U\\1", rawToChar(as.raw(233)), perl=TRUE, useBytes=TRUE)), as.raw(233)))
stopifnot(identical(sub("a", "x\\", "a", useBytes=TRUE), "x"))
stopifnot(identical(sub("a", "\\1", "a", useBytes=TRUE), ""))
stopifnot(identical(sub("a", "\\U", "a", useBytes=TRUE), "U"))
stopifnot(identical(sub("a", "\\Ufoo", "a", fixed=TRUE, useBytes=TRUE), "\\Ufoo"))
opaque <- rawToChar(as.raw(255))
stopifnot(identical(sub(opaque, "\\Ufoo", opaque, perl=TRUE, useBytes=TRUE), "foo"))
stopifnot(identical(sub("a", "\\Ufoo", "A", perl=TRUE, useBytes=TRUE, ignore.case=TRUE), "foo"))
stopifnot(identical(sub("é", "X", "É", perl=TRUE, useBytes=TRUE, ignore.case=TRUE), "É"))
TRUE
})
