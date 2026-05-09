print(suppressWarnings(.Internal(warning(FALSE, FALSE, FALSE, "careful"))))
print(.Internal(gettext("R", "hello")))
print(.Internal(ngettext(1L, "one", "many", "R")))
print(.Internal(ngettext(2L, "one", "many", "R")))
