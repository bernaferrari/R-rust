//! init bindings_misc bindings — extracted verbatim from init.rs; binding order preserved.

use super::*;

pub(crate) unsafe fn install(base_env: SEXP) {
    unsafe {
        // GNU sample.R: closures over .Internal(sample)/sample2, not primitives.
        // setMethod("sample", ...) needs a function skeleton (rport-2gpp.2).
        eval_base_binding(
            base_env,
            "sample",
            "function(x, size, replace = FALSE, prob = NULL) {\n\
             if (length(x) == 1L && is.numeric(x) && is.finite(x) && x >= 1) {\n\
                 if (missing(size)) size <- x\n\
                 sample.int(x, size, replace, prob)\n\
             } else {\n\
                 if (missing(size)) size <- length(x)\n\
                 x[sample.int(length(x), size, replace, prob)]\n\
             }\n\
             }",
        );
        eval_base_binding(
            base_env,
            "sample.int",
            "function(n, size = n, replace = FALSE, prob = NULL,\n\
                      useHash = (n > 1e7 && !replace && is.null(prob) && size <= n/2)) {\n\
             stopifnot(length(n) == 1L)\n\
             if (useHash) {\n\
                 stopifnot(is.null(prob), !replace)\n\
                 .Internal(sample2(n, size))\n\
             } else .Internal(sample(n, size, replace, prob))\n\
             }",
        );
        // GNU rev.R: UseMethod so S4 `[` / length methods run (reg-S4.R mapply).
        eval_base_binding(base_env, "rev", "function(x) UseMethod(\"rev\")");
        eval_base_binding(
            base_env,
            "rev.default",
            "function(x) if (length(x)) x[length(x):1L] else x",
        );
        // GNU New-Internal.R: closure over .Internal(is.unsorted), not a primitive.
        // setMethod("is.unsorted", "A", ...) needs that skeleton (reg-S4.R 616).
        eval_base_binding(
            base_env,
            "is.unsorted",
            "function(x, na.rm = FALSE, strictly = FALSE) {\n\
             if (length(x) <= 1L) return(FALSE)\n\
             if (!na.rm && anyNA(x)) return(NA)\n\
             if (na.rm && any(ii <- is.na(x))) x <- x[!ii]\n\
             .Internal(is.unsorted(x, na.rm, strictly))\n\
             }",
        );
        // GNU funprog.R: identity <- function(x) x. setMethod(..., identity)
        // needs a closure, not a primitive (reg-S4.R PR#15691).
        eval_base_binding(base_env, "identity", "function(x) x");
        eval_base_binding(
            base_env,
            "debug",
            "function(fun, text = \"\", condition = NULL, signature = NULL) {\n\
             if(is.null(signature)) .Internal(debug(fun, text, condition))\n\
             else if(requireNamespace(\"methods\")) methods:::.debugMethod(fun, text, condition, signature, once = FALSE)\n\
             else stop(\"failed to load the methods package for debugging by signature\")\n\
             }",
        );
        eval_base_binding(
            base_env,
            "debugonce",
            "function(fun, text = \"\", condition = NULL, signature = NULL) {\n\
             if(is.null(signature)) .Internal(debugonce(fun, text, condition))\n\
             else if(requireNamespace(\"methods\")) methods:::.debugMethod(fun, text, condition, signature, once = TRUE)\n\
             else stop(\"failed to load the methods package for debugging by signature\")\n\
             }",
        );
        eval_base_binding(
            base_env,
            "undebug",
            "function(fun, signature = NULL) {\n\
             if(is.null(signature)) .Internal(undebug(fun))\n\
             else if(requireNamespace(\"methods\")) methods:::.undebugMethod(fun, signature = signature)\n\
             else stop(\"failed to load methods package for undebugging by signature\")\n\
             }",
        );
        eval_base_binding(
            base_env,
            "isdebugged",
            "function(fun, signature = NULL) {\n\
             if(is.null(signature)) .Internal(isdebugged(fun))\n\
             else if(requireNamespace(\"methods\")) methods:::.isMethodDebugged(fun, signature)\n\
             else stop(\"failed to load methods package for handling signature\")\n\
             }",
        );
        unsafe {
            let sym = crate::sexp::symbol::Rf_install(c"identity".as_ptr());
            let fun = crate::sexp::envir::R_findVarInFrame(base_env, sym);
            let _ = crate::eval::bc_compile::compile_closure(fun);
        }
        eval_base_binding(
            base_env,
            "is.na<-",
            "function(x, value) UseMethod(\"is.na<-\")",
        );
        eval_base_binding(
            base_env,
            "is.na<-.default",
            "function(x, value) { x[value] <- NA; x }",
        );
        // GNU table.R: is.table <- function(x) inherits(x, "table")
        eval_base_binding(base_env, "is.table", "function(x) inherits(x, \"table\")");
        eval_base_binding(base_env, "as.table", include_str!("../gnu_as_table.R"));
        eval_base_binding(base_env, "table", include_str!("../gnu_table.R"));
        eval_base_binding(
            base_env,
            "print.table",
            include_str!("../gnu_print_table.R"),
        );
        eval_base_binding(
            base_env,
            "conformToProto",
            include_str!("../gnu_conform_to_proto.R"),
        );
        eval_base_binding(base_env, "strcapture", include_str!("../gnu_strcapture.R"));
        eval_base_binding(base_env, "xtabs", include_str!("../gnu_xtabs.R"));
        eval_base_binding(
            base_env,
            "as.table.default",
            include_str!("../gnu_as_table_default.R"),
        );
        eval_base_binding(base_env, "as.array", include_str!("../gnu_as_array.R"));
        eval_base_binding(
            base_env,
            "as.array.default",
            include_str!("../gnu_as_array_default.R"),
        );
        eval_base_binding(
            base_env,
            "provideDimnames",
            include_str!("../gnu_provide_dimnames.R"),
        );
        eval_base_binding(base_env, "kmeans", include_str!("../gnu_kmeans.R"));

        // GNU utils/R/sourceutils.R: getSrcref for rematched S4 methods
        // (reg-S4.R 638). Lives in utils; install in base so source() tests
        // see it without attaching utils.
        eval_base_binding(
            base_env,
            "getSrcref",
            "function(x) {\n\
             if (inherits(x, \"srcref\")) x\n\
             else if (!is.null(srcref <- attr(x, \"srcref\")) ||\n\
                      is.function(x) && !is.null(srcref <- getSrcref(body(x))))\n\
                 srcref\n\
             else if (methods::is(x, \"MethodDefinition\"))\n\
                 getSrcref(unclass(methods::unRematchDefinition(x)))\n\
             }",
        );
        // GNU base/R/srcfile.R: print/as.character of parse srcrefs
        // (reg-S4.R getSrcref of rematched methods).
        eval_base_binding(
            base_env,
            "as.character.srcref",
            "function(x, useSource = TRUE, to = x, ...) {\n\
             srcfile <- attr(x, \"srcfile\")\n\
             lines <- if (!is.null(srcfile)) srcfile$lines\n\
             if (!isTRUE(useSource) || is.null(lines) || !length(lines)) {\n\
               fn <- if (is.null(srcfile)) \"\" else srcfile$filename\n\
               return(paste0(\"<srcref: file \\\"\", fn, \"\\\">\"))\n\
             }\n\
             first <- as.integer(x[1L]); last <- as.integer(x[3L])\n\
             last <- min(last, length(lines))\n\
             if (is.na(first) || is.na(last) || first < 1L || first > last)\n\
               return(character())\n\
             out <- lines[first:last]\n\
             if (length(out)) {\n\
               out[length(out)] <- substring(out[length(out)], 1L, as.integer(x[4L]))\n\
               out[1L] <- substring(out[1L], as.integer(x[2L]))\n\
             }\n\
             out\n\
             }",
        );
        eval_base_binding(
            base_env,
            "print.srcref",
            "function(x, useSource = TRUE, ...) {\n\
             cat(as.character.srcref(x, useSource = useSource), sep = \"\\n\")\n\
             invisible(x)\n\
             }",
        );

        // GNU print.R: print is UseMethod, not a primitive. setMethod("print")
        // uses the closure as the generic skeleton (rport-2gpp.2.3).
        eval_base_binding(base_env, "print", "function(x, ...) UseMethod(\"print\")");
        eval_base_binding(
            base_env,
            "summary",
            "function(object, ...) UseMethod(\"summary\")",
        );
        eval_base_binding(
            base_env,
            "summary.factor",
            "function(object, maxsum = 100L, ...) {\n\
             nas <- is.na(object)\n\
             ll <- levels(object)\n\
             if (ana <- any(nas)) maxsum <- maxsum - 1L\n\
             tbl <- table(object)\n\
             tt <- c(tbl)\n\
             names(tt) <- dimnames(tbl)[[1L]]\n\
             if (length(ll) > maxsum) {\n\
                 drop <- maxsum:length(ll)\n\
                 o <- sort.list(tt, decreasing = TRUE)\n\
                 tt <- c(tt[o[-drop]], `(Other)` = sum(tt[o[drop]]))\n\
             }\n\
             if (ana) c(tt, NAs = sum(nas)) else tt\n\
             }",
        );
        // GNU stats/R/AIC.R: AIC is UseMethod so AIC.pfit S3 methods run
        // (reg-S4.R 334-343). The former builtin is AIC.default.
        eval_base_binding(
            base_env,
            "AIC",
            "function(object, ..., k = 2) UseMethod(\"AIC\")",
        );
        eval_base_binding(
            base_env,
            "AIC.logLik",
            "function(object, ..., k = 2) -2 * as.numeric(object) + k * attr(object, \"df\")",
        );
        // GNU sort.R: xtfrm is a primitive internal generic; the default
        // method is this closure (unclass numeric, else rank).
        eval_base_binding(
            base_env,
            "xtfrm.default",
            "function(x) {\n\
             y <- if (is.numeric(x)) unclass(x) else as.vector(rank(x, ties.method = \"min\", na.last = \"keep\"))\n\
             if (!is.numeric(y) || ((length(y) != length(x)) && !inherits(x, \"data.frame\")))\n\
                 stop(\"cannot xtfrm 'x'\")\n\
             y\n\
             }",
        );

        // GNU sort.R: order is a closure over .Internal(order), not a
        // primitive. methods::setGeneric needs formals (rport-txofy).
        eval_base_binding(
            base_env,
            "order",
            "function(..., na.last = TRUE, decreasing = FALSE,\n\
             method = c(\"auto\", \"shell\", \"radix\")) {\n\
             z <- list(...)\n\
             if (length(z) == 0L) return(integer())\n\
             method <- match.arg(method)\n\
             if (any(vapply(z, is.object, logical(1L)))) {\n\
                 z <- lapply(z, function(x) if (is.object(x)) as.vector(xtfrm(x)) else x)\n\
                 return(do.call(\"order\", c(z, list(na.last = na.last, decreasing = decreasing, method = method))))\n\
             }\n\
             .Internal(order(na.last, decreasing, ...))\n\
             }",
        );
        // GNU sort.R / mean.R / stats median.R: closures, not primitives.
        eval_base_binding(
            base_env,
            "sort",
            "function(x, decreasing = FALSE, ...) {\n\
             if (!is.logical(decreasing) || length(decreasing) != 1L)\n\
                 stop(\"'decreasing' must be a length-1 logical vector.\\nDid you intend to set 'partial'?\")\n\
             UseMethod(\"sort\")\n\
             }",
        );
        eval_base_binding(
            base_env,
            "sort.default",
            "function(x, decreasing = FALSE, na.last = NA, ...) {\n\
             if (is.object(x))\n\
                 x[order(x, na.last = na.last, decreasing = decreasing)]\n\
             else .Internal(sort(x, decreasing, na.last, ...))\n\
             }",
        );
        eval_base_binding(
            base_env,
            "sort.int",
            "function(x, partial = NULL, na.last = NA, decreasing = FALSE, ...) {\n\
             decreasing <- as.logical(decreasing)\n\
             if (!is.logical(decreasing) || length(decreasing) != 1L || is.na(decreasing))\n\
                 stop(\"'decreasing' must be a length-1 logical vector.\\nDid you intend to set 'partial'?\")\n\
             .Internal(sort(x, decreasing, na.last, ...))\n\
             }",
        );
        eval_base_binding(
            base_env,
            "getOption",
            "function(x, default = NULL) {\n\
             if (missing(default)) .Internal(getOption(x))\n\
             else {\n\
                 ans <- .Internal(getOption(x))\n\
                 if (is.null(ans)) default else ans\n\
             }\n\
             }",
        );
        eval_base_binding(base_env, "diff.ts", include_str!("../gnu_diff_ts.R"));
        eval_base_binding(base_env, "mean", "function(x, ...) UseMethod(\"mean\")");
        eval_base_binding(
            base_env,
            "mean.default",
            "function(x, trim = 0, na.rm = FALSE, ...) {\n\
             if (!is.numeric(x) && !is.complex(x) && !is.logical(x)) {\n\
                 warning(\"argument is not numeric or logical: returning NA\")\n\
                 return(NA_real_)\n\
             }\n\
             if (isTRUE(na.rm)) x <- x[!is.na(x)]\n\
             if (!is.numeric(trim) || length(trim) != 1L)\n\
                 stop(\"'trim' must be numeric of length one\")\n\
             n <- length(x)\n\
             if (trim > 0 && n) {\n\
                 if (is.complex(x))\n\
                     stop(\"trimmed means are not defined for complex data\")\n\
                 if (anyNA(x)) return(NA_real_)\n\
                 if (trim >= 0.5) return(median(x, na.rm = FALSE))\n\
                 lo <- floor(n * trim) + 1\n\
                 hi <- n + 1 - lo\n\
                 x <- sort(x)[lo:hi]\n\
             }\n\
             .Internal(mean(x))\n\
             }",
        );
        eval_base_binding(
            base_env,
            "median",
            "function(x, na.rm = FALSE, ...) UseMethod(\"median\")",
        );
        eval_base_binding(
            base_env,
            "median.default",
            "function(x, na.rm = FALSE, ...) {\n\
             if (is.factor(x) || is.data.frame(x)) stop(\"need numeric data\")\n\
             if (length(names(x))) names(x) <- NULL\n\
             if (na.rm) x <- x[!is.na(x)] else if (any(is.na(x))) return(x[NA_integer_])\n\
             n <- length(x)\n\
             if (n == 0L) return(x[NA_integer_])\n\
             half <- (n + 1L) %/% 2L\n\
             if (n %% 2L == 1L) sort(x, partial = half)[half]\n\
             else mean(sort(x, partial = half + 0L:1L)[half + 0L:1L])\n\
             }",
        );

        eval_base_binding(
            base_env,
            "print.default",
            "function(x, digits = NULL, quote = TRUE, na.print = NULL,\n\
             print.gap = NULL, right = FALSE, max = NULL, width = NULL,\n\
             useSource = TRUE, ...) {\n\
             args <- pairlist(digits = digits, quote = quote, na.print = na.print,\n\
                 print.gap = print.gap, right = right, max = max, width = width,\n\
                 useSource = useSource, ...)\n\
             missings <- c(missing(digits), missing(quote), missing(na.print),\n\
                 missing(print.gap), missing(right), missing(max),\n\
                 missing(width), missing(useSource))\n\
             .Internal(print.default(x, args, missings))\n\
             }",
        );
        eval_base_binding(
            base_env,
            "print.function",
            include_str!("../gnu_print_function.R"),
        );
        // GNU New-Internal.R: cbind/rbind are closures around .Internal
        // so the first argument is always deparse.level. cbind2 defaults
        // pass -1L to disable S4 redispatch (bind.c tryS4).
        eval_base_binding(
            base_env,
            "cbind",
            "function(..., deparse.level = 1) .Internal(cbind(deparse.level, ...))",
        );
        eval_base_binding(
            base_env,
            "rbind",
            "function(..., deparse.level = 1) .Internal(rbind(deparse.level, ...))",
        );

        eval_base_binding(
            base_env,
            "formals",
            "function(fun = sys.function(sys.parent()), envir = parent.frame()) {\n\
             if (is.character(fun))\n\
                 fun <- get(fun, mode = \"function\", envir = envir)\n\
             .Internal(formals(fun))\n\
             }",
        );
        eval_base_binding(
            base_env,
            "body",
            "function(fun = sys.function(sys.parent())) {\n\
             if (is.character(fun))\n\
                 fun <- get(fun, mode = \"function\", envir = parent.frame())\n\
             .Internal(body(fun))\n\
             }",
        );
        eval_base_binding(base_env, "Sys.info", "function() .Internal(Sys.info())");
        eval_base_binding(base_env, "formalArgs", "function(def) names(formals(def))");

        // GNU formals.R: replacement functions are closures, not primitives.
        eval_base_binding(
            base_env,
            "body<-",
            "function (fun, envir = environment(fun), value) {\n\
             if (!is.function(fun)) warning(\"'fun' is not a function\")\n\
             if (is.expression(value)) {\n\
                 if (length(value) > 1L)\n\
                     warning(\"using the first element of 'value' of type \\\"expression\\\"\")\n\
                 value <- value[[1L]]\n\
             }\n\
             as.function(c(formals(fun),\n\
                 if (is.null(value) || is.atomic(value) || is.list(value)) list(value) else value),\n\
                 envir)\n\
             }",
        );
        eval_base_binding(
            base_env,
            "formals<-",
            "function (fun, envir = environment(fun), value) {\n\
             if (!is.function(fun)) warning(\"'fun' is not a function\")\n\
             bd <- body(fun)\n\
             as.function(c(value,\n\
                 if (is.null(bd) || is.atomic(bd) || is.list(bd)) list(bd) else bd),\n\
                 envir)\n\
             }",
        );

        // GNU apply.R: n-d arrays, empty-extent MARGIN, and FUN=NULL collapse.
        eval_base_binding(base_env, "apply", include_str!("../gnu_apply.R"));
        eval_base_binding(base_env, "Vectorize", include_str!("../gnu_vectorize.R"));
        eval_base_binding(
            base_env,
            "determinant",
            "function(x, logarithm = TRUE, ...) UseMethod(\"determinant\")",
        );
        eval_base_binding(
            base_env,
            "determinant.matrix",
            "function(x, logarithm = TRUE, ...) {\n\
             if ((n <- ncol(x)) != nrow(x))\n\
                 stop(\"'x' must be a square matrix\")\n\
             if (n < 1L)\n\
                 return(structure(list(modulus = structure(if (logarithm) 0 else 1, logarithm = logarithm), sign = 1L), class = \"det\"))\n\
             if (is.complex(x))\n\
                 stop(\"'determinant' not currently defined for complex matrices\")\n\
             det_ge_real(x, logarithm)\n\
             }",
        );
        eval_base_binding(
            base_env,
            "det",
            "function(x, ...) { z <- determinant(x, logarithm = TRUE, ...); c(z$sign * exp(z$modulus)) }",
        );
        eval_base_binding(
            base_env,
            "isSymmetric",
            "function(object, ...) UseMethod(\"isSymmetric\")",
        );
        eval_base_binding(
            base_env,
            "isSymmetric.matrix",
            "function(object, tol = 100 * .Machine$double.eps, tol1 = 8 * tol, trans = \"C\", ...) {\n\
             if (!is.matrix(object)) return(FALSE)\n\
             d <- dim(object)\n\
             if ((n <- d[1L]) != d[2L]) return(FALSE)\n\
             iCplx <- is.complex(object) && trans == \"C\"\n\
             if (n > 1L && length(tol1)) {\n\
                 Cj <- if (iCplx) Conj else identity\n\
                 for (i in unique(c(1L, 2L, n - 1L, n)))\n\
                     if (is.character(all.equal(object[i, ], Cj(object[, i]), tolerance = tol1, ...)))\n\
                         return(FALSE)\n\
             }\n\
             test <- if (iCplx)\n\
                 all.equal.numeric(object, Conj(t(object)), tolerance = tol, ...)\n\
             else\n\
                 all.equal(object, t(object), tolerance = tol, ...)\n\
             isTRUE(test)\n\
             }",
        );

        eval_base_binding(
            base_env,
            "La.svd",
            "function(x, nu = min(n, p), nv = min(n, p)) {\n\
             if (!is.logical(x) && !is.numeric(x) && !is.complex(x))\n\
                 stop(\"argument to 'La.svd' must be numeric or complex\")\n\
             if (any(!is.finite(x))) stop(\"infinite or missing values in 'x'\")\n\
             x <- as.matrix(x)\n\
             n <- nrow(x); p <- ncol(x)\n\
             if (!n || !p) stop(\"a dimension is zero\")\n\
             zero <- if (is.complex(x)) 0+0i else 0\n\
             if (nu || nv) {\n\
                 np <- min(n, p)\n\
                 if (nu <= np && nv <= np) {\n\
                     jobu <- \"S\"; u <- matrix(zero, n, np); vt <- matrix(zero, np, p); nu0 <- nv0 <- np\n\
                 } else {\n\
                     jobu <- \"A\"; u <- matrix(zero, n, n); vt <- matrix(zero, p, p); nu0 <- n; nv0 <- p\n\
                 }\n\
             } else {\n\
                 jobu <- \"N\"; u <- matrix(zero, 1L, 1L); vt <- matrix(zero, 1L, 1L); nu0 <- nv0 <- 0L\n\
             }\n\
             res <- if (is.complex(x)) La_svd_cmplx(jobu, x, double(min(n, p)), u, vt) else La_svd(jobu, x, double(min(n, p)), u, vt)\n\
             res <- res[c(\"d\", if (nu) \"u\", if (nv) \"vt\")]\n\
             if (nu && nu < nu0) res$u <- res$u[, seq_len(min(n, nu)), drop = FALSE]\n\
             if (nv && nv < nv0) res$vt <- res$vt[seq_len(min(p, nv)), drop = FALSE]\n\
             res\n\
             }",
        );
        // GNU kappa.R: public norm/rcond are closures (implicit generics).
        eval_base_binding(
            base_env,
            "norm",
            "function(x, type = c(\"O\", \"I\", \"F\", \"M\", \"2\")) {\n\
             if (identical(\"2\", type)) {\n\
                 if (!length(x)) 0 else if (anyNA(x)) NA_real_ else svd(x, nu = 0L, nv = 0L)$d[1L]\n\
             } else if (is.numeric(x) || is.logical(x))\n\
                 .Internal(La_dlange(x, type))\n\
             else if (is.complex(x))\n\
                 .Internal(La_zlange(x, type))\n\
             else stop(sprintf(\"invalid 'x': type \\\"%s\\\"\", typeof(x)))\n\
             }",
        );
        eval_base_binding(
            base_env,
            "rcond",
            "function(x, norm = c(\"O\", \"I\", \"1\"), triangular = FALSE, uplo = \"U\", ...) {\n\
             norm <- match.arg(norm)\n\
             stopifnot(length(d <- dim(x)) == 2L)\n\
             if (!all(d)) return(1 / 0)\n\
             if (d[1L] != d[2L])\n\
                 return(rcond(qr.R(qr(if (d[1L] < d[2L]) t(x) else x)), triangular = TRUE, uplo = \"U\", norm = norm, ...))\n\
             if (is.complex(x)) {\n\
                 if (triangular) switch(uplo,\n\
                     \"U\" = .Internal(La_ztrcon(x, norm)),\n\
                     \"L\" = .Internal(La_ztrcon3(x, norm, \"L\")),\n\
                     stop(\"'uplo' must be \\\"U\\\" or \\\"L\\\"\"))\n\
                 else .Internal(La_zgecon(x, norm))\n\
             } else {\n\
                 if (triangular) switch(uplo,\n\
                     \"U\" = .Internal(La_dtrcon(x, norm)),\n\
                     \"L\" = .Internal(La_dtrcon3(x, norm, \"L\")),\n\
                     stop(\"'uplo' must be \\\"U\\\" or \\\"L\\\"\"))\n\
                 else .Internal(La_dgecon(x, norm))\n\
             }\n\
             }",
        );
        eval_base_binding(
            base_env,
            ".getNamespace",
            "function(name) .Internal(getRegisteredNamespace(name))",
        );
        eval_base_binding(
            base_env,
            "..getNamespace",
            "function(name, where) {\n\
             .Internal(getRegisteredNamespace(name)) %||% tryCatch(loadNamespace(name), error = function(e) .GlobalEnv)\n\
             }",
        );
        // GNU namespace.R: isBaseNamespace(ns) identical(ns, .BaseNamespaceEnv).
        // methods::show,genericFunction-method calls .minimalName → this.
        eval_base_binding(
            base_env,
            "isBaseNamespace",
            "function(ns) identical(ns, .BaseNamespaceEnv)",
        );
        // GNU namespace.R: isNamespaceLoaded(name) .Internal(isRegisteredNamespace(name))
        eval_base_binding(
            base_env,
            "isNamespaceLoaded",
            "function(name) .Internal(isRegisteredNamespace(name))",
        );

        // GNU namespace.R: methods::.isExported and show() need these.
        eval_base_binding(
            base_env,
            ".getNamespaceInfo",
            "function(ns, which) {\n\
             info <- get(\".__NAMESPACE__.\", envir = ns, inherits = FALSE)\n\
             if (!exists(which, envir = info, inherits = FALSE) && which == \"S3methods\") {\n\
               assign(which, matrix(NA_character_, 0L, 4L), envir = info)\n\
             }\n\
             get(which, envir = info, inherits = FALSE)\n\
             }",
        );
        eval_base_binding(
            base_env,
            "getNamespaceInfo",
            "function(ns, which) {\n\
             ns <- asNamespace(ns)\n\
             .getNamespaceInfo(ns, which)\n\
             }",
        );
        eval_base_binding(
            base_env,
            "getNamespaceExports",
            "function(ns) {\n\
             ns <- asNamespace(ns)\n\
             names(if (isBaseNamespace(ns)) .BaseNamespaceEnv else .getNamespaceInfo(ns, \"exports\"))\n\
             }",
        );
        eval_base_binding(
            base_env,
            "getExportedValue",
            "function(ns, name) .Internal(getNamespaceValue(ns, name, TRUE))",
        );
        eval_base_binding(
            base_env,
            "getNamespaceImports",
            "function(ns) { ns <- asNamespace(ns); if (isBaseNamespace(ns)) NULL else .getNamespaceInfo(ns, 'imports') }",
        );
        eval_base_binding(
            base_env,
            "getNamespaceName",
            "function(ns) {\n\
             ns <- asNamespace(ns)\n\
             if (identical(ns, .BaseNamespaceEnv) || identical(ns, baseenv())) \"base\"\n\
             else unname(getNamespaceInfo(ns, \"spec\")[\"name\"])\n\
             }",
        );
        eval_base_binding(
            base_env,
            ".S3method",
            "function(generic, class, method) {\n\
             if (missing(method)) method <- paste(generic, class, sep = \".\")\n\
             method <- match.fun(method)\n\
             registerS3method(generic, class, method, envir = parent.frame())\n\
             }",
        );

        eval_base_binding(
            base_env,
            ".register_print_data_frame",
            "{ registerS3method(\"print\", \"data.frame\", print.data.frame); TRUE }",
        );
        eval_base_binding(
            base_env,
            ".register_round_posix",
            "{ registerS3method(\"round\", \"Date\", round.Date); registerS3method(\"round\", \"POSIXt\", round.POSIXt); TRUE }",
        );
        eval_base_binding(
            base_env,
            "print.proc_time",
            "function(x, ...) {\n\
             y <- x[1L:3L]\n\
             w <- max(7L, nchar(sprintf(\"%.3f\", y)))\n\
             cat(paste(sprintf(paste0(\"%\", w, \"s\"), c(\"user\", \"system\", \"elapsed\")), collapse = \" \"), \"\\n\", sep = \" \")\n\
             cat(paste(sprintf(paste0(\"%\", w, \".3f\"), y), collapse = \" \"), \"\\n\", sep = \" \")\n\
             invisible(x)\n\
             }",
        );
        eval_base_binding(
            base_env,
            ".register_print_proc_time",
            "{ registerS3method(\"print\", \"proc_time\", print.proc_time); TRUE }",
        );
        eval_base_binding(
            base_env,
            "setNamespaceInfo",
            "function(ns, which, val) {\n\
             ns <- asNamespace(ns)\n\
             info <- get(\".__NAMESPACE__.\", envir = ns, inherits = FALSE)\n\
             assign(which, val, envir = info)\n\
             }",
        );
        eval_base_binding(
            base_env,
            ".rmpkg",
            "function(pkg) sub(\"package:\", \"\", pkg, fixed=TRUE)",
        );
        eval_base_binding(
            base_env,
            "trace",
            "function(what, tracer, exit, at, print, signature,\n\
             where = topenv(parent.frame()), edit = FALSE)\n\
             {\n\
             if(nargs() > 1L && !.isMethodsDispatchOn()) {\n\
             ns <- try(loadNamespace(\"methods\"))\n\
             if(isNamespace(ns))\n\
             message(\"(loaded the methods namespace)\", domain = NA)\n\
             else\n\
             stop(\"tracing functions requires the 'methods' package, but unable to load the 'methods' namespace\")\n\
             }\n\
             else if(nargs() == 1L)\n\
             return(.primTrace(what))\n\
             tState <- tracingState(FALSE)\n\
             on.exit(tracingState(tState))\n\
             call <- sys.call()\n\
             call[[1L]] <- quote(methods:::.TraceWithMethods)\n\
             call$where <- where\n\
             eval.parent(call)\n\
             }",
        );
        eval_base_binding(
            base_env,
            "untrace",
            "function(what, signature = NULL, where = topenv(parent.frame())) {\n\
             if(!.isMethodsDispatchOn())\n\
             return(.primUntrace(what))\n\
             tState <- tracingState(FALSE)\n\
             on.exit(tracingState(tState))\n\
             call <- sys.call()\n\
             call[[1L]] <- quote(methods:::.TraceWithMethods)\n\
             call$where <- where\n\
             call$untrace <- TRUE\n\
             invisible(eval.parent(call))\n\
             }",
        );
        eval_base_binding(
            base_env,
            ".doTrace",
            "function(expr, msg) {\n\
             on <- tracingState(FALSE)\n\
             if(on) {\n\
             on.exit(tracingState(TRUE))\n\
             if(!missing(msg)) {\n\
             call <- deparse(sys.call(sys.parent(1L)))\n\
             if(length(call) > 1L)\n\
             call <- paste(call[[1]], \"....\")\n\
             cat(\"Tracing\", call, msg, \"\\n\")\n\
             }\n\
             exprObj <- substitute(expr)\n\
             eval.parent(exprObj)\n\
             }\n\
             NULL\n\
             }",
        );

        eval_base_binding(
            base_env,
            "matrix",
            "function(data = NA, nrow = NULL, ncol = NULL, byrow = FALSE, dimnames = NULL) {\n\
             if (is.object(data) || !is.atomic(data)) data <- as.vector(data)\n\
             matrix_impl(data, nrow, ncol, byrow, dimnames)\n\
             }",
        );

        // `identical` is an ordinary base closure in GNU R, not the internal
        // primitive itself. Keeping that wrapper matters for argument
        // matching: an explicitly empty actual such as `identical(x, y,)`
        // selects `num.eq = TRUE` before `.Internal(identical(...))` eagerly
        // evaluates its complete argument list.
        let identical_formals = formals_from_specs(&[
            arg("x"),
            arg("y"),
            arg_default("num.eq", FormalDefault::True),
            arg_default("single.NA", FormalDefault::True),
            arg_default("attrib.as.set", FormalDefault::True),
            arg_default("ignore.bytecode", FormalDefault::True),
            arg_default("ignore.environment", FormalDefault::False),
            arg_default("ignore.srcref", FormalDefault::True),
            arg_default("extptr.as.ref", FormalDefault::False),
        ]);
        let _identical_formals_guard = super::super::protect::protect(identical_formals);

        // Build `identical(x, y, num.eq, ..., extptr.as.ref)` as a language
        // object, then wrap it in `.Internal(...)`. The internal lookup uses
        // R_FunTab, so replacing the public base binding with this closure
        // does not hide the primitive implementation.
        let internal_call = Rf_allocList(10);
        let _internal_call_guard = super::super::protect::protect(internal_call);
        crate::sexp::accessors::SET_TYPEOF(internal_call, SEXPTYPE::LANGSXP.as_c_int());
        let mut cell = internal_call;
        for name in [
            "identical",
            "x",
            "y",
            "num.eq",
            "single.NA",
            "attrib.as.set",
            "ignore.bytecode",
            "ignore.environment",
            "ignore.srcref",
            "extptr.as.ref",
        ] {
            SETCAR(cell, Rf_install_in_current(name));
            cell = CDR(cell);
        }
        let identical_body = Rf_lang2(Rf_install_in_current(".Internal"), internal_call);
        let _identical_body_guard = super::super::protect::protect(identical_body);
        let identical_closure =
            crate::mainutils::dstruct::mkCLOSXP(identical_formals, identical_body, base_env);
        let _identical_closure_guard = super::super::protect::protect(identical_closure);

        defineVar(
            Rf_install_in_current("identical"),
            identical_closure,
            base_env,
        );

        // `I` <- function(x) {
        //     class(x) <- unique(c("AsIs", oldClass(x)))
        //     x
        // }
        //
        // GNU R defines this wrapper in base's dataframe.R.  It must prepend
        // rather than replace an existing explicit class, and `unique` keeps
        // repeated I() calls idempotent.  Keep it as a closure so assigning
        // the class follows the evaluator's normal copy-on-modify path.
        let as_is_formals = formals_from_specs(&[arg("x")]);
        let _as_is_formals_guard = super::super::protect::protect(as_is_formals);
        let x = Rf_install_in_current("x");

        let as_is = Rf_mkString(c"AsIs".as_ptr());
        let _as_is_guard = super::super::protect::protect(as_is);
        let old_class = Rf_lang2(Rf_install_in_current("oldClass"), x);
        let _old_class_guard = super::super::protect::protect(old_class);
        let classes = Rf_lang3(Rf_install_in_current("c"), as_is, old_class);
        let _classes_guard = super::super::protect::protect(classes);
        let unique_classes = Rf_lang2(Rf_install_in_current("unique"), classes);
        let _unique_classes_guard = super::super::protect::protect(unique_classes);
        let class_lhs = Rf_lang2(Rf_install_in_current("class"), x);
        let _class_lhs_guard = super::super::protect::protect(class_lhs);
        let class_assignment = Rf_lang3(Rf_install_in_current("<-"), class_lhs, unique_classes);
        let _class_assignment_guard = super::super::protect::protect(class_assignment);
        let as_is_body = Rf_lang3(Rf_install_in_current("{"), class_assignment, x);
        let _as_is_body_guard = super::super::protect::protect(as_is_body);
        let as_is_closure =
            crate::mainutils::dstruct::mkCLOSXP(as_is_formals, as_is_body, base_env);
        let _as_is_closure_guard = super::super::protect::protect(as_is_closure);

        defineVar(Rf_install_in_current("I"), as_is_closure, base_env);

        // `chooseOpsMethod` is the base S3 generic consulted by
        // DispatchGroup when two distinct Ops methods are found.  The full
        // base package supplies this closure; install its small generic
        // definition here so embedded sessions get the same hook.
        let choose_formals = formals_from_specs(&[
            arg("x"),
            arg("y"),
            arg("mx"),
            arg("my"),
            arg("cl"),
            arg("reverse"),
        ]);
        let _choose_formals_guard = super::super::protect::protect(choose_formals);
        let choose_generic = Rf_mkString(c"chooseOpsMethod".as_ptr());
        let _choose_generic_guard = super::super::protect::protect(choose_generic);
        let choose_body = Rf_lang2(Rf_install_in_current("UseMethod"), choose_generic);
        let _choose_body_guard = super::super::protect::protect(choose_body);
        let choose_closure =
            crate::mainutils::dstruct::mkCLOSXP(choose_formals, choose_body, base_env);
        let _choose_closure_guard = super::super::protect::protect(choose_closure);
        defineVar(
            Rf_install_in_current("chooseOpsMethod"),
            choose_closure,
            base_env,
        );

        // The base default declines the conflict, allowing DispatchGroup to
        // issue its standard incompatible-method warning and use the default
        // arithmetic implementation.
        let choose_default_formals = formals_from_specs(&[
            arg("x"),
            arg("y"),
            arg("mx"),
            arg("my"),
            arg("cl"),
            arg("reverse"),
        ]);
        let _choose_default_formals_guard = super::super::protect::protect(choose_default_formals);
        let choose_default_body = Rf_ScalarLogical(FALSE);
        let _choose_default_body_guard = super::super::protect::protect(choose_default_body);
        let choose_default_closure = crate::mainutils::dstruct::mkCLOSXP(
            choose_default_formals,
            choose_default_body,
            base_env,
        );
        let _choose_default_closure_guard = super::super::protect::protect(choose_default_closure);
        defineVar(
            Rf_install_in_current("chooseOpsMethod.default"),
            choose_default_closure,
            base_env,
        );
    }
}
