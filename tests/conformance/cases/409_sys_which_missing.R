out <- Sys.which("definitely-not-a-real-command-rport")
print(names(out))
print(unname(out))
print(length(Sys.which(c("definitely-not-a-real-command-rport", "also-not-a-real-command-rport"))))
