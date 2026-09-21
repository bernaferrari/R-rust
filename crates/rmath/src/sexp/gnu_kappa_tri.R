function(z, exact = FALSE, norm = NULL, ...)
{
    if (exact && (is.null(norm) || identical("2", norm))) {
        s <- svd(z, nu = 0, nv = 0)$d
        if (s[1L]) s[1L] / s[length(s)] else Inf
    } else {
        .Primitive("kappa")(z, ...)
    }
}
