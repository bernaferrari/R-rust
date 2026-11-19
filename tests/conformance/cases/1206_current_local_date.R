clock_check <- function(tz = "") {
    before <- Sys.time()
    date <- format(Sys.Date())
    parsed <- format(strptime("17:35:14", "%H:%M:%S"), "%Y-%m-%d")
    after <- Sys.time()
    stopifnot(all(as.numeric(c(before, after)) > 946684800))
    days <- format(c(before, after), "%Y-%m-%d", tz = tz)
    stopifnot(date %in% days, parsed %in% days)
}

old_tz <- Sys.getenv("TZ", unset = NA_character_)
clock_check()
# The browser provides a read-only environment. Native R sessions also check
# zones on either side of the civil date boundary.
if (isTRUE(Sys.setenv(TZ = "Pacific/Kiritimati"))) {
    clock_check("Pacific/Kiritimati")
    Sys.setenv(TZ = "Etc/GMT+12")
    clock_check("Etc/GMT+12")
    if (is.na(old_tz)) Sys.unsetenv("TZ") else Sys.setenv(TZ = old_tz)
}
cat("Current time and local calendar date agree\n")
