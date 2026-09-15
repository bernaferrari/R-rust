cat(is.null(interaction.plot(gl(2, 3), gl(3, 2), 1:6)), "\n", sep = "")
cat(is.null(lag.plot(1:5)), "\n", sep = "")
cat(tryCatch(eff.aovlist(1), error = function(e) conditionMessage(e)), "\n", sep = "")
