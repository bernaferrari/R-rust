{
    # GNU utils::isS3method (R 4.6.1, utils/R/objects.R). The closure below
    # closes over its helpers so multi-dot recursion stays on this function
    # when utils masks the base binding. Two runtime adjustments:
    # get0(mode="function") ignores mode, and a base primitive whose name is
    # on tools::nonS3methods(NULL) (t.test, seq.int, ...) is not the GNU
    # function — fall through to the registry. A missing .__S3MethodsTable__.
    # is not a method: exists() on NULL searches the caller.
    isS3method <- function(method, f, class, envir = parent.frame()) {
        stopList <- list(
            base = c("all.equal", "all.names", "all.vars", "as.data.frame.vector",
                "format.info", "format.pval", "max.col", "qr.Q", "qr.R", "qr.X",
                "qr.coef", "qr.fitted", "qr.qty", "qr.qy", "qr.resid", "qr.solve",
                "rep.int", "seq.int", "sort.int", "sort.list"),
            AMORE = "sim.MLPnet",
            BSDA = "sign.test",
            BiocGenerics = "rep.int",
            ChemometricsWithR = "lda.loofun",
            ElectoGraph = "plot.wedding.cake",
            FrF2 = "all.2fis.clear.catlg",
            GLDEX = c("hist.su", "pretty.su"),
            Hmisc = c("abs.error.pred", "all.digits", "all.is.numeric", "format.df",
                "format.pval", "t.test.cluster"),
            HyperbolicDist = "log.hist",
            MASS = c("frequency.polygon", "gamma.dispersion", "gamma.shape", "hist.FD",
                "hist.scott"),
            LinearizedSVR = "sigma.est",
            Matrix = c("qr.Q", "qr.R", "qr.coef", "qr.fitted", "qr.qty", "qr.qy",
                "qr.resid"),
            PerformanceAnalytics = c("mean.LCL", "mean.UCL", "mean.geometric",
                "mean.stderr"),
            RCurl = "merge.list",
            RNetCDF = c("close.nc", "dim.def.nc", "dim.inq.nc", "dim.rename.nc",
                "open.nc", "print.nc"),
            Rmpfr = c("mpfr.is.0", "mpfr.is.integer"),
            SMPracticals = "exp.gibbs",
            SparseM = c("as.matrix.csc", "as.matrix.csr", "as.matrix.ssc",
                "as.matrix.ssr", "as.matrix.coo", "is.matrix.csc", "is.matrix.csr",
                "is.matrix.ssc", "is.matrix.ssr", "is.matrix.coo"),
            TANOVA = "sigma.hat",
            TeachingDemos = "sigma.test",
            XML = "text.SAX",
            ape = "sort.index",
            arm = "sigma.hat",
            assist = "chol.new",
            boot = "exp.tilt",
            car = "scatterplot.matrix",
            calibrator = "t.fun",
            clusterfly = "ggobi.som",
            coda = "as.mcmc.list",
            crossdes = "all.combn",
            ctv = "update.views",
            deSolve = "plot.1D",
            effects = "all.effects",
            elliptic = "sigma.laurent",
            equivalence = "sign.boot",
            fields = c("qr.q2ty", "qr.yq2"),
            gbm = c("pretty.gbm.tree", "quantile.rug"),
            genetics = "diseq.ci",
            gpclib = "scale.poly",
            grDevices = "boxplot.stats",
            graphics = c("close.screen", "plot.design", "plot.new", "plot.window",
                "plot.xy", "split.screen"),
            ic.infer = "all.R2",
            hier.part = "all.regs",
            lasso2 = "qr.rtr.inv",
            latticeExtra = "xyplot.list",
            locfit = c("density.lf", "plot.eval"),
            moments = c("all.cumulants", "all.moments"),
            mosaic = "t.test",
            mratios = c("t.test.ration", "t.test.ratio.default", "t.test.ratio.formula"),
            ncdf = c("open.ncdf", "close.ncdf", "dim.create.ncdf", "dim.def.ncdf",
                "dim.inq.ncdf", "dim.same.ncdf"),
            plyr = c("rbind.fill", "rbind.fill.matrix"),
            quadprog = c("solve.QP", "solve.QP.compact"),
            reposTools = "update.packages2",
            reshape = "all.vars.character",
            rgeos = "scale.poly",
            rowr = "cbind.fill",
            sac = "cumsum.test",
            sfsmisc = "cumsum.test",
            sm = "print.graph",
            spatstat = "lengths.psp",
            splusTimeDate = "sort.list",
            splusTimeSeries = "sort.list",
            stats = c("anova.lmlist", "expand.model.frame", "fitted.values",
                "influence.measures", "lag.plot", "qr.influence", "t.test",
                "plot.spec.phase", "plot.spec.coherency"),
            stremo = "sigma.hat",
            supclust = c("sign.change", "sign.flip"),
            tensorA = "chol.tensor",
            utils = c("close.socket", "flush.console", "update.packages"),
            wavelets = "plot.dwt.multiple"
        )
        nonS3 <- function(package) {
            # Index by position. names() / named [[ are not required: a NULL
            # package is the whole tools::nonS3methods list.
            flat <- function() {
                out <- character()
                for (i in seq_along(stopList)) {
                    vals <- stopList[[i]]
                    if (is.character(vals) && length(vals)) out <- c(out, vals)
                }
                out
            }
            if (is.null(package) || !is.character(package) || length(package) != 1L ||
                is.na(package) || !nzchar(package))
                return(flat())
            if (startsWith(package, "package:"))
                package <- substring(package, 9L)
            thisPkg <- tryCatch(stopList[[package]], error = function(err) NULL)
            if (!is.character(thisPkg) || !length(thisPkg)) {
                nms <- names(stopList)
                if (is.character(nms) && length(nms) == length(stopList)) {
                    hit <- which(nms == package)
                    if (length(hit)) thisPkg <- stopList[[hit[[1L]]]]
                }
            }
            if (is.character(thisPkg) && length(thisPkg)) thisPkg else character()
        }
        in_tab <- function(name, tab) {
            is.character(tab) && length(tab) > 0L && isTRUE(any(name == tab))
        }
        # Kept outside the big named list. A visible base primitive whose GNU
        # home is stats (t.test) must hit this even if that list element is
        # not recovered by name.
        stats_non_s3 <- c("anova.lmlist", "expand.model.frame", "fitted.values",
            "influence.measures", "lag.plot", "qr.influence", "t.test",
            "plot.spec.phase", "plot.spec.coherency")
        on_stop_list <- function(name, package) {
            if (!is.character(name) || length(name) != 1L || is.na(name))
                return(FALSE)
            if (in_tab(name, stats_non_s3) &&
                (is.null(package) || !is.character(package) || length(package) != 1L ||
                    is.na(package) || !nzchar(package) || package == "stats" ||
                    package == "package:stats"))
                return(TRUE)
            # NULL package is tools::nonS3methods(NULL): every package vector,
            # compared one at a time. One giant c() makes any() miss later names.
            if (is.null(package) || !is.character(package) || length(package) != 1L ||
                is.na(package) || !nzchar(package)) {
                if (in_tab(name, stopList[["base"]])) return(TRUE)
                if (in_tab(name, stopList[["stats"]])) return(TRUE)
                if (in_tab(name, stopList[["graphics"]])) return(TRUE)
                if (in_tab(name, stopList[["utils"]])) return(TRUE)
                if (in_tab(name, stopList[["grDevices"]])) return(TRUE)
                n <- length(stopList)
                if (is.numeric(n) && length(n) == 1L && !is.na(n)) {
                    i <- 1
                    while (i <= n) {
                        if (in_tab(name, stopList[[i]])) return(TRUE)
                        i <- i + 1
                    }
                }
                return(FALSE)
            }
            if (startsWith(package, "package:"))
                package <- substring(package, 9L)
            if (package == "base") return(in_tab(name, stopList[["base"]]))
            if (package == "stats") return(in_tab(name, stopList[["stats"]]))
            if (package == "graphics") return(in_tab(name, stopList[["graphics"]]))
            tab <- nonS3(package)
            in_tab(name, tab)
        }
        getfun <- function(name, envir) {
            if (!is.character(name) || length(name) != 1L || is.na(name) || !nzchar(name))
                return(NULL)
            seen_base <- FALSE
            guard <- 0L
            env <- envir
            while (is.environment(env) && guard < 100L) {
                guard <- guard + 1L
                if (identical(env, baseenv()) || identical(env, .BaseNamespaceEnv))
                    seen_base <- TRUE
                have <- tryCatch(exists(name, envir = env, inherits = FALSE),
                    error = function(err) FALSE)
                if (isTRUE(have)) {
                    found <- tryCatch(get(name, envir = env, inherits = FALSE),
                        error = function(err) NULL)
                    if (identical(typeof(found), "promise"))
                        found <- tryCatch(eval(found), error = function(err) NULL)
                    if (is.function(found)) return(found)
                }
                if (identical(env, emptyenv())) break
                parent <- tryCatch(parent.env(env), error = function(err) NULL)
                if (!is.environment(parent) || identical(parent, env)) break
                env <- parent
            }
            if (!seen_base) return(NULL)
            # get(mode="function") still sees FunTab primitives. get0 ignores mode.
            found <- tryCatch(get(name, mode = "function", envir = baseenv()),
                error = function(err) NULL)
            if (is.function(found)) found else NULL
        }
        # as.character() on a closure, builtin, or CHARSXP throws
        # "cannot coerce type to vector of type". Only symbols and
        # character vectors are safe inputs.
        scalar_chr <- function(x) {
            if (is.character(x)) {
                if (length(x) >= 1L && !is.na(x[[1L]])) x[[1L]] else ""
            } else if (is.name(x)) {
                y <- tryCatch(as.character(x), error = function(err) "")
                if (is.character(y) && length(y) >= 1L && !is.na(y[[1L]])) y[[1L]] else ""
            } else ""
        }
        is_ume <- function(e) {
            if (!is.call(e)) return("")
            op <- scalar_chr(e[[1L]])
            if (!nzchar(op)) return("")
            if (op == "UseMethod") {
                if (length(e) < 2L) return("")
                scalar_chr(e[[2L]])
            } else if (op == "{") {
                n <- length(e)
                if (n < 2L) return("")
                for (i in 2:n) {
                    res <- is_ume(e[[i]])
                    if (nzchar(res)) return(res)
                }
                ""
            } else if (op == "if") {
                if (length(e) < 3L) return("")
                res <- is_ume(e[[3L]])
                if (nzchar(res) || length(e) < 4L) res else is_ume(e[[4L]])
            } else ""
        }
        extract_use_method <- function(txt) {
            if (!is.character(txt) || !length(txt) || is.na(txt[[1L]])) return("")
            txt <- txt[[1L]]
            for (q in c("\"", "'")) {
                parts <- strsplit(txt, paste0("UseMethod(", q), fixed = TRUE)[[1L]]
                if (length(parts) >= 2L) {
                    stopq <- strsplit(parts[[2L]], q, fixed = TRUE)[[1L]]
                    if (length(stopq) >= 1L && nzchar(stopq[[1L]])) return(stopq[[1L]])
                }
            }
            ""
        }
        ume_from_fun <- function(fun) {
            # Do not eval the body. is_ume(body(fun)) on UseMethod("mean")
            # dispatches mean.default(body(fun)) in this runtime.
            txt <- tryCatch(paste(deparse(fun), collapse = "\n"),
                error = function(err) "")
            hit <- extract_use_method(txt)
            if (nzchar(hit)) return(hit)
            txt <- tryCatch(paste(deparse(body(fun)), collapse = "\n"),
                error = function(err) "")
            extract_use_method(txt)
        }
        is_s4_generic <- function(fun) {
            if (!isTRUE(.isMethodsDispatchOn()) || !isS4(fun)) return(FALSE)
            if (!isNamespaceLoaded("methods")) return(FALSE)
            ns <- tryCatch(asNamespace("methods"), error = function(e) NULL)
            if (!is.environment(ns) || !exists("is", envir = ns, inherits = FALSE))
                return(FALSE)
            is_fun <- tryCatch(get("is", envir = ns, inherits = FALSE),
                error = function(e) NULL)
            if (identical(typeof(is_fun), "promise"))
                is_fun <- tryCatch(eval(is_fun), error = function(e) NULL)
            if (!is.function(is_fun)) return(FALSE)
            isTRUE(tryCatch(is_fun(fun, "genericFunction"), error = function(e) FALSE))
        }
        s4_derived_default <- function(fun) {
            ns <- asNamespace("methods")
            if (!exists("getMethodsForDispatch", envir = ns, inherits = FALSE))
                stop("no getMethodsForDispatch")
            dispatch <- get("getMethodsForDispatch", envir = ns, inherits = FALSE)
            if (identical(typeof(dispatch), "promise")) dispatch <- eval(dispatch)
            meths_env <- dispatch(fun)
            if (!is.environment(meths_env)) stop("no methods table")
            meths <- as.list(meths_env, all.names = TRUE)
            nms <- names(meths)
            if (is.null(nms)) stop("no method names")
            keep <- grep("^ANY\\b", nms)
            if (!length(keep)) return(NULL)
            r <- meths[keep]
            is_fun <- get("is", envir = ns, inherits = FALSE)
            if (identical(typeof(is_fun), "promise")) is_fun <- eval(is_fun)
            picked <- NULL
            for (i in seq_along(r)) {
                hit <- tryCatch(isTRUE(is_fun(r[[i]], "derivedDefaultMethod")),
                    error = function(e) FALSE)
                if (isTRUE(hit)) {
                    picked <- r[[i]]
                    break
                }
            }
            if (is.null(picked)) return(NULL)
            if (isS4(picked)) {
                data <- tryCatch(picked@.Data, error = function(e) NULL)
                if (is.function(data)) return(data)
            }
            if (is.function(picked)) picked else NULL
        }
        find_generic <- function(fname, envir) {
            fun <- tryCatch(getfun(fname, envir), error = function(err) NULL)
            if (!is.function(fun) || is.primitive(fun)) return("")
            res <- ""
            if (isTRUE(tryCatch(is_s4_generic(fun), error = function(err) FALSE))) {
                derived <- tryCatch(s4_derived_default(fun), error = function(err) NULL)
                if (is.function(derived) && !is.primitive(derived))
                    res <- tryCatch(ume_from_fun(derived), error = function(err) "")
            }
            if (!is.character(res) || length(res) != 1L || is.na(res) || !nzchar(res))
                res <- tryCatch(ume_from_fun(fun), error = function(err) "")
            if (!is.character(res) || length(res) != 1L || is.na(res)) "" else res
        }
        defenv_for <- function(genfun) {
            if (identical(typeof(genfun), "closure")) topenv(environment(genfun))
            else .BaseNamespaceEnv
        }
        internal <- c(.internalGenerics, "[", "[[", "$", "[<-", "[[<-", "$<-", "@", "@<-",
            .S3PrimitiveGenerics, "abs", "sign", "sqrt", "floor", "ceiling", "trunc",
            "round", "signif", "exp", "log", "expm1", "log1p", "cos", "sin", "tan",
            "cospi", "sinpi", "tanpi", "acos", "asin", "atan", "cosh", "sinh", "tanh",
            "acosh", "asinh", "atanh", "lgamma", "gamma", "digamma", "trigamma",
            "cumsum", "cumprod", "cummax", "cummin", "+", "-", "*", "/", "^", "%%",
            "%/%", "&", "|", "!", "==", "!=", "<", "<=", ">=", ">", "all", "any",
            "sum", "prod", "max", "min", "range", "Arg", "Conj", "Im", "Mod", "Re",
            "%*%")
        known <- c(names(.knownS3Generics), internal)
        check <- function(method, f, class, envir) {
            if (missing(method)) {
                method <- paste(f, class, sep = ".")
            } else {
                f.c <- strsplit(method, ".", fixed = TRUE)[[1L]]
                nfc <- length(f.c)
                if (nfc < 2L || !is.character(f.c) || f.c[[1L]] == "")
                    return(FALSE)
                if (nfc == 2L) {
                    f <- f.c[[1L]]
                    class <- f.c[[2L]]
                } else {
                    for (j in 2:nfc)
                        if (check(f = paste(f.c[1:(j - 1L)], collapse = "."),
                            class = paste(f.c[j:nfc], collapse = "."),
                            envir = envir))
                            return(TRUE)
                    return(FALSE)
                }
            }
            if (!is.character(f) || length(f) != 1L || is.na(f) || !nzchar(f))
                return(FALSE)
            if (!any(f == known)) {
                f <- find_generic(f, envir)
                if (!is.character(f) || length(f) != 1L || is.na(f) || !nzchar(f))
                    return(FALSE)
            }
            m <- tryCatch(getfun(method, envir), error = function(err) NULL)
            if (is.function(m)) {
                # environment() on a builtin goes through getAttrib and can
                # throw "cannot coerce type to vector of type". GNU
                # environment(primitive) is NULL and the package is "base".
                kind <- tryCatch(typeof(m), error = function(err) "")
                if (is.primitive(m) || identical(kind, "builtin") || identical(kind, "special")) {
                    # Visible primitive on the stop list is not an S3 method.
                    # Do not consult the registry: a base primitive named
                    # t.test / seq.int would otherwise be found there.
                    # An environment that cannot see the primitive falls
                    # through, so a registered method can still be TRUE.
                    if (on_stop_list(method, NULL) || on_stop_list(method, "base"))
                        return(FALSE)
                    return(TRUE)
                } else {
                    em <- environment(m)
                    pkg <- NULL
                    if (is.environment(em) && isNamespace(em)) {
                        pkg <- environmentName(em)
                        if (!is.character(pkg) || length(pkg) != 1L || is.na(pkg))
                            pkg <- NULL
                    }
                    return(!on_stop_list(method, pkg))
                }
            }
            defenv <- NULL
            w <- .knownS3Generics[f]
            if (is.character(w) && length(w) >= 1L && !is.na(w[[1L]]) && nzchar(w[[1L]]))
                defenv <- tryCatch(asNamespace(w[[1L]]), error = function(e) NULL)
            if (!is.environment(defenv) && isTRUE(any(f == internal)))
                defenv <- .BaseNamespaceEnv
            if (!is.environment(defenv)) {
                genfun <- tryCatch(getfun(f, envir), error = function(err) NULL)
                if (!is.function(genfun)) return(FALSE)
                if (is_s4_generic(genfun)) {
                    picked <- tryCatch({
                        ns <- asNamespace("methods")
                        if (!exists("selectMethod", envir = ns, inherits = FALSE))
                            stop("no selectMethod")
                        sel <- get("selectMethod", envir = ns, inherits = FALSE)
                        if (identical(typeof(sel), "promise")) sel <- eval(sel)
                        sel(genfun, "ANY")
                    }, error = function(e) NULL)
                    if (is.function(picked)) genfun <- picked
                }
                defenv <- tryCatch(defenv_for(genfun), error = function(err) NULL)
            }
            if (!is.environment(defenv)) return(FALSE)
            S3Table <- defenv[[".__S3MethodsTable__."]]
            if (!is.environment(S3Table)) return(FALSE)
            exists(method, envir = S3Table, inherits = FALSE)
        }
        ans <- if (missing(method)) check(f = f, class = class, envir = envir)
            else check(method = method, envir = envir)
        if (!is.logical(ans) || length(ans) != 1L) FALSE
        else if (is.na(ans)) FALSE
        else ans
    }
    isS3method
}
