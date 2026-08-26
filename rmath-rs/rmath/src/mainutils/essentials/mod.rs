//! Essential R built-in functions — c(), seq(), rep(), paste(), cat(), typeof(), is.na(), names().
//!
//! These are the most fundamental R functions that every R program uses.
//! Split into domain submodules; every public path `crate::mainutils::essentials::*`
//! resolves exactly as before via the glob re-exports below.

//! Essential R built-in functions — c(), seq(), rep(), paste(), cat(), typeof(), is.na(), names().
//!
//! These are the most fundamental R functions that every R program uses.

use std::ffi::CString;
use std::os::raw::c_int;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::{R_NilValue, R_UnboundValue};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

mod conditions;
mod functional;
mod io;
mod mathstats;
mod matrix;
mod print;
mod runtime;
mod s3;
mod s4;
mod sets;
mod shared;
mod strings;
mod tables;
#[cfg(test)]
mod tests;
mod vectors;
pub use self::conditions::*;
pub use self::functional::*;
pub use self::io::*;
pub use self::mathstats::*;
pub use self::matrix::*;
pub use self::print::*;
pub use self::runtime::*;
pub use self::s3::*;
pub use self::s4::*;
pub use self::sets::*;
pub use self::shared::*;
pub use self::strings::*;
pub use self::tables::*;
pub use self::vectors::*;

// ---------------------------------------------------------------------------
// Core vector/scalar helpers live in `essentials_basic`.
// ---------------------------------------------------------------------------

pub use super::essentials_basic::*;

// ---------------------------------------------------------------------------
// Distribution-function builtins live in the `distributions` submodule and are
// re-exported here so registration paths (crate::mainutils::essentials::do_dnorm)
// stay valid. See rport-btb7 for the incremental decomposition plan.
// ---------------------------------------------------------------------------
pub mod distributions;
pub use self::distributions::*;
pub mod environment_bindings;
pub use self::environment_bindings::*;

// ---------------------------------------------------------------------------
// Register essentials builtins
// ---------------------------------------------------------------------------

/// Register essential builtins in the base environment.
pub unsafe fn register_essentials_builtins(env: SEXP) {
    unsafe {
        use crate::sexp::accessors::SET_FRAME;

        let all_fns = [
            "c",
            "seq",
            "sequence",
            "seq_len",
            "seq_along",
            "rep",
            "rep.int",
            "rep_len",
            "paste",
            "paste0",
            "cat",
            "print",
            "typeof",
            "mode",
            "storage.mode",
            "storage.mode<-",
            "identity",
            "is.na",
            "names",
            "logical",
            "integer",
            "numeric",
            "double",
            "single",
            "complex",
            "character",
            "raw",
            "vector",
            "which",
            "ifelse",
            "any",
            "all",
            "table",
            "tabulate",
            "pairlist",
            "simplify2array",
            "match.arg",
            "char.expand",
            "type.convert",
            "as.environment",
            "pos.to.env",
            "sort.list",
            "match.fun",
            "as.integer",
            "as.double",
            "as.character",
            "as.Date",
            "as.POSIXct",
            "as.logical",
            "as.pairlist",
            "as.list",
            "as.vector",
            "as.call",
            "length",
            "nchar",
            "substr",
            "tolower",
            "toupper",
            "enc2native",
            "enc2utf8",
            "trimws",
            "sprintf",
            "gsub",
            "sub",
            "grep",
            "grepl",
            "agrep",
            "agrepl",
            "pcre_config",
            "strsplit",
            "pmin",
            "pmax",
            "cumsum",
            "cumprod",
            "cumvar",
            "which.min",
            "which.max",
            "append",
            "head",
            "tail",
            "sort",
            "rev",
            "unique",
            ".primTrace",
            ".primUntrace",
            "@",
            "@<-",
            "$<-",
            ".cache_class",
            "...elt",
            "...length",
            "...names",
            "forceAndCall",
            "declare",
            "environment<-",
            "standardGeneric",
            "xtfrm",
            ".External.graphics",
            "browser",
            "[",
            ".subset",
            "[[",
            ".subset2",
            "setdiff",
            "union",
            "intersect",
            "setequal",
            "is.finite",
            "is.infinite",
            "is.nan",
            "is.matrix",
            "is.array",
            "is.list",
            "chartr",
            "format",
            "weekdays",
            "months",
            "quarters",
            "format.info",
            "apply",
            "tapply",
            "mapply",
            "outer",
            "sweep",
            "abs",
            "sign",
            "ceiling",
            "floor",
            "round",
            "trunc",
            "sqrt",
            "log",
            "log2",
            "log10",
            "exp",
            "dnorm",
            "pnorm",
            "qnorm",
            "dpois",
            "ppois",
            "qpois",
            "dbinom",
            "pbinom",
            "qbinom",
            "dgamma",
            "pgamma",
            "qgamma",
            "dcauchy",
            "pcauchy",
            "qcauchy",
            "dexp",
            "pexp",
            "qexp",
            "dbeta",
            "pbeta",
            "qbeta",
            "dt",
            "pt",
            "qt",
            "dchisq",
            "pchisq",
            "qchisq",
            "dweibull",
            "pweibull",
            "qweibull",
            "df",
            "pf",
            "qf",
            "dnbinom",
            "pnbinom",
            "qnbinom",
            "dunif",
            "punif",
            "qunif",
            "dgeom",
            "pgeom",
            "qgeom",
            "dlnorm",
            "plnorm",
            "qlnorm",
            "dlogis",
            "plogis",
            "qlogis",
            "dsignrank",
            "psignrank",
            "qsignrank",
            "dwilcox",
            "pwilcox",
            "qwilcox",
            "dhyper",
            "phyper",
            "qhyper",
            "ptukey",
            "qtukey",
            "dmultinom",
            "NROW",
            "NCOL",
            "nrow",
            "ncol",
            "tsp",
            "tsp<-",
            "lengths",
            "length<-",
            "rownames",
            "row.names",
            "colnames",
            "class",
            ".class2",
            "list",
            "data.frame",
            "attr",
            "attributes",
            "structure",
            "::",
            ":::",
            "comment",
            "unname",
            "oldClass",
            "names<-",
            "dim<-",
            "dimnames<-",
            "rownames<-",
            "row.names<-",
            "colnames<-",
            "class<-",
            "comment<-",
            "oldClass<-",
            "attr<-",
            "attributes<-",
            "noquote",
            "deparse",
            "nargs",
            "UseMethod",
            "NextMethod",
            "missing",
            "parent.frame",
            "sys.call",
            "sys.frame",
            "unclass",
            "oldClass",
            "getwd",
            "setwd",
            "basename",
            "dirname",
            "file.path",
            "file.show",
            "file.exists",
            "file.info",
            "file.size",
            "file.mtime",
            "list.files",
            "list.dirs",
            "normalizePath",
            "tempdir",
            "tempfile",
            "dir.exists",
            "dir.create",
            "file.create",
            "file.append",
            "file.link",
            "file.symlink",
            "file.remove",
            "file.rename",
            "file.copy",
            "file.access",
            "file.choose",
            "unlink",
            "nzchar",
            "lapply",
            "sapply",
            "vapply",
            "Map",
            "Filter",
            "do.call",
            "set.seed",
            "RNGkind",
            "runif",
            "rnorm",
            "rpois",
            "rexp",
            "sample",
            "sample.int",
            "is.atomic",
            "is.recursive",
            "is.object",
            "is.vector",
            "is.data.frame",
            "is.unsorted",
            "is.primitive",
            "is.loaded",
            "is.single",
            "file",
            "url",
            "textConnection",
            "textConnectionValue",
            "rawConnection",
            "close",
            "flush",
            "summary",
            "str",
            "as.data.frame",
            "unlist",
            // S3 print/summary dispatch
            "print.default",
            "print.data.frame",
            "print.table",
            "print.factor",
            "print.raw",
            "summary.default",
            "summary.data.frame",
            "format.data.frame",
            // Matrix/linear algebra
            "matrix",
            "array",
            "aperm",
            "backsolve",
            "asplit",
            "drop",
            "diag",
            "dim",
            "%*%",
            "crossprod",
            "tcrossprod",
            "max.col",
            "det",
            "solve",
            // Environment functions
            "emptyenv",
            "baseenv",
            "globalenv",
            "new.env",
            "environment",
            "ls",
            "lockBinding",
            "unlockBinding",
            "bindingIsLocked",
            "bindingIsActive",
            "makeActiveBinding",
            "lockEnvironment",
            "environmentIsLocked",
            // R runtime essentials
            "args",
            "formals",
            "body",
            // String/vector completion
            "charmatch",
            "pmatch",
            "charToRaw",
            "rawToChar",
            "strtoi",
            "strtrim",
            "regexpr",
            "gregexpr",
            "regexec",
            // Data manipulation
            "order",
            "rank",
            "duplicated",
            "anyDuplicated",
            "duplicated.array",
            "anyDuplicated.array",
            "match",
            "%in%",
            "diff",
            "setNames",
            "findInterval",
            "cut",
            // String operations
            "startsWith",
            "endsWith",
            // R runtime type checks
            "is.language",
            "is.call",
            "is.symbol",
            "is.name",
            "is.pairlist",
            "is.function",
            "is.expression",
            "is.environment",
            // S3
            "setOldClass",
            "methods",
            // Matrix
            "lower.tri",
            "upper.tri",
            // Math2 builtins
            "round",
            "signif",
            "trunc",
            "log2",
            // R runtime
            "eval",
            "substitute",
            "quote",
            "parse",
            // Error system
            "conditionMessage",
            "conditionCall",
            "simpleError",
            "simpleWarning",
            "withRestarts",
            // S3/S4
            "isS4",
            "is",
            "setClass",
            "setValidity",
            "isVirtualClass",
            // S4 class system
            "new",
            "show",
            "slotNames",
            "slot",
            "extends",
            "isSealedClass",
            "sealClass",
            "representation",
            "possibleExtends",
            "setReplaceMethod",
            "getMethod",
            "removeGeneric",
            "removeMethod",
            "isGeneric",
            "findMethod",
            "findMethods",
            "showMethods",
            "getGenerics",
            "getMethods",
            "existsMethod",
            "hasMethod",
            "selectMethod",
            // Complete I/O
            "scan",
            "write.table",
            "readLines",
            "writeLines",
            "sink",
            "sink.number",
            // Math/Statistics
            "cov",
            "cor",
            "scale",
            "rle",
            "inverse.rle",
            // R runtime
            "commandArgs",
            "getOption",
            "options",
            "interactive",
            "getRversion",
            "R.Version",
            // Complete data operations
            "reshape",
            "complete.cases",
            "na.omit",
            "na.exclude",
            // Complete string/vector
            "strwrap",
            "system.file",
            "system",
            "system2",
            // Complete R runtime
            "deparse1",
            "dput",
            "dget",
            "bquote",
            // Complete I/O
            "packageStartupMessage",
            // Environment completion
            "parent.env",
            "environmentName",
            "exists",
            "find",
            "get",
            "assign",
            "rm",
            "dyn.load",
            "dyn.unload",
            "library.dynam",
            // Complete S3 coercion
            "as.complex",
            "as.raw",
            "as",
            // Complete I/O
            "capture.output",
            "withVisible",
            "invisible",
            "proc.time",
            "stop",
            "warning",
            "message",
            "stopifnot",
            "suppressWarnings",
            "suppressMessages",
            "tryCatch",
            "force",
            // Complete R runtime
            "isTRUE",
            "isFALSE",
            "anyNA",
            // Complete list operations
            "modifyList",
            "split",
            // Complete R runtime — with/within/transform
            "with",
            "within",
            "transform",
            // Complete base R — table operations, factors, aggregation
            "prop.table",
            "addmargins",
            "ftable",
            "xtabs",
            "aggregate",
            "ave",
            "by",
            "interaction",
            "relevel",
            "droplevels",
            "factor",
            "ordered",
            "as.factor",
            "as.ordered",
            "gl",
            "addNA",
            "is.factor",
            "is.ordered",
            "levels",
            "levels<-",
            "nlevels",
            // Complete R runtime — Sys.* functions, R.home
            "R.home",
            "date",
            "Sys.getenv",
            "Sys.setenv",
            "Sys.unsetenv",
            "Sys.which",
            "Sys.info",
            "Sys.time",
            "Sys.sleep",
            "Sys.Date",
            "Sys.timezone",
            "OlsonNames",
            "Sys.localeconv",
            "Sys.getlocale",
            "Sys.setlocale",
            "Sys.readlink",
            "Sys.chmod",
            "Sys.umask",
            "path.expand",
            "l10n_info",
            "Cstack_info",
            "extSoftVersion",
            "Sys.getpid",
            "capabilities",
            // Complete data operations — subset
            "subset",
            // Complete R runtime — match.call, sys.nframe, sys.function, on.exit
            "match.call",
            "sys.nframe",
            "sys.function",
            "on.exit",
            // Complete I/O — read.csv, write.csv, read.table
            "read.csv",
            "write.csv",
            "read.table",
            // Complete connections — gzfile, pipe, fifo, socket, seek, pushBack, readBin, writeBin
            "gzfile",
            "bzfile",
            "xzfile",
            "pipe",
            "fifo",
            "socketConnection",
            "isOpen",
            "isIncomplete",
            "isSeekable",
            "seek",
            "pushBack",
            "pushBackLength",
            "readBin",
            "writeBin",
            // Complete S3 generics — as.matrix, as.numeric
            "as.matrix",
            "as.numeric",
            "inherits",
            "toString",
            // Complete R runtime — par, getGraphicsEvent
            "par",
            "layout",
            "getGraphicsEvent",
            // Complete R runtime — Rprof, Rprofmem, gc, gcinfo, memory.size, object.size
            "Rprof",
            "Rprofmem",
            "gc",
            "gc.time",
            "gcinfo",
            "gctorture",
            "gctorture2",
            "memory.size",
            "memory.profile",
            "object.size",
            // Complete I/O — European CSV, delimited, fixed-width
            "read.csv2",
            "write.csv2",
            "read.delim",
            "read.fwf",
            "readChar",
            "writeChar",
            // Complete S3 — method dispatch
            "getS3method",
            "registerS3method",
            "setGeneric",
            "setMethod",
            // Complete R runtime — serialization
            "readRDS",
            "saveRDS",
            "serialize",
            "unserialize",
            "save",
            "load",
            // Complete error handling — calling handlers and restarts
            "withCallingHandlers",
            "computeRestarts",
            "findRestart",
            "invokeRestart",
            "tryInvokeRestart",
            "isRestart",
            "restartDescription",
            // Complete package system
            ".libPaths",
            "library",
            "require",
            "installed.packages",
            "find.package",
            "packageVersion",
            "packageDescription",
            "loadNamespace",
            "requireNamespace",
            "getNamespace",
            "asNamespace",
            "loadedNamespaces",
            "data",
            "attach",
            "detach",
            "search",
            "searchpaths",
            // Complete R runtime — source, demo, example
            "source",
            "sys.source",
            "demo",
            "example",
            // Complete base R — colSums, rowSums, colMeans, rowMeans, col, row
            "colSums",
            "rowSums",
            "colMeans",
            "rowMeans",
            "col",
            "row",
            // Complete R runtime — cbind, rbind, t (transpose), statistics
            "cbind",
            "rbind",
            "t",
            "var",
            "sd",
            "median",
            "IQR",
            "cummin",
            "cummax",
            "cumvar",
            "dimnames",
            "Re",
            "Im",
            "Mod",
            "Arg",
            "Conj",
            "sin",
            "cos",
            "tan",
            "asin",
            "acos",
            "atan",
            "atan2",
            "expm1",
            "log1p",
            "acosh",
            "asinh",
            "atanh",
            "cospi",
            "sinpi",
            "tanpi",
            // Core arithmetic — dispatched via do_summary/do_math1 in eval.rs
            "sum",
            "min",
            "max",
            "prod",
            "range",
            // Core math — dispatched via do_math1 in eval.rs
            "ceiling",
            "floor",
            "sqrt",
            "log",
            "log10",
            "exp",
            "sinh",
            "cosh",
            "tanh",
            // Type checks — dispatched via do_is_type in eval.rs
            "is.numeric",
            "is.integer",
            "is.double",
            "is.logical",
            "is.character",
            "is.null",
            "identical",
            // Complete special functions for libRmath
            "lgamma",
            "gamma",
            "digamma",
            "trigamma",
            "psigamma",
            "beta",
            "lbeta",
            "choose",
            "lchoose",
            "factorial",
            "lfactorial",
            "besselI",
            "besselJ",
            "besselK",
            "besselY",
        ];

        let frame = (*env).data.envsxp.frame;
        let mut chain = frame;
        for name in all_fns {
            let kind = match name {
                "quote" | "substitute" => SEXPTYPE::SPECIALSXP,
                _ => SEXPTYPE::BUILTINSXP,
            };
            let prim = crate::eval::primitive::make_primitive_binding(name, kind);
            let sym = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
            let cell = Rf_cons(prim, chain);
            (*cell).data.listsxp.tagval = sym;
            chain = cell;
        }
        let pi_sym = Rf_install(c"pi".as_ptr());
        let pi_value = Rf_ScalarReal(std::f64::consts::PI);
        let _pi_value_guard = protect(pi_value);
        let pi_cell = Rf_cons(pi_value, chain);
        (*pi_cell).data.listsxp.tagval = pi_sym;
        chain = pi_cell;

        let letters_value = static_string_vector(&[
            "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q",
            "r", "s", "t", "u", "v", "w", "x", "y", "z",
        ]);
        let _letters_guard = protect(letters_value);
        let letters_cell = Rf_cons(letters_value, chain);
        (*letters_cell).data.listsxp.tagval = Rf_install(c"letters".as_ptr());
        chain = letters_cell;

        let letters_upper_value = static_string_vector(&[
            "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q",
            "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
        ]);
        let _letters_upper_guard = protect(letters_upper_value);
        let letters_upper_cell = Rf_cons(letters_upper_value, chain);
        (*letters_upper_cell).data.listsxp.tagval = Rf_install(c"LETTERS".as_ptr());
        chain = letters_upper_cell;

        let version_value = do_R_version(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            R_NilValue(),
            env,
        );
        let _version_guard = protect(version_value);
        for name in ["R.version", "version"] {
            let sym = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
            let cell = Rf_cons(version_value, chain);
            (*cell).data.listsxp.tagval = sym;
            chain = cell;
        }

        let version_string = do_R_version_string(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            R_NilValue(),
            env,
        );
        let _version_string_guard = protect(version_string);
        let sym = Rf_install(c"R.version.string".as_ptr());
        let cell = Rf_cons(version_string, chain);
        (*cell).data.listsxp.tagval = sym;
        chain = cell;
        SET_FRAME(env, chain);
    }
}
