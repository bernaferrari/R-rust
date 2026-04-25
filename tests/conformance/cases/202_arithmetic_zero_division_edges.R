print(c(1 / 0, -1 / 0, 0 / 0))
print(suppressWarnings(c(1 %% 0, -1 %% 0, 1.5 %% 0, 1.5 %% 0.0)))
print(suppressWarnings(c(1L %% 0L, 1L %/% 0L)))
print(suppressWarnings(c(1 %/% 0, -1 %/% 0, 1.5 %/% 0.0)))
