//! init bindings_datetime bindings — extracted verbatim from init.rs; binding order preserved.

use super::*;

pub(crate) unsafe fn install(base_env: SEXP) {
    unsafe {
        // GNU stats::setNames is a closure so `setNames(, nm)` uses the
        // `object = nm` default instead of evalList's empty-arg error.
        eval_base_binding(
            base_env,
            "setNames",
            "function(object = nm, nm) { names(object) <- nm; object }",
        );
        eval_base_binding(
            base_env,
            "lazyLoadDBexec",
            include_str!("../gnu_lazyLoadDBexec.R"),
        );
        eval_base_binding(base_env, "lazyLoad", include_str!("../gnu_lazyLoad.R"));
        eval_base_binding(
            base_env,
            "%notin%",
            "function(x, table) match(x, table, nomatch = 0L) == 0L",
        );

        // GNU datetime.R constructors: class + tzone/units only.
        eval_base_binding(
            base_env,
            ".POSIXlt",
            "function(xx, tz = NULL, cl = c(\"POSIXlt\", \"POSIXt\")) { class(xx) <- cl; attr(xx, \"tzone\") <- tz; xx }",
        );
        eval_base_binding(
            base_env,
            ".difftime",
            "function(xx, units, cl = \"difftime\") { class(xx) <- cl; attr(xx, \"units\") <- units; xx }",
        );
        // GNU datetime.R: ISOdatetime/ISOdate are closures over paste/strptime.
        eval_base_binding(
            base_env,
            "ISOdatetime",
            "function(year, month, day, hour, min, sec, tz = \"\") {\n\
             if(min(lengths(list(year, month, day, hour, min, sec), use.names=FALSE)) == 0L)\n\
             .POSIXct(integer(), tz = tz)\n\
             else {\n\
             x <- paste(year, month, day, hour, min, sec, sep = \"-\")\n\
             as.POSIXct(strptime(x, \"%Y-%m-%d-%H-%M-%OS\", tz = tz), tz = tz)\n\
             }\n\
             }",
        );
        eval_base_binding(
            base_env,
            "ISOdate",
            "function(year, month, day, hour = 12, min = 0, sec = 0, tz = \"GMT\")\n\
             ISOdatetime(year, month, day, hour, min, sec, tz)",
        );
        // GNU all.equal.R: dispatches via as.POSIXct, ignoring POSIXlt
        // bookkeeping attributes like "balanced".
        eval_base_binding(
            base_env,
            "all.equal.POSIXt",
            "function(target, current, ..., tolerance = 1e-3, scale, check.tzone = TRUE) {\n\
             if(!inherits(target, \"POSIXt\")) return(\"'target' is not a POSIXt\")\n\
             if(!inherits(current, \"POSIXt\")) return(\"'current' is not a POSIXt\")\n\
             target  <- as.POSIXct(target)\n\
             current <- as.POSIXct(current)\n\
             msg <- NULL\n\
             if(check.tzone) {\n\
             tz <- function(dt) {\n\
             if(is.null(tz <- attr(dt, \"tzone\"))) \"\" else tz[1L]\n\
             }\n\
             tzt <- tz(target)\n\
             tzc <- tz(current)\n\
             if(!isTRUE(tzt == tzc))\n\
             msg <- sprintf(\"'tzone' attributes are inconsistent ('%s' and '%s')\", tzt, tzc)\n\
             }\n\
             attr(target, \"tzone\") <- attr(current, \"tzone\") <- NULL\n\
             val <- all.equal.numeric(target, current, ..., tolerance = tolerance, scale = 1)\n\
             if(!isTRUE(val)) msg <- c(msg, val)\n\
             if(is.null(msg)) TRUE else msg\n\
             }",
        );
        eval_base_binding(base_env, "names.POSIXlt", "function(x) names(x$year)");
        // GNU datetime.R: `[.difftime` keeps class and units through NextMethod.
        eval_base_binding(
            base_env,
            "[.difftime",
            "function(x, ..., drop = TRUE)\n\
             .difftime(NextMethod(\"[\"), attr(x, \"units\"), oldClass(x))",
        );
        // GNU datetime.R: format.difftime appends the units.
        eval_base_binding(
            base_env,
            "format.difftime",
            "function(x, ..., with.units = TRUE) {\n\
             y <- if (length(x))\n\
             paste0(format(unclass(x), ...), if (with.units) paste0(\" \", units(x)))\n\
             else\n\
             character()\n\
             names(y) <- names(x)\n\
             y\n\
             }",
        );
        // GNU datetime.R: strftime is format(as.POSIXlt(x, tz), ...) so a
        // vector 'format' recycles and names travel from the POSIXlt.
        eval_base_binding(
            base_env,
            "strftime",
            "function(x, format = \"\", tz = \"\", usetz = FALSE, ...)\n\
             format(as.POSIXlt(x, tz = tz), format = format, usetz = usetz, ...)",
        );
        // GNU datetime.R as.POSIXlt.POSIXct: strptime's Inf/-Inf handling
        // calls it by name, and it must reach the .Internal form so it can
        // never re-enter the generic.
        eval_base_binding(
            base_env,
            "as.POSIXlt.POSIXct",
            include_str!("../gnu_as_POSIXlt_POSIXct.R"),
        );
        // GNU dates.R/datetime.R +/- methods. Once Ops.difftime is bound,
        // Date/POSIXt arithmetic must dispatch through these so the
        // dispatch.rs precedence rules (Ops.difftime yields to +/-.Date and
        // +/-.POSIXt) resolve exactly like GNU.
        eval_base_binding(base_env, "+.Date", include_str!("../gnu_plus_Date.R"));
        eval_base_binding(base_env, "-.Date", include_str!("../gnu_minus_Date.R"));
        eval_base_binding(base_env, "+.POSIXt", include_str!("../gnu_plus_POSIXt.R"));
        eval_base_binding(base_env, "-.POSIXt", include_str!("../gnu_minus_POSIXt.R"));
        eval_base_binding(
            base_env,
            ".check_tzones",
            include_str!("../gnu_check_tzones.R"),
        );
        eval_base_binding(
            base_env,
            "length.POSIXlt",
            "function(x) max(lengths(unclass(x)))",
        );
        eval_base_binding(
            base_env,
            "names<-.POSIXlt",
            "function(x, value) { n <- length(x); yr <- x$year; if (length(yr) < n) x$year <- rep_len(yr, n); if (length(value) < n) value <- c(as.character(value), rep(NA_character_, n - length(value))); if (length(value) > n) value <- value[seq_len(n)]; names(x$year) <- value; x }",
        );
        {
            const LEAP: [f64; 27] = [
                78_796_800.0,
                94_694_400.0,
                126_230_400.0,
                157_766_400.0,
                189_302_400.0,
                220_924_800.0,
                252_460_800.0,
                283_996_800.0,
                315_532_800.0,
                362_793_600.0,
                394_329_600.0,
                425_865_600.0,
                489_024_000.0,
                567_993_600.0,
                631_152_000.0,
                662_688_000.0,
                709_948_800.0,
                741_484_800.0,
                773_020_800.0,
                820_454_400.0,
                867_715_200.0,
                915_148_800.0,
                1_136_073_600.0,
                1_230_768_000.0,
                1_341_100_800.0,
                1_435_708_800.0,
                1_483_228_800.0,
            ];
            let leap = crate::sexp::constructors::Rf_allocVector3(
                crate::sexp::ffi::SEXPTYPE::REALSXP,
                LEAP.len() as i64,
            );
            let _l = super::super::protect::protect(leap);
            for (i, sec) in LEAP.iter().enumerate() {
                *crate::sexp::accessors::REAL(leap).add(i) = *sec;
            }
            let klass =
                crate::sexp::constructors::Rf_allocVector3(crate::sexp::ffi::SEXPTYPE::STRSXP, 2);
            let _k = super::super::protect::protect(klass);
            crate::sexp::accessors::SET_STRING_ELT(
                klass,
                0,
                crate::sexp::constructors::Rf_mkChar(c"POSIXct".as_ptr()),
            );
            crate::sexp::accessors::SET_STRING_ELT(
                klass,
                1,
                crate::sexp::constructors::Rf_mkChar(c"POSIXt".as_ptr()),
            );
            crate::sexp::attrib_core::R_classgets(leap, klass);
            defineVar(Rf_install_in_current(".leap.seconds"), leap, base_env);
        }

        // GNU New-Internal.R: deparse is a closure so warning() attributes
        // to `deparse(...)`, not the caller of the primitive. The 4th
        // `.Internal` argument stays the character `control` vector: our
        // bit layout is not GNU Defn.h's, so `.deparseOpts()` integers
        // cannot be forwarded yet.
        eval_base_binding(
            base_env,
            "deparse",
            r#"function(expr, width.cutoff = 60L,
         backtick = mode(expr) %in% c("call", "expression", "(", "function"),
         control = c("keepNA", "keepInteger", "niceNames", "showAttributes"),
         nlines = -1L)
    .Internal(deparse(expr, width.cutoff, backtick, control, nlines))"#,
        );
        // GNU cat.R: visible `cat` is a closure so deparse does not treat
        // it as a primitive (inlist++), matching stock if-as-arg wrapping.
        eval_base_binding(
            base_env,
            "cat",
            r#"function(..., file = "", sep = " ", fill = FALSE,
                labels = NULL, append = FALSE)
    .Internal(cat(list(...), file, sep, fill, labels, append))"#,
        );

        // GNU source.R: withAutoprint is a closure so body()/formals()
        // and eval-etc's withVisible(withAutoprint({...})) match stock.
        eval_base_binding(
            base_env,
            "withAutoprint",
            r#"function(exprs, evaluated = FALSE, local = parent.frame(),
                          print. = TRUE, echo = TRUE, max.deparse.length = Inf,
                          width.cutoff = max(20, getOption("width")),
                          deparseCtrl = c("keepInteger", "showAttributes", "keepNA"),
                          skip.echo = 0,
                          ...)
{
    if (!evaluated) {
        exprs <- substitute(exprs)
        if (is.call(exprs)) {
            if (exprs[[1]] == quote(`{`)) {
                exprs <- as.list(exprs)[-1]
                if (missing(skip.echo) && is.list(srcrefs <- attr(exprs, "srcref"))) {
                    skip.echo <- srcrefs[[1L]][7L] - 1L
                }
            }
        }
    }
    source(exprs = exprs, local = local, print.eval = print., echo = echo,
           max.deparse.length = max.deparse.length, width.cutoff = width.cutoff,
           deparseCtrl = deparseCtrl, skip.echo = skip.echo, ...)
}"#,
        );
    }
}
