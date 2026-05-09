print(identical(pos.to.env(1), .GlobalEnv))
print(identical(pos.to.env(length(search())), baseenv()))
print(environmentName(pos.to.env(length(search()))))
