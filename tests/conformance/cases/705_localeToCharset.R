cat(localeToCharset("C"), "\n", sep = "")
cat(localeToCharset("POSIX"), "\n", sep = "")
# The candidate charset list for a UTF-8 locale is platform-dependent
# (this reference returns "UTF-8"; Linux GNU also offers "ISO8859-1").
# The defined contract is that UTF-8 locales map to a charset set
# containing "UTF-8".
cat("UTF-8" %in% localeToCharset("en_US.UTF-8"), "\n", sep = "")
