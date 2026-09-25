function(z, exact = FALSE,
         norm = NULL, method = c("qr", "direct"),
         inv_z = solve(z),
         triangular = FALSE, uplo = "U", ...)
{
    z <- as.matrix(z)
    if(!all(d <- dim(z)))
        return(0)
    nNorm <- is.null(norm)
    if(exact) {
        if(nNorm || norm == "2") {
            s <- svd(z, nu = 0, nv = 0)$d
            if(s[1]) s[1]/s[length(s)] else Inf
        }
        else {
            if(nNorm) norm <- "1"
            norm(z, type=norm) * norm(inv_z, type=norm)
        }
    }
    else if(match.arg(method) == "qr" || d[1] != d[2])
        kappa.qr(qr(if(d[1] < d[2]) t(z) else z),
                 exact = FALSE, norm = norm, ...)
    else if(triangular)
        .kappa_tri(z, exact = FALSE, norm = norm, uplo = uplo, ...)
    else if(is.complex(z))
        1/.Internal(La_zgecon(z, if(nNorm) "1" else norm))
    else
        1/.Internal(La_dgecon(z, if(nNorm) "1" else norm))
}
