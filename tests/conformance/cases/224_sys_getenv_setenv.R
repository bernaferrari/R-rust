Sys.setenv(RPORT_CONF_ENV = "value")
print(Sys.getenv("RPORT_CONF_ENV") == "value")
Sys.unsetenv("RPORT_CONF_ENV")
print(is.na(Sys.getenv("RPORT_CONF_ENV", unset = NA_character_)))
