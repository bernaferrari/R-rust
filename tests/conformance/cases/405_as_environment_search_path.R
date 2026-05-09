print(identical(as.environment(1), .GlobalEnv))
print(identical(as.environment(length(search())), baseenv()))
print(environmentName(as.environment("package:base")))
print(identical(as.environment(".GlobalEnv"), .GlobalEnv))
