{
C_R_CreateAtVector <- "C_R_CreateAtVector"
C_R_GAxisPars <- "C_R_GAxisPars"

axisTicks <- function(usr, log, axp = NULL, nint = 5) {
    if(is.null(axp))
	axp <- unlist(.axisPars(usr, log=log, nintLog=nint), use.names=FALSE)
    .Call(C_R_CreateAtVector, axp, if(log) 10^usr else usr, nint, log)
}
.axisPars <- function(usr, log = FALSE, nintLog = 5) {
    .Call(C_R_GAxisPars, usr, log, nintLog)
}
axisTicks
}
