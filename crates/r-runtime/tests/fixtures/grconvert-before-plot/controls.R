vals <- c(0, .5, 1)
preplot_result <- character()
for (axis in c("x", "y")) {
  for (from in c("device", "ndc", "inches", "lines", "chars", "nic", "nfc", "npc", "user")) {
    for (to in c("device", "ndc", "user")) {
      v <- if (axis == "x") grconvertX(vals, from, to) else grconvertY(vals, from, to)
      preplot_result <- c(preplot_result, paste0(axis, "|", from, "|", to, "|", paste(sprintf("%.5g", v), collapse=",")))
    }
  }
}
