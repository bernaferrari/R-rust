li <- l10n_info()
print(paste(names(li), collapse = ","))
print(is.logical(li[[1]]))
print(is.character(li[[4]]))

cs <- Cstack_info()
print(typeof(cs))
print(paste(names(cs), collapse = ","))
print(length(cs))

ev <- extSoftVersion()
print(is.character(ev))
print(paste(names(ev), collapse = ","))
print(length(ev))
