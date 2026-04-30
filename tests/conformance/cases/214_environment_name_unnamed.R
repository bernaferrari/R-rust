e <- new.env(parent = emptyenv())
print(environmentName(e))
print(environmentName(parent.env(e)))
