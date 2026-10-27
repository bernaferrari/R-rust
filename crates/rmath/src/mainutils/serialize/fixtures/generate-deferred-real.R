# GNU R 4.7.0 development r90451, serialization format version 3.
# Serialize before materializing the deferred character vector.
values <- c(0, -0, 1e-9, 1e20, pi, Inf, -Inf, NaN, NA_real_, 1.2345678901234567)
for (spec in list(list(name = 'default', scipen = 0, outdec = '.'),
                  list(name = 'fixed', scipen = 20, outdec = '.'),
                  list(name = 'comma', scipen = 0, outdec = ','),
                  list(name = 'utf8', scipen = 0, outdec = '·'))) {
  options(scipen = spec$scipen, digits = 4, OutDec = spec$outdec)
  value <- as.character(values)
  saveRDS(value, paste0('deferred-real-', spec$name, '.rds'), version = 3, compress = FALSE)
  options(scipen = 0, digits = 3, OutDec = '.')
  cat(spec$name, ': ', paste(value, collapse = '|'), '\n', sep = '')
}
