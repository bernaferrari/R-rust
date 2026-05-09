if (exists("mclapply", mode = "function")) {
  print(mclapply(integer(0), function(x) x + 1L, mc.cores = 1L))
  print(typeof(mclapply(integer(0), identity, mc.cores = 1L)))
  print(mclapply(1:3, function(x) x + 1L, mc.cores = 1L))
} else {
  print(parallel::mclapply(integer(0), function(x) x + 1L, mc.cores = 1L))
  print(typeof(parallel::mclapply(integer(0), identity, mc.cores = 1L)))
  print(parallel::mclapply(1:3, function(x) x + 1L, mc.cores = 1L))
}
