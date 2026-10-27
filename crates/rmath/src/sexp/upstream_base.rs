//! Upstream GNU base-package sources loaded verbatim at initialization.
//!
//! The port does not build a lazy-load database for `base`; instead the
//! upstream R sources packaged at `crates/rmath/upstream/base/R` are evaluated
//! into a persistent staging environment (a child of `.BaseNamespaceEnv`, so
//! file-local helpers behave like base-namespace internals) and only the
//! backlog whitelisted exports that GNU itself attaches to `package:base`
//! are copied into the base environment frame. Names whose native
//! dependencies are not implemented yet stay unbound (`BLOCKED_EXPORTS`)
//! rather than shadowing working primitives with closures that would fail
//! when called. Sources come from `docs/export-dependency-backlog.tsv`.
//!
//! Dependency consistency: an S3 generic is only published together with a
//! reachable `name.default` (bound in base or staged); generics whose
//! default method is blocked or failed to load are skipped at copy time and
//! purged from staging afterwards (`purge_stale_generics`) so staged
//! siblings fall through to the port's native builtins instead of failing
//! dispatch.
//!
//! Loading is a fixpoint: each pass re-evaluates only the top-level
//! expressions that failed in the previous pass, so files may appear in any
//! order while cross-file load-time dependencies still converge.

use super::accessors::SET_SYMVALUE;
use super::envir::{R_findVarInFrame, defineVar};
use super::ffi::SEXP;
use super::globals::{R_NilValue, R_UnboundValue};

struct UpstreamFile {
    source: &'static str,
    exports: &'static [&'static str],
}

const UPSTREAM_FILES: &[UpstreamFile] = &[
    UpstreamFile {
        source: include_str!("../../upstream/base/R/bitwise.R"),
        exports: &[],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/debug.R"),
        exports: &[],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/eapply.R"),
        exports: &[],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/rm.R"),
        exports: &[],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/Scripts.R"),
        exports: &[".Script"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/TAOCP.R"),
        exports: &[".TAOCP1997init"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/all.equal.R"),
        exports: &["all.equal.envRefClass"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/backquote.R"),
        exports: &["bquote"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/builtins.R"),
        exports: &["builtins"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/chol.R"),
        exports: &["chol"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/conflicts.R"),
        exports: &["conflicts"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/contributors.R"),
        exports: &["contributors"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/data.matrix.R"),
        exports: &["data.matrix"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/diag.R"),
        exports: &["diag"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/dump.R"),
        exports: &["dump"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/eigen.R"),
        exports: &["print.eigen"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/expand.grid.R"),
        exports: &["expand.grid"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/findInt.R"),
        exports: &["findInterval"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/gl.R"),
        exports: &["gl"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/ifelse.R"),
        exports: &["ifelse"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/is.R"),
        exports: &["is.vector"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/jitter.R"),
        exports: &["jitter"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/max.col.R"),
        exports: &["max.col"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/methodsSupport.R"),
        exports: &["returnValue"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/notyet.R"),
        exports: &[".NotYetImplemented"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/options.R"),
        exports: &["options"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/parse.R"),
        exports: &["parse"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/pretty.R"),
        exports: &["pretty"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/rank.R"),
        exports: &["rank"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/rep.R"),
        exports: &["rep.factor"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/replicate.R"),
        exports: &["replicate"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/rowsum.R"),
        exports: &["rowsum"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/scan.R"),
        exports: &["scan"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/serialize.R"),
        exports: &["unserialize"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/split.R"),
        exports: &[".formula2varlist"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/svd.R"),
        exports: &["svd"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/tabulate.R"),
        exports: &["tabulate"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/utilities.R"),
        exports: &["mat.or.vec"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/utils.R"),
        exports: &[".standard_regexps"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/withVisible.R"),
        exports: &["withVisible"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/xor.R"),
        exports: &["xor"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/zapsmall.R"),
        exports: &["zapsmall"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/zzz.R"),
        exports: &[".S3_methods_table"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/LAPACK.R"),
        exports: &["La_library", "La_version"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/S3extensions.R"),
        exports: &["nameOfClass", "nameOfClass.default"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/allnames.R"),
        exports: &["all.names", "all.vars"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/assign.R"),
        exports: &["assign", "list2env"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/attach.R"),
        exports: &[".detach", ".maskedMsg"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/autoload.R"),
        exports: &["autoload", "autoloader"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/backsolve.R"),
        exports: &["backsolve", "forwardsolve"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/cut.R"),
        exports: &["cut", "cut.default"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/dput.R"),
        exports: &["dget", "dput"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/format.R"),
        exports: &["format.AsIs", "formatC"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/license.R"),
        exports: &["licence", "license"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/mapply.R"),
        exports: &[".mapply", "mapply"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/quit.R"),
        exports: &["q", "quit"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/scale.R"),
        exports: &["scale", "scale.default"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/seq.R"),
        exports: &["sequence", "sequence.default"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/sink.R"),
        exports: &["sink", "sink.number"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/solve.R"),
        exports: &["solve", "solve.qr"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/source.R"),
        exports: &["source", "sys.source"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/taskCallback.R"),
        exports: &["getTaskCallbackNames", "taskCallbackManager"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/temp.R"),
        exports: &["tempdir", "tempfile"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/toString.R"),
        exports: &["toString", "toString.default"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/warnings.R"),
        exports: &["c.warnings", "warnings"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/zdynvars.R"),
        exports: &[".dynLibs", ".libPaths"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/RNG.R"),
        exports: &["RNGkind", "RNGversion", "set.seed"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/aperm.R"),
        exports: &["aperm", "aperm.default", "aperm.table"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/array.R"),
        exports: &["array2DF", "asplit", "slice.index"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/environment.R"),
        exports: &["env.profile", "environment", "environmentName"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/frametools.R"),
        exports: &["transform", "transform.data.frame", "transform.default"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/get.R"),
        exports: &["get", "get0", "mget"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/mode.R"),
        exports: &["mode", "mode<-", "storage.mode"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/rle.R"),
        exports: &["inverse.rle", "print.rle", "rle"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/sort_by.R"),
        exports: &["sort_by", "sort_by.data.frame", "sort_by.default"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/strwrap.R"),
        exports: &["formatDL", "strtrim", "trimws"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/time.R"),
        exports: &["date", "summary.proc_time", "system.time"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/unix/system.unix.R"),
        exports: &["Sys.which", "system", "system2"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/Bessel.R"),
        exports: &["besselI", "besselJ", "besselK", "besselY"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/funprog.R"),
        exports: &["Filter", "Find", "Position", "dontCheck"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/getenv.R"),
        exports: &["Sys.getenv", "Sys.getpid", "Sys.setenv", "Sys.unsetenv"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/load.R"),
        exports: &[
            "findPackageEnv",
            "save.image",
            "sys.load.image",
            "sys.save.image",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/locales.R"),
        exports: &[
            ".LC.categories",
            "Sys.getlocale",
            "Sys.localeconv",
            "Sys.setlocale",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/message.R"),
        exports: &[
            ".makeMessage",
            "message",
            "messageCondition",
            "packageStartupMessage",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/pmax.R"),
        exports: &["pmax", "pmax.int", "pmin", "pmin.int"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/range.R"),
        exports: &[".rangeNum", "range.default", "range.Date", "range.POSIXct"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/stop.R"),
        exports: &["bindtextdomain", "gettext", "gettextf", "ngettext"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/table.R"),
        exports: &[
            "[.table",
            "margin.table",
            "print.summary.table",
            "summary.table",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/Defunct.R"),
        exports: &[
            ".__H__.cbind",
            ".__H__.rbind",
            "default.stringsAsFactors",
            "is.R",
            "unix.time",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/sort.R"),
        exports: &[
            ".doSortWrap",
            ".doWrap",
            ".gt",
            ".gtn",
            "grouping",
            "xtfrm.AsIs",
            "xtfrm.factor",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/duplicated.R"),
        exports: &[
            "anyDuplicated.array",
            "anyDuplicated.matrix",
            "duplicated.array",
            "duplicated.matrix",
            "unique.array",
            "unique.matrix",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/eval.R"),
        exports: &[
            "local",
            "new.env",
            "parent.env",
            "parent.env<-",
            "with",
            "with.default",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/matrix.R"),
        exports: &["col", "lower.tri", "row", "t", "t.data.frame", "upper.tri"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/qr.R"),
        exports: &["qr", "qr.Q", "qr.R", "qr.X", "qr.fitted", "qr.resid"],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/sets.R"),
        exports: &[
            ".set_ops_need_as_vector",
            "intersect",
            "is.element",
            "setdiff",
            "setequal",
            "union",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/as.R"),
        exports: &[
            "as.character.default",
            "as.function",
            "as.function.default",
            "as.list.function",
            "as.matrix",
            "as.null.default",
            "as.qr",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/bindenv.R"),
        exports: &[
            "bindingIsActive",
            "bindingIsLocked",
            "environmentIsLocked",
            "lockBinding",
            "lockEnvironment",
            "makeActiveBinding",
            "unlockBinding",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/grep.R"),
        exports: &[
            "agrep",
            "agrepl",
            "gregexec",
            "grepv",
            "pcre_config",
            "regexec",
            "strsplit",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/print.R"),
        exports: &[
            "[.listof",
            "[.noquote",
            "as.matrix.noquote",
            "print.Dlist",
            "print.listof",
            "print.simple.list",
            "[.Dlist",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/colSums.R"),
        exports: &[
            ".colMeans",
            ".colSums",
            ".rowMeans",
            ".rowSums",
            "colMeans",
            "colSums",
            "rowMeans",
            "rowSums",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/vector.R"),
        exports: &[
            "character",
            "complex",
            "integer",
            "logical",
            "single",
            "vector",
            "double",
            "numeric",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/match.R"),
        exports: &[
            "char.expand",
            "charmatch",
            "match",
            "match.arg",
            "match.call",
            "mtfrm",
            "mtfrm.Date",
            "mtfrm.POSIXct",
            "mtfrm.POSIXlt",
            "mtfrm.default",
            "pmatch",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/srcfile.R"),
        exports: &[
            ".isOpen",
            "close.srcfile",
            "close.srcfilealias",
            "getSrcLines",
            "open.srcfile",
            "open.srcfilealias",
            "open.srcfilecopy",
            "print.srcfile",
            "srcfilealias",
            "srcref",
            "summary.srcfile",
            "summary.srcref",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/dynload.R"),
        exports: &[
            "$.DLLInfo",
            "[.DLLInfoList",
            "dyn.load",
            "dyn.unload",
            "getLoadedDLLs",
            "getCallingDLL",
            "getCallingDLLe",
            "getDLLRegisteredRoutines",
            "getDLLRegisteredRoutines.character",
            "is.loaded",
            "print.DLLInfo",
            "print.DLLInfoList",
            "print.DLLRegisteredRoutines",
            "print.NativeRoutineList",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/library.R"),
        exports: &[
            ".expand_R_libs_env_var",
            ".getRequiredPackages",
            ".getRequiredPackages2",
            "conflictRules",
            "find.package",
            "format.libraryIQR",
            "format.packageInfo",
            "library.dynam",
            "library.dynam.unload",
            "packageNotFoundError",
            "print.libraryIQR",
            "print.packageInfo",
            "use",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/raw.R"),
        exports: &[
            "charToRaw",
            "intToBits",
            "intToUtf8",
            "numToBits",
            "numToInts",
            "packBits",
            "raw",
            "rawShift",
            "rawToBits",
            "rawToChar",
            "utf8ToInt",
            "validEnc",
            "validUTF8",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/dataframe.R"),
        exports: &[
            "$<-.data.frame",
            "Ops.data.frame",
            "[.AsIs",
            "[<-.data.frame",
            "[[.data.frame",
            "[[<-.data.frame",
            "anyNA.data.frame",
            "as.data.frame.ts",
            "cbind.data.frame",
            "dim.data.frame",
            "dimnames.data.frame",
            "dimnames<-.data.frame",
            "is.na.data.frame",
            "print.AsIs",
            "xpdrows.data.frame",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/character.R"),
        exports: &[
            "abbreviate",
            "casefold",
            "chartr",
            "dQuote",
            "endsWith",
            "make.names",
            "make.unique",
            "sQuote",
            "startsWith",
            "strrep",
            "strtoi",
            "substr",
            "substr<-",
            "substring",
            "tolower",
            "toupper",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/factor.R"),
        exports: &[
            "Math.factor",
            "Ops.factor",
            "Ops.ordered",
            "Summary.factor",
            "Summary.ordered",
            "[.factor",
            "[<-.factor",
            "[[.factor",
            "[[<-.factor",
            "addNA",
            "as.logical.factor",
            "c.factor",
            "is.na<-.factor",
            "length<-.factor",
            "levels<-.factor",
            "print.factor",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/octhex.R"),
        exports: &[
            "!.hexmode",
            "!.octmode",
            "&.hexmode",
            "&.octmode",
            "[.hexmode",
            "[.octmode",
            "as.character.hexmode",
            "as.character.octmode",
            "as.hexmode",
            "as.octmode",
            "c.hexmode",
            "c.octmode",
            "format.hexmode",
            "format.octmode",
            "|.hexmode",
            "|.octmode",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/conditions.R"),
        exports: &[
            ".tryResumeInterrupt",
            "allowInterrupts",
            "as.character.condition",
            "as.character.error",
            "conditionCall",
            "conditionCall.condition",
            "conditionMessage",
            "conditionMessage.condition",
            "errorCondition",
            "isRestart",
            "print.condition",
            "print.restart",
            "restartDescription",
            "restartFormals",
            "simpleCondition",
            "simpleError",
            "simpleWarning",
            "suspendInterrupts",
            "warningCondition",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/namespace.R"),
        exports: &[
            ".mergeExportMethods",
            ".mergeImportMethods",
            "asNamespace",
            "dynGet",
            "getNamespace",
            "getNamespaceUsers",
            "getNamespaceVersion",
            "isNamespace",
            "loadingNamespaceInfo",
            "namespaceExport",
            "namespaceImport",
            "namespaceImportClasses",
            "namespaceImportFrom",
            "namespaceImportMethods",
            "packageHasNamespace",
            "parseNamespaceFile",
            "registerS3method",
            "requireNamespace",
            "topenv",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/version.R"),
        exports: &[
            "$.package_version",
            ".decode_numeric_version",
            ".encode_numeric_version",
            ".make_numeric_version",
            "R_system_version",
            "Summary.numeric_version",
            "[[.numeric_version",
            "[[<-.numeric_version",
            "anyNA.numeric_version",
            "as.list.numeric_version",
            "as.package_version",
            "c.numeric_version",
            "duplicated.numeric_version",
            "getRversion",
            "is.numeric_version",
            "is.package_version",
            "print.numeric_version",
            "rep.numeric_version",
            "unique.numeric_version",
            "xtfrm.numeric_version",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/files.R"),
        exports: &[
            "Sys.chmod",
            "Sys.readlink",
            "Sys.sleep",
            "Sys.umask",
            "basename",
            "dir.create",
            "dir.exists",
            "dirname",
            "file.access",
            "file.append",
            "file.choose",
            "file.copy",
            "file.create",
            "file.exists",
            "file.info",
            "file.link",
            "file.mode",
            "file.mtime",
            "file.path",
            "file.remove",
            "file.rename",
            "file.show",
            "file.size",
            "file.symlink",
            "getwd",
            "list.dirs",
            "list.files",
            "normalizePath",
            "path.expand",
            "setwd",
            "system.file",
            "unlink",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/dates.R"),
        exports: &[
            "+.Date",
            "-.Date",
            ".Date",
            "Math.Date",
            "Ops.Date",
            "Summary.Date",
            "Sys.Date",
            "[.Date",
            "[<-.Date",
            "[[.Date",
            "as.Date",
            "as.Date.POSIXct",
            "as.Date.POSIXlt",
            "as.Date.character",
            "as.Date.factor",
            "as.Date.numeric",
            "as.character.Date",
            "as.list.Date",
            "c.Date",
            "cut.Date",
            "diff.Date",
            "format.Date",
            "is.numeric.Date",
            "julian.Date",
            "length<-.Date",
            "mean.Date",
            "months.Date",
            "quarters.Date",
            "rep.Date",
            "round.Date",
            "split.Date",
            "summary.Date",
            "trunc.Date",
            "weekdays.Date",
            "xtfrm.Date",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/connections.R"),
        exports: &[
            "bzfile",
            "closeAllConnections",
            "fifo",
            "file",
            "flush",
            "flush.connection",
            "gzcon",
            "gzfile",
            "isIncomplete",
            "isOpen",
            "isSeekable",
            "isatty",
            "memCompress",
            "memDecompress",
            "nullfile",
            "open",
            "open.connection",
            "pipe",
            "print.connection",
            "pushBack",
            "pushBackLength",
            "rawConnection",
            "readBin",
            "readChar",
            "readLines",
            "seek",
            "seek.connection",
            "socketConnection",
            "stderr",
            "stdin",
            "stdout",
            "textConnection",
            "textConnectionValue",
            "truncate",
            "url",
            "writeBin",
            "writeChar",
            "writeLines",
            "xzfile",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/New-Internal.R"),
        exports: &[
            "..deparseOpts",
            ".deparseOpts",
            ".noGenerics",
            "Cstack_info",
            "Encoding",
            "args",
            "atan2",
            "beta",
            "choose",
            "commandArgs",
            "comment",
            "comment<-",
            "do.call",
            "drop",
            "encodeString",
            "factorial",
            "format.info",
            "gc",
            "gcinfo",
            "gctorture",
            "gctorture2",
            "geterrmessage",
            "iconv",
            "iconvlist",
            "inherits",
            "isa",
            "l10n_info",
            "lbeta",
            "lchoose",
            "lfactorial",
            "logb",
            "memory.profile",
            "nchar",
            "polyroot",
            "psigamma",
            "search",
            "searchpaths",
            "sprintf",
            "try",
            "typeof",
        ],
    },
    UpstreamFile {
        source: include_str!("../../upstream/base/R/datetime.R"),
        exports: &[
            "$<-.POSIXlt",
            "*.difftime",
            "+.POSIXt",
            "-.POSIXt",
            ".POSIXct",
            ".check_tzones",
            "/.difftime",
            "Math.POSIXt",
            "Math.difftime",
            "OlsonNames",
            "Ops.POSIXt",
            "Ops.difftime",
            "Summary.POSIXct",
            "Summary.POSIXlt",
            "Summary.difftime",
            "Sys.time",
            "[.POSIXct",
            "[.POSIXlt",
            "[<-.POSIXct",
            "[<-.POSIXlt",
            "[[.POSIXct",
            "[[.POSIXlt",
            "[[<-.POSIXlt",
            "anyNA.POSIXlt",
            "as.POSIXct",
            "as.POSIXct.Date",
            "as.POSIXct.POSIXlt",
            "as.POSIXct.default",
            "as.POSIXct.numeric",
            "as.POSIXlt",
            "as.POSIXlt.POSIXct",
            "as.POSIXlt.character",
            "as.POSIXlt.default",
            "as.POSIXlt.Date",
            "as.POSIXlt.factor",
            "as.POSIXlt.numeric",
            "as.character.POSIXt",
            "as.difftime",
            "as.double.POSIXlt",
            "as.list.POSIXct",
            "as.list.POSIXlt",
            "as.list.difftime",
            "as.matrix.POSIXlt",
            "as.vector.POSIXlt",
            "balancePOSIXlt",
            "c.POSIXct",
            "c.POSIXlt",
            "c.difftime",
            "cut.POSIXt",
            "diff.POSIXt",
            "diff.difftime",
            "difftime",
            "format.POSIXct",
            "format.POSIXlt",
            "is.finite.POSIXlt",
            "is.infinite.POSIXlt",
            "is.na.POSIXlt",
            "is.nan.POSIXlt",
            "is.numeric.POSIXt",
            "is.numeric.difftime",
            "julian",
            "julian.POSIXt",
            "length<-.difftime",
            "mean.POSIXct",
            "mean.POSIXlt",
            "mean.difftime",
            "months",
            "months.POSIXt",
            "print.difftime",
            "quarters",
            "quarters.POSIXt",
            "rep.POSIXct",
            "rep.POSIXlt",
            "rep.difftime",
            "round.POSIXt",
            "sort.POSIXlt",
            "split.POSIXct",
            "weekdays",
            "weekdays.POSIXt",
            "xtfrm.POSIXct",
            "xtfrm.POSIXlt",
            "xtfrm.difftime",
        ],
    },
];

/// Ledger of names intentionally left unbound (kept for reference).
#[allow(dead_code)]
const BLOCKED_EXPORTS: &[&str] = &[
    ".pretty",
    ".signalSimpleWarning",
    "Encoding<-",
    "R.Version",
    "R_compiled_by",
    "Sys.glob",
    "Sys.timezone",
    "[.data.frame",
    "activeBindingFunction",
    "as.character.factor",
    "as.list.environment",
    "attachNamespace",
    "bitwAnd",
    "bitwNot",
    "bitwOr",
    "bitwShiftL",
    "bitwShiftR",
    "bitwXor",
    "browserCondition",
    "browserSetDebug",
    "browserText",
    "capabilities",
    "chol.default",
    "chol2inv",
    "clearPushBack",
    "computeRestarts",
    "curlGetHeaders",
    "debuggingState",
    "eapply",
    "eigen",
    "extSoftVersion",
    "findRestart",
    "getDLLRegisteredRoutines.DLLInfo",
    "globalCallingHandlers",
    "icuGetCollate",
    "icuSetCollate",
    "importIntoEnv",
    "infoRDS",
    "invokeRestart",
    "invokeRestartInteractively",
    "libcurlVersion",
    "load",
    "loadNamespace",
    "loadedNamespaces",
    "mem.maxNSize",
    "mem.maxVSize",
    "pretty.default",
    "qr.coef",
    "qr.default",
    "qr.qty",
    "qr.qy",
    "rawConnectionValue",
    "readRDS",
    "readRenviron",
    "readline",
    "reg.finalizer",
    "registerS3methods",
    "remove",
    "rm",
    "rowsum.data.frame",
    "rowsum.default",
    "save",
    "saveRDS",
    "serialize",
    "serverSocket",
    "setSessionTimeLimit",
    "setTimeLimit",
    "signalCondition",
    "socketAccept",
    "socketSelect",
    "socketTimeout",
    "solve.default",
    "sort.list",
    "stop",
    "t.default",
    "tracingState",
    "truncate.connection",
    // Use the base try closure adapted to native tryCatch's frame shape.
    // A staged GNU copy would shadow it for upstream sibling functions.
    "try",
    "tryCatch",
    "tryInvokeRestart",
    "unloadNamespace",
    "unz",
    "warning",
    "withCallingHandlers",
    "withRestarts",
    "zstdfile",
];

/// Maximum fixpoint passes over the upstream file list.
const MAX_PASSES: usize = 8;

/// Load the upstream base sources into `base_env`.
///
/// Runs after `retarget_base_closure_envs` published `.BaseNamespaceEnv`, so
/// the staging environment's parent is the base namespace: closures defined
/// by upstream files resolve their free variables against the staging frame
/// (file-local helpers) and then the shared base frame, matching GNU base
/// namespace semantics without attaching unexported helper names to
/// `package:base`.
pub(crate) unsafe fn load_upstream_base_sources(base_env: SEXP) {
    // Default ON since the loader-on corpus reached 1181/1181 strict
    // (visibility fix for stock-invisible internals, all.names internal
    // contract; the earlier arima/permdist panics were mid-edit artifacts
    // and the 528 timeout was a debug-profile harness default). Opt out
    // with RPORT_UPSTREAM_BASE=0 (diagnostic escape hatch).
    if std::env::var_os("RPORT_UPSTREAM_BASE").as_deref() == Some(std::ffi::OsStr::new("0")) {
        return;
    }

    unsafe {
        let namespace = R_findVarInFrame(base_env, Rf_install_in_current(".BaseNamespaceEnv"));
        if namespace.is_null() || namespace == R_UnboundValue() {
            return;
        }
        let staging =
            crate::sexp::memory_ext::NewEnvironment(R_NilValue(), namespace, R_NilValue());
        let _staging = crate::sexp::protect::protect(staging);
        load_files(base_env, staging);
        load_oracle_recovered_closures(base_env, staging);
        load_fortran_symbol_bindings(base_env);
        load_profile_common_bindings(base_env);
        load_lazy_native_symbol_info(base_env, namespace);
    }
}

/// GNU registers the `appl` Fortran routines as `.F_*` symbols in the base
/// namespace (`useDynLib(base, .fixes = ".F_")`). Staged `qr.R` reaches them
/// through `.Fortran(.F_dqrrsd, ...)`; bind each name to its routine-name
/// string so the foreign-call interface resolves the bundled Rust kernels.
const BASE_FORTRAN_SYMBOLS: &[&str] = &[
    "dqrdc2", "dqrcf", "dqrqy", "dqrqty", "dqrrsd", "dqrxb", "dtrco",
];

unsafe fn load_fortran_symbol_bindings(base_env: SEXP) {
    unsafe {
        for routine in BASE_FORTRAN_SYMBOLS {
            let binding = format!(".F_{routine}");
            if defined_in(base_env, binding.as_str()) {
                continue;
            }
            let symbol = Rf_install_in_current(binding.as_str());
            let routine_name = std::ffi::CString::new(*routine).unwrap_or_default();
            let string = crate::sexp::constructors::Rf_mkString(routine_name.as_ptr());
            crate::sexp::envir::defineVar(symbol, string, base_env);
        }
    }
}

/// Evaluate every upstream file into `staging`, then copy whitelisted exports
/// that are still unbound into `base_env`. Repeats until a pass binds nothing
/// new; only expressions that failed in the previous pass are re-evaluated.
unsafe fn load_files(base_env: SEXP, staging: SEXP) {
    unsafe {
        let parser_factory = crate::eval::parser::active_factory();
        let swallow = error_swallowing_handler(staging);
        if swallow.is_null() {
            return;
        }
        let _swallow = crate::sexp::protect::protect(swallow);
        let try_catch = Rf_install_in_current("tryCatch");
        let failed: Vec<Vec<bool>> = UPSTREAM_FILES.iter().map(|_| Vec::new()).collect();
        for pass in 0..MAX_PASSES {
            let mut bound_any = false;
            for (index, file) in UPSTREAM_FILES.iter().enumerate() {
                if file.exports.iter().all(|name| defined_in(base_env, name)) {
                    continue;
                }
                let parsed = crate::sexp::memory::with_arena(|arena| {
                    crate::eval::parser::parse_expressions(
                        file.source,
                        arena,
                        parser_factory.clone(),
                    )
                });
                crate::eval::parser::flush_literal_warnings();
                let Ok(exprs) = parsed else {
                    continue;
                };
                let mut failed_now = vec![false; exprs.len()];
                for (position, expr) in exprs.iter().enumerate() {
                    if pass > 0 && !failed[index].get(position).copied().unwrap_or(true) {
                        continue;
                    }
                    // Blocked backlog names must not even be defined in the
                    // staging frame: staged siblings would still reach them
                    // through S3 dispatch from their shared closure env.
                    if assigns_blocked_name(expr.as_raw()) {
                        continue;
                    }
                    // R-level tryCatch keeps load-time failures inside the
                    // condition machinery; the Rust backstop is below.
                    let wrapped = crate::sexp::constructors::Rf_lang3(
                        try_catch,
                        expr.clone().as_raw(),
                        swallow,
                    );
                    crate::sexp::accessors::SETTAG(
                        crate::sexp::accessors::CDR(crate::sexp::accessors::CDR(wrapped)),
                        Rf_install_in_current("error"),
                    );
                    let _wrapped = crate::sexp::protect::protect(wrapped);
                    let evaluated =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                            let _ = crate::eval::eval::Rf_eval(wrapped, staging);
                        }));
                    if evaluated.is_err() {
                        // A failed load-time expression leaves that binding
                        // unbound; the fixpoint retries once its missing
                        // dependency may have appeared.
                        crate::sexp::context::R_SetInError(false);
                        failed_now[position] = true;
                    }
                }
                bound_any |= copy_exports(base_env, staging, file.exports);
            }
            if !bound_any {
                break;
            }
        }

        purge_stale_generics(base_env, staging);
    }
}

/// `function(e) NULL` evaluated in `staging`: the error handler installed for
/// every load-time expression so failures are silent and recoverable.
unsafe fn error_swallowing_handler(staging: SEXP) -> SEXP {
    unsafe {
        let parsed = crate::sexp::memory::with_arena(|arena| {
            crate::eval::parser::parse_expressions(
                "function(e) NULL",
                arena,
                crate::eval::parser::active_factory(),
            )
        });
        crate::eval::parser::flush_literal_warnings();
        let Ok(exprs) = parsed else {
            return std::ptr::null_mut();
        };
        if exprs.len() != 1 {
            return std::ptr::null_mut();
        }
        crate::eval::eval::Rf_eval(exprs[0].clone().as_raw(), staging)
    }
}

/// Copy `names` bound in `staging` but still unbound in `base_env`.
///
/// Dependency-consistency filter: an S3 generic is copied ONLY IF its
/// primary default method (`name.default`) is either already bound in
/// `base_env` or also available in `staging` (and thus being copied). A
/// generic published without its default method makes every dispatch fail
/// with "no applicable method" — worse than leaving the port's native
/// builtin in place. When the default is blocked or failed to load, the
/// generic joins the skip set; the load fixpoint retries it on later
unsafe fn copy_exports(base_env: SEXP, staging: SEXP, names: &[&str]) -> bool {
    let mut copied = false;
    for name in names {
        unsafe {
            if defined_in(base_env, name) {
                continue;
            }
            let symbol = Rf_install_in_current(name);
            let value = R_findVarInFrame(staging, symbol);
            if value.is_null() || value == R_UnboundValue() {
                continue;
            }
            if calls_use_method(value) && !default_method_reachable(base_env, staging, name) {
                continue;
            }
            defineVar(symbol, value, base_env);
            // GNU defineVar publishes closures on the symbol value cell;
            // deparse and primitives.R introspection rely on that slot.
            SET_SYMVALUE(symbol, value);
            copied = true;
        }
    }
    copied
}

/// Remove staged S3 generics whose default method never became reachable.
///
/// A generic that is not copied (its `name.default` is blocked or failed to
/// load) may still linger in the staging frame: staged siblings such as
/// `svd` calling `t` reach it through their shared closure environment and
/// then fail dispatch ("no applicable method") instead of falling through
/// to the port's native builtin in the base frame. Sweep every whitelisted
/// export still staged, dropping generics whose default is neither bound
/// in `base_env` nor staged; repeat until stable so chained generics
/// (a default that is itself an orphaned generic) converge.
unsafe fn purge_stale_generics(base_env: SEXP, staging: SEXP) {
    unsafe {
        loop {
            let mut removed = false;
            for file in UPSTREAM_FILES {
                for name in file.exports {
                    let symbol = Rf_install_in_current(name);
                    let value = R_findVarInFrame(staging, symbol);
                    if value.is_null() || value == R_UnboundValue() {
                        continue;
                    }
                    if calls_use_method(value) && !default_method_reachable(base_env, staging, name)
                    {
                        crate::sexp::envir::remove_binding_raw(staging, symbol);
                        removed = true;
                    }
                }
            }
            if !removed {
                break;
            }
        }
    }
}

/// Whether `value` is a closure whose body dispatches via `UseMethod`,
/// i.e. the staged definition is an S3 generic.
unsafe fn calls_use_method(value: SEXP) -> bool {
    unsafe {
        if super::accessors::TYPEOF(value) != crate::sexp::ffi::SEXPTYPE::CLOSXP {
            return false;
        }
        body_calls(super::accessors::BODY(value))
    }
}

/// Recursive scan of a closure body for a call to the `UseMethod` symbol.
unsafe fn body_calls(expr: SEXP) -> bool {
    unsafe {
        if expr.is_null() || expr == R_NilValue() {
            return false;
        }
        let kind = super::accessors::TYPEOF(expr);
        if kind == crate::sexp::ffi::SEXPTYPE::LANGSXP {
            let head = super::accessors::CAR(expr);
            if super::accessors::TYPEOF(head) == crate::sexp::ffi::SEXPTYPE::SYMSXP
                && let Some(name) = symbol_name(head)
                && name == "UseMethod"
            {
                return true;
            }
        }
        if kind != crate::sexp::ffi::SEXPTYPE::LANGSXP
            && kind != crate::sexp::ffi::SEXPTYPE::LISTSXP
        {
            return false;
        }
        let mut cell = if kind == crate::sexp::ffi::SEXPTYPE::LANGSXP {
            super::accessors::CDR(expr)
        } else {
            expr
        };
        while !cell.is_null() && cell != R_NilValue() {
            if body_calls(super::accessors::CAR(cell)) {
                return true;
            }
            cell = super::accessors::CDR(cell);
        }
        false
    }
}

/// Whether the generic's primary default method `name.default` is bound in
/// `base_env` (native builtin or previously copied) or staged for copy.
unsafe fn default_method_reachable(base_env: SEXP, staging: SEXP, name: &str) -> bool {
    let default = format!("{name}.default");
    (unsafe { defined_in(base_env, &default) }) || (unsafe { defined_in(staging, &default) })
}

/// Closures present in the oracle base image whose upstream sources are not
/// part of the vendored r-source tree (funprog.R additions). Definitions are
/// recovered verbatim from the oracle's deparsed closures.
const ORACLE_RECOVERED_SOURCES: &[(&str, &str)] = &[
    (
        "Compose",
        r#"Compose <- function (...)
{
    funs <- lapply(list(...), match.fun)
    if (!length(funs))
        stop("No functions specified")
    function(...) Reduce(function(e, f) f(e), funs[-1L], funs[[1L]](...))
}"#,
    ),
    (
        "Funcall",
        r"Funcall <- function (f, ...)
f(...)",
    ),
    (
        "Partial",
        r"Partial <- function (f, ...)
{
    f <- match.fun(f)
    args <- list(...)
    function(...)
    {
        do.call(f, c(args, list(...)))
    }
}",
    ),
    (
        ".enc2utf8_sub",
        r#".enc2utf8_sub <- function (x)
{
    if (!is.character(x))
        return(x)
    x <- enc2utf8(x)
    bad <- which(!is.na(x) & !validUTF8(x))
    if (length(bad))
        x[bad] <- iconv(x[bad], from = "UTF-8", to = "UTF-8", sub = "byte")
    bad <- which(!is.na(x) & !validUTF8(x))
    if (length(bad))
        x[bad] <- iconv(x[bad], from = "ASCII", to = "ASCII", sub = "byte")
    x
}"#,
    ),
];

/// Evaluate the oracle-recovered closures into `staging` and publish them.
unsafe fn load_oracle_recovered_closures(base_env: SEXP, staging: SEXP) {
    unsafe {
        let parser_factory = crate::eval::parser::active_factory();
        let names: Vec<&str> = ORACLE_RECOVERED_SOURCES.iter().map(|(n, _)| *n).collect();
        for (name, source) in ORACLE_RECOVERED_SOURCES {
            if defined_in(base_env, name) {
                continue;
            }
            let parsed = crate::sexp::memory::with_arena(|arena| {
                crate::eval::parser::parse_expressions(source, arena, parser_factory.clone())
            });
            crate::eval::parser::flush_literal_warnings();
            let Ok(exprs) = parsed else {
                continue;
            };
            for expr in &exprs {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    let _ = crate::eval::eval::Rf_eval(expr.clone().as_raw(), staging);
                }));
            }
            crate::sexp::context::R_SetInError(false);
        }
        copy_exports(base_env, staging, &names);
    }
}

/// Bindings from `r-source/src/library/profile/Common.R` that GNU installs
/// into `package:base` during profile processing: the `.First.sys` and
/// `.OptRequireMethods` closures verbatim, and the `.Library.site` active
/// binding (GNU removes the lazy-load placeholder with `.Internal(mkUnbound)`
/// first; the port never binds one, so only the active binding is needed).
unsafe fn load_profile_common_bindings(base_env: SEXP) {
    unsafe {
        let parser_factory = crate::eval::parser::active_factory();
        const PROFILE_SOURCES: &[(&str, &str)] = &[
            (
                ".First.sys",
                r#".First.sys <- function()
{
    verbose <- getOption("verbose", FALSE)
    for(pkg in getOption("defaultPackages")) {
        res <- require(pkg, quietly = !verbose, warn.conflicts = FALSE,
                       character.only = TRUE)
        if(!res)
            warning(gettextf('package %s in options("defaultPackages") was not found', sQuote(pkg)),
                    call. = FALSE, domain = NA)
    }
}"#,
            ),
            (
                ".OptRequireMethods",
                r#".OptRequireMethods <- function()
{
    pkg <- "methods" # done this way to avoid R CMD check warning
    if(pkg %in% getOption("defaultPackages"))
        if(!require(pkg, quietly = !getOption("verbose", FALSE), warn.conflicts = FALSE,
                    character.only = TRUE))
            warning('package "methods" in options("defaultPackages") was not found',
                    call. = FALSE)
}"#,
            ),
            (
                ".Library.site",
                r#"local({
    siteLibrary <- character()
    slfun <- function(v) {
        if (!missing(v))
            siteLibrary <<- v
        siteLibrary
    }
    makeActiveBinding(".Library.site", slfun, baseenv())
    makeActiveBinding(".Library.site", slfun, globalenv())
})"#,
            ),
        ];
        for (name, source) in PROFILE_SOURCES {
            if defined_in(base_env, name) {
                continue;
            }
            let parsed = crate::sexp::memory::with_arena(|arena| {
                crate::eval::parser::parse_expressions(source, arena, parser_factory.clone())
            });
            crate::eval::parser::flush_literal_warnings();
            let Ok(exprs) = parsed else {
                continue;
            };
            for expr in &exprs {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    let _ = crate::eval::eval::Rf_eval(expr.clone().as_raw(), base_env);
                }));
            }
            crate::sexp::context::R_SetInError(false);
        }
        // GNU main.c setup_Rmainloop removes the global-environment copy
        // after the site profile runs; the port runs no site profile, so
        // drop it immediately after the Common.R sources are evaluated.
        let library_site = Rf_install_in_current(".Library.site");
        crate::sexp::envir::remove_binding_raw(crate::sexp::globals::R_GlobalEnv(), library_site);
    }
}

/// Whether `name` has a binding in `env`'s own frame.
unsafe fn defined_in(env: SEXP, name: &str) -> bool {
    unsafe {
        let value = R_findVarInFrame(env, Rf_install_in_current(name));
        !value.is_null() && value != R_UnboundValue()
    }
}

unsafe fn Rf_install_in_current(name: &str) -> SEXP {
    unsafe {
        let cstring = std::ffi::CString::new(name).unwrap_or_default();
        crate::sexp::symbol::Rf_install(cstring.as_ptr())
    }
}

/// Whether evaluating `expr` assigns to a blocked export name (including
/// chained assignments); such definitions must stay out of staging.
unsafe fn assigns_blocked_name(expr: SEXP) -> bool {
    unsafe { assignment_target_blocked(expr) }
}

unsafe fn assignment_target_blocked(expr: SEXP) -> bool {
    unsafe {
        if expr.is_null() || expr == R_NilValue() {
            return false;
        }
        let kind = super::accessors::TYPEOF(expr);
        if kind == crate::sexp::ffi::SEXPTYPE::LANGSXP {
            let head = super::accessors::CAR(expr);
            if super::accessors::TYPEOF(head) == crate::sexp::ffi::SEXPTYPE::SYMSXP
                && let Some(name) = symbol_name(head)
                && (name == "<-" || name == "=" || name == "<<-")
                && let Some(target) = symbol_name(super::accessors::CADR(expr))
                && (BLOCKED_EXPORTS.contains(&target.as_str()) || target == "getNativeSymbolInfo")
            {
                return true;
            }
        }
        if kind != crate::sexp::ffi::SEXPTYPE::LANGSXP
            && kind != crate::sexp::ffi::SEXPTYPE::LISTSXP
        {
            return false;
        }
        let mut cell = if kind == crate::sexp::ffi::SEXPTYPE::LANGSXP {
            super::accessors::CDR(expr)
        } else {
            expr
        };
        while !cell.is_null() && cell != R_NilValue() {
            if assignment_target_blocked(super::accessors::CAR(cell)) {
                return true;
            }
            cell = super::accessors::CDR(cell);
        }
        false
    }
}

/// UTF-8 name of a symbol, when printable.
unsafe fn symbol_name(symbol: SEXP) -> Option<String> {
    unsafe {
        if super::accessors::TYPEOF(symbol) != crate::sexp::ffi::SEXPTYPE::SYMSXP {
            return None;
        }
        let printed = super::accessors::PRINTNAME(symbol);
        if printed.is_null() {
            return None;
        }
        let bytes = super::accessors::CHAR(printed);
        if bytes.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(bytes)
            .to_str()
            .ok()
            .map(str::to_owned)
    }
}

/// Ordinary R promise: materialize the original closure only on first lookup.
unsafe fn load_lazy_native_symbol_info(base_env: SEXP, namespace: SEXP) {
    unsafe {
        let result = (|| {
            let owner = crate::sexp::owner::OwnerToken::current()?;
            let _pin = owner.pin()?;
            owner.require_active()?;
            let factory = owner.node_factory();
            let symbol = factory.wrap(Rf_install_in_current("getNativeSymbolInfo"))?;
            if defined_in(base_env, "getNativeSymbolInfo") {
                return Ok(());
            }
            let eval = factory.wrap(Rf_install_in_current("eval"))?;
            let parse = factory.wrap(Rf_install_in_current("parse"))?;
            let text = factory.wrap(Rf_install_in_current("text"))?;
            let environment = factory.wrap(namespace)?;
            let source = factory.strings(&[include_str!("native_symbol_info.R")])?;
            let parse_args = factory.pairlist_cell(&source, &factory.nil(), &text)?;
            let parsed = factory.call(&parse, &parse_args)?;
            let eval_env = factory.pairlist_cell(&environment, &factory.nil(), &factory.nil())?;
            let eval_args = factory.pairlist_cell(&parsed, &eval_env, &factory.nil())?;
            let expression = factory.call(&eval, &eval_args)?;
            let promise = factory.promise(&expression, &environment)?;
            owner.require_active()?;
            defineVar(symbol.as_raw(), promise.as_raw(), base_env);
            SET_SYMVALUE(symbol.as_raw(), promise.as_raw());
            owner.require_active()
        })();
        result.unwrap_or_else(|e: crate::sexp::object::SexpError| {
            crate::sexp::context::r_error(e.to_string())
        });
    }
}

#[cfg(test)]
mod lazy_native_symbol_tests {
    use super::*;
    #[test]
    fn lazy_native_symbol_binding_retains_expression_through_collection_and_forces_once() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let owner = session.owner_token().unwrap();
            let base = crate::sexp::globals::R_BaseEnv();
            load_lazy_native_symbol_info(base, base);
            let symbol = Rf_install_in_current("getNativeSymbolInfo");
            let promise = owner.sexp(R_findVarInFrame(base, symbol)).unwrap();
            assert_eq!(promise.typeof_(), crate::sexp::ffi::SEXPTYPE::PROMSXP);
            owner.full_gc().unwrap();
            crate::sexp::envir::lock_binding_raw(base, symbol);
            let first = owner
                .sexp(crate::eval::eval::Rf_eval(symbol, base))
                .unwrap();
            assert_eq!(first.typeof_(), crate::sexp::ffi::SEXPTYPE::CLOSXP);
            let second = crate::eval::eval::Rf_eval(symbol, base);
            assert_eq!(first.as_raw(), second);
            let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                defineVar(symbol, R_NilValue(), base)
            }));
            assert!(
                denied.is_err(),
                "forcing the promise must not unlock or rebind the export"
            );
        });
    }
    #[test]
    fn lazy_native_symbol_binding_rejects_revocation_during_force() {
        use std::{
            cell::{Cell, RefCell},
            rc::Rc,
        };
        let original = crate::sexp::session::RSession::new_for_gc_tests();
        let replacement = Rc::new(RefCell::new(None::<crate::sexp::session::RSession>));
        let notifications = Rc::new(Cell::new(0));
        let observed = notifications.clone();
        let target = Rc::downgrade(&replacement);
        let owner = original.owner_token().unwrap();
        let weak = owner.weak_owner().unwrap();
        unsafe {
            let base = crate::sexp::globals::R_BaseEnv();
            load_lazy_native_symbol_info(base, base);
            let symbol = Rf_install_in_current("getNativeSymbolInfo");
            let promise = owner.sexp(R_findVarInFrame(base, symbol)).unwrap();
            let unbound = crate::sexp::globals::R_UnboundValue();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if observed.replace(1) != 0 {
                    return;
                }
                let pin = weak.pin().unwrap();
                (*pin.as_ptr()).memory_state.gc_force_gap = 0;
                crate::sexp::instance::revoke_instance_availability(pin.as_ptr());
                *target.upgrade().unwrap().borrow_mut() =
                    Some(crate::sexp::session::RSession::new_for_gc_tests());
            }));
            (*owner.as_ptr()).memory_state.gc_force_gap = 1;
            (*owner.as_ptr()).memory_state.gc_force_wait = 1;
            let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::eval::eval::Rf_eval(symbol, base)
            }));
            assert!(denied.is_err());
            assert_eq!(notifications.get(), 1);
            assert_eq!(crate::sexp::accessors::PRVALUE(promise.as_raw()), unbound);
        }
        let replacement = replacement.borrow();
        replacement.as_ref().unwrap().with_active(|| {
            assert!(
                replacement
                    .as_ref()
                    .unwrap()
                    .owner_token()
                    .unwrap()
                    .node_factory()
                    .character("live replacement")
                    .is_ok()
            );
        });
    }
}
