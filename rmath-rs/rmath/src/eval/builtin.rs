#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![deny(unsafe_op_in_unsafe_fn)]

//! Builtin/primitive compatibility entrypoints.
//!
//! The real primitive metadata lives in [`super::primitive`]. This module keeps
//! the historical names used by older translated code while delegating to the
//! Rust-shaped descriptor layer.

use std::os::raw::c_int;

use crate::mainutils::names::FunTabEntry;
use crate::sexp::accessors::SET_PRIMOFFSET;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::memory;

use super::primitive;
pub use super::primitive::{PRIMNAME, PrimFun};

/// Get the canonical R function table.
pub fn R_FunTab() -> *const FunTabEntry {
    crate::mainutils::names::R_FunTab.as_ptr()
}

/// Get the canonical R function table length.
pub fn R_FunTabSize() -> usize {
    primitive::fun_tab_len()
}

/// Get the function pointer for a primitive (SPECIAL or BUILTIN).
///
/// This is the equivalent of R's `PRIMFUN()` macro.
#[inline]
pub unsafe fn PRIMFUN(op: SEXP) -> Option<PrimFun> {
    unsafe { primitive::get_primfun(op) }
}

/// Initialize builtin slots.
///
/// Primitive SEXP nodes are created lazily through `R_Primitive` in this port,
/// so there is no process-global slot table to initialize here.
pub fn R_InitBuiltinSlots() {}

/// Create a SPECIALSXP or BUILTINSXP from a function table index.
pub unsafe fn R_mkPrim(_name: *const std::os::raw::c_char, offset: c_int, kind: c_int) -> SEXP {
    let sexptype = if kind == SEXPTYPE::SPECIALSXP.as_c_int() || kind == 0 {
        SEXPTYPE::SPECIALSXP
    } else {
        SEXPTYPE::BUILTINSXP
    };

    memory::with_arena(|arena| {
        let prim = arena.alloc_node(sexptype);
        if !prim.is_null() {
            unsafe { SET_PRIMOFFSET(prim, offset) };
        }
        prim
    })
}

/// Handler type for BUILTINSXP functions.
pub(super) type BuiltinHandler = unsafe fn(SEXP, SEXP, SEXP, SEXP) -> SEXP;

pub(super) type EvaluatedBuiltinHandler = BuiltinHandler;

#[derive(Clone, Copy)]
pub(super) struct UnevaluatedBuiltin {
    pub name: &'static str,
    pub handler: BuiltinHandler,
    pub restore_visibility_always: bool,
}

pub(super) fn unevaluated_builtin_handler(name: &str) -> Option<UnevaluatedBuiltin> {
    UNEVALUATED_BUILTINS
        .iter()
        .find(|builtin| builtin.name == name)
        .copied()
}

pub(super) const UNEVALUATED_BUILTINS: &[UnevaluatedBuiltin] = &[
    UnevaluatedBuiltin {
        name: "missing",
        handler: crate::eval::missing::do_missing,
        restore_visibility_always: true,
    },
    UnevaluatedBuiltin {
        name: "on.exit",
        handler: crate::mainutils::builtin::do_onexit,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "capture.output",
        handler: crate::mainutils::essentials::do_capture_output,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "suppressWarnings",
        handler: crate::mainutils::essentials::do_suppress_warnings,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "suppressMessages",
        handler: crate::mainutils::essentials::do_suppress_messages,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "tryCatch",
        handler: crate::mainutils::essentials::do_tryCatch,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "with",
        handler: crate::mainutils::essentials::do_with,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "apply",
        handler: crate::mainutils::essentials::do_apply,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "lapply",
        handler: crate::mainutils::essentials::do_lapply,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "sapply",
        handler: crate::mainutils::essentials::do_sapply,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "vapply",
        handler: crate::mainutils::essentials::do_vapply,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "Map",
        handler: crate::mainutils::essentials::do_map,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "Filter",
        handler: crate::mainutils::essentials::do_filter,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "do.call",
        handler: crate::mainutils::essentials::do_do_call,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "substitute",
        handler: crate::mainutils::coerce::do_substitute,
        restore_visibility_always: true,
    },
    UnevaluatedBuiltin {
        name: "quote",
        handler: crate::mainutils::essentials::do_quote,
        restore_visibility_always: true,
    },
];

#[derive(Clone, Copy)]
pub(super) struct EvaluatedBuiltin {
    pub name: &'static str,
    pub handler: EvaluatedBuiltinHandler,
}

/// Find the Rust implementation for an evaluated builtin name.
pub(super) fn evaluated_builtin_handler(name: &str) -> Option<EvaluatedBuiltinHandler> {
    EVALUATED_BUILTINS
        .iter()
        .find(|builtin| builtin.name == name)
        .map(|builtin| builtin.handler)
}

pub(super) const EVALUATED_BUILTINS: &[EvaluatedBuiltin] = &[
    EvaluatedBuiltin {
        name: "+",
        handler: super::arithmetic::do_arith,
    },
    EvaluatedBuiltin {
        name: "-",
        handler: super::arithmetic::do_arith,
    },
    EvaluatedBuiltin {
        name: "*",
        handler: super::arithmetic::do_arith,
    },
    EvaluatedBuiltin {
        name: "/",
        handler: super::arithmetic::do_arith,
    },
    EvaluatedBuiltin {
        name: "^",
        handler: super::arithmetic::do_arith,
    },
    EvaluatedBuiltin {
        name: "%%",
        handler: super::arithmetic::do_arith,
    },
    EvaluatedBuiltin {
        name: "%/%",
        handler: super::arithmetic::do_arith,
    },
    EvaluatedBuiltin {
        name: ":",
        handler: crate::mainutils::seq::do_colon,
    },
    EvaluatedBuiltin {
        name: "<",
        handler: super::arithmetic::do_relop,
    },
    EvaluatedBuiltin {
        name: ">",
        handler: super::arithmetic::do_relop,
    },
    EvaluatedBuiltin {
        name: "<=",
        handler: super::arithmetic::do_relop,
    },
    EvaluatedBuiltin {
        name: ">=",
        handler: super::arithmetic::do_relop,
    },
    EvaluatedBuiltin {
        name: "==",
        handler: super::arithmetic::do_relop,
    },
    EvaluatedBuiltin {
        name: "!=",
        handler: super::arithmetic::do_relop,
    },
    EvaluatedBuiltin {
        name: "abs",
        handler: crate::mainutils::essentials::do_abs,
    },
    EvaluatedBuiltin {
        name: "sign",
        handler: crate::mainutils::essentials::do_sign,
    },
    EvaluatedBuiltin {
        name: "sqrt",
        handler: super::arithmetic::do_math1,
    },
    EvaluatedBuiltin {
        name: "log",
        handler: super::arithmetic::do_math1,
    },
    EvaluatedBuiltin {
        name: "log10",
        handler: super::arithmetic::do_math1,
    },
    EvaluatedBuiltin {
        name: "exp",
        handler: super::arithmetic::do_math1,
    },
    EvaluatedBuiltin {
        name: "ceiling",
        handler: super::arithmetic::do_math1,
    },
    EvaluatedBuiltin {
        name: "floor",
        handler: super::arithmetic::do_math1,
    },
    EvaluatedBuiltin {
        name: "length",
        handler: super::arithmetic::do_length,
    },
    EvaluatedBuiltin {
        name: "sum",
        handler: super::arithmetic::do_summary,
    },
    EvaluatedBuiltin {
        name: "min",
        handler: super::arithmetic::do_summary,
    },
    EvaluatedBuiltin {
        name: "max",
        handler: super::arithmetic::do_summary,
    },
    EvaluatedBuiltin {
        name: "prod",
        handler: super::arithmetic::do_summary,
    },
    EvaluatedBuiltin {
        name: "range",
        handler: super::arithmetic::do_summary,
    },
    EvaluatedBuiltin {
        name: "mean",
        handler: super::arithmetic::do_mean,
    },
    EvaluatedBuiltin {
        name: "is.numeric",
        handler: super::arithmetic::do_is_type,
    },
    EvaluatedBuiltin {
        name: "is.integer",
        handler: super::arithmetic::do_is_type,
    },
    EvaluatedBuiltin {
        name: "is.double",
        handler: super::arithmetic::do_is_type,
    },
    EvaluatedBuiltin {
        name: "is.logical",
        handler: super::arithmetic::do_is_type,
    },
    EvaluatedBuiltin {
        name: "is.character",
        handler: super::arithmetic::do_is_type,
    },
    EvaluatedBuiltin {
        name: "is.null",
        handler: super::arithmetic::do_is_type,
    },
    EvaluatedBuiltin {
        name: "identical",
        handler: crate::mainutils::identical::do_identical,
    },
    EvaluatedBuiltin {
        name: "c",
        handler: crate::mainutils::essentials::do_c,
    },
    EvaluatedBuiltin {
        name: "seq",
        handler: crate::mainutils::essentials::do_seq,
    },
    EvaluatedBuiltin {
        name: "sequence",
        handler: crate::mainutils::essentials::do_sequence,
    },
    EvaluatedBuiltin {
        name: "rep",
        handler: crate::mainutils::essentials::do_rep,
    },
    EvaluatedBuiltin {
        name: "paste",
        handler: crate::mainutils::essentials::do_paste,
    },
    EvaluatedBuiltin {
        name: "paste0",
        handler: crate::mainutils::essentials::do_paste0,
    },
    EvaluatedBuiltin {
        name: "cat",
        handler: crate::mainutils::essentials::do_cat,
    },
    EvaluatedBuiltin {
        name: "print",
        handler: crate::mainutils::essentials::do_print,
    },
    EvaluatedBuiltin {
        name: "typeof",
        handler: crate::mainutils::essentials::do_typeof,
    },
    EvaluatedBuiltin {
        name: "is.na",
        handler: crate::mainutils::essentials::do_is_na,
    },
    EvaluatedBuiltin {
        name: "names",
        handler: crate::mainutils::essentials::do_names,
    },
    EvaluatedBuiltin {
        name: "which",
        handler: crate::mainutils::essentials::do_which,
    },
    EvaluatedBuiltin {
        name: "ifelse",
        handler: crate::mainutils::essentials::do_ifelse,
    },
    EvaluatedBuiltin {
        name: "table",
        handler: crate::mainutils::essentials::do_table,
    },
    EvaluatedBuiltin {
        name: "as.integer",
        handler: crate::mainutils::essentials::do_as_integer,
    },
    EvaluatedBuiltin {
        name: "as.double",
        handler: crate::mainutils::essentials::do_as_double,
    },
    EvaluatedBuiltin {
        name: "as.character",
        handler: crate::mainutils::essentials::do_as_character,
    },
    EvaluatedBuiltin {
        name: "as.logical",
        handler: crate::mainutils::essentials::do_as_logical,
    },
    EvaluatedBuiltin {
        name: "as.vector",
        handler: crate::mainutils::essentials::do_as_vector,
    },
    EvaluatedBuiltin {
        name: "as.list",
        handler: crate::mainutils::essentials::do_as_list,
    },
    EvaluatedBuiltin {
        name: "nchar",
        handler: crate::mainutils::essentials::do_nchar,
    },
    EvaluatedBuiltin {
        name: "substr",
        handler: crate::mainutils::essentials::do_substr,
    },
    EvaluatedBuiltin {
        name: "tolower",
        handler: crate::mainutils::essentials::do_tolower,
    },
    EvaluatedBuiltin {
        name: "toupper",
        handler: crate::mainutils::essentials::do_toupper,
    },
    EvaluatedBuiltin {
        name: "set.seed",
        handler: crate::mainutils::rng_dispatch::do_set_seed,
    },
    EvaluatedBuiltin {
        name: "RNGkind",
        handler: crate::mainutils::rng_dispatch::do_RNGkind,
    },
    EvaluatedBuiltin {
        name: "runif",
        handler: crate::mainutils::rng_dispatch::do_runif,
    },
    EvaluatedBuiltin {
        name: "rnorm",
        handler: crate::mainutils::rng_dispatch::do_rnorm,
    },
    EvaluatedBuiltin {
        name: "rpois",
        handler: crate::mainutils::rng_dispatch::do_rpois,
    },
    EvaluatedBuiltin {
        name: "rexp",
        handler: crate::mainutils::rng_dispatch::do_rexp,
    },
    EvaluatedBuiltin {
        name: "sample",
        handler: crate::mainutils::rng_dispatch::do_sample,
    },
    EvaluatedBuiltin {
        name: "apply",
        handler: crate::mainutils::essentials::do_apply,
    },
    EvaluatedBuiltin {
        name: "tapply",
        handler: crate::mainutils::essentials::do_tapply,
    },
    EvaluatedBuiltin {
        name: "mapply",
        handler: crate::mainutils::essentials::do_mapply,
    },
    EvaluatedBuiltin {
        name: "outer",
        handler: crate::mainutils::essentials::do_outer,
    },
    EvaluatedBuiltin {
        name: "sweep",
        handler: crate::mainutils::essentials::do_sweep,
    },
    EvaluatedBuiltin {
        name: "trimws",
        handler: crate::mainutils::essentials::do_trimws,
    },
    EvaluatedBuiltin {
        name: "sprintf",
        handler: crate::mainutils::essentials::do_sprintf,
    },
    EvaluatedBuiltin {
        name: "gsub",
        handler: crate::mainutils::essentials::do_gsub,
    },
    EvaluatedBuiltin {
        name: "sub",
        handler: crate::mainutils::essentials::do_sub,
    },
    EvaluatedBuiltin {
        name: "grep",
        handler: crate::mainutils::essentials::do_grep,
    },
    EvaluatedBuiltin {
        name: "grepl",
        handler: crate::mainutils::essentials::do_grepl,
    },
    EvaluatedBuiltin {
        name: "strsplit",
        handler: crate::mainutils::essentials::do_strsplit,
    },
    EvaluatedBuiltin {
        name: "pmin",
        handler: crate::mainutils::essentials::do_pmin,
    },
    EvaluatedBuiltin {
        name: "pmax",
        handler: crate::mainutils::essentials::do_pmax,
    },
    EvaluatedBuiltin {
        name: "which.min",
        handler: crate::mainutils::essentials::do_which_min,
    },
    EvaluatedBuiltin {
        name: "which.max",
        handler: crate::mainutils::essentials::do_which_max,
    },
    EvaluatedBuiltin {
        name: "append",
        handler: crate::mainutils::essentials::do_append,
    },
    EvaluatedBuiltin {
        name: "head",
        handler: crate::mainutils::essentials::do_head,
    },
    EvaluatedBuiltin {
        name: "tail",
        handler: crate::mainutils::essentials::do_tail,
    },
    EvaluatedBuiltin {
        name: "[",
        handler: crate::mainutils::subset::do_subset_dflt,
    },
    EvaluatedBuiltin {
        name: "[[",
        handler: crate::mainutils::subset::do_subset2_dflt,
    },
    EvaluatedBuiltin {
        name: "setdiff",
        handler: crate::mainutils::essentials::do_setdiff,
    },
    EvaluatedBuiltin {
        name: "union",
        handler: crate::mainutils::essentials::do_union,
    },
    EvaluatedBuiltin {
        name: "intersect",
        handler: crate::mainutils::essentials::do_intersect,
    },
    EvaluatedBuiltin {
        name: "setequal",
        handler: crate::mainutils::essentials::do_setequal,
    },
    EvaluatedBuiltin {
        name: "is.finite",
        handler: crate::mainutils::essentials::do_is_finite,
    },
    EvaluatedBuiltin {
        name: "is.infinite",
        handler: crate::mainutils::essentials::do_is_infinite,
    },
    EvaluatedBuiltin {
        name: "is.nan",
        handler: crate::mainutils::essentials::do_is_nan,
    },
    EvaluatedBuiltin {
        name: "is.matrix",
        handler: crate::mainutils::essentials::do_is_matrix,
    },
    EvaluatedBuiltin {
        name: "is.array",
        handler: crate::mainutils::essentials::do_is_array,
    },
    EvaluatedBuiltin {
        name: "is.list",
        handler: crate::mainutils::essentials::do_is_list,
    },
    EvaluatedBuiltin {
        name: "chartr",
        handler: crate::mainutils::essentials::do_chartr,
    },
    EvaluatedBuiltin {
        name: "format",
        handler: crate::mainutils::essentials::do_format,
    },
    EvaluatedBuiltin {
        name: "NROW",
        handler: crate::mainutils::essentials::do_NROW,
    },
    EvaluatedBuiltin {
        name: "NCOL",
        handler: crate::mainutils::essentials::do_NCOL,
    },
    EvaluatedBuiltin {
        name: "lengths",
        handler: crate::mainutils::essentials::do_lengths,
    },
    EvaluatedBuiltin {
        name: "rownames",
        handler: crate::mainutils::essentials::do_rownames,
    },
    EvaluatedBuiltin {
        name: "colnames",
        handler: crate::mainutils::essentials::do_colnames,
    },
    EvaluatedBuiltin {
        name: "class",
        handler: crate::mainutils::essentials::do_class_get,
    },
    EvaluatedBuiltin {
        name: "list",
        handler: crate::mainutils::essentials::do_list,
    },
    EvaluatedBuiltin {
        name: "data.frame",
        handler: crate::mainutils::essentials::do_data_frame,
    },
    EvaluatedBuiltin {
        name: "Names",
        handler: crate::mainutils::essentials::do_Names,
    },
    EvaluatedBuiltin {
        name: "attr",
        handler: crate::mainutils::essentials::do_attr,
    },
    EvaluatedBuiltin {
        name: "attributes",
        handler: crate::mainutils::essentials::do_attributes,
    },
    EvaluatedBuiltin {
        name: "structure",
        handler: crate::mainutils::essentials::do_structure,
    },
    EvaluatedBuiltin {
        name: "noquote",
        handler: crate::mainutils::essentials::do_noquote,
    },
    EvaluatedBuiltin {
        name: "deparse",
        handler: crate::mainutils::essentials::do_deparse,
    },
    EvaluatedBuiltin {
        name: "nargs",
        handler: crate::mainutils::essentials::do_nargs,
    },
    EvaluatedBuiltin {
        name: "UseMethod",
        handler: crate::mainutils::objects::do_usemethod,
    },
    EvaluatedBuiltin {
        name: "NextMethod",
        handler: crate::mainutils::objects::do_nextmethod,
    },
    EvaluatedBuiltin {
        name: "useMethod",
        handler: crate::mainutils::objects::do_usemethod,
    },
    EvaluatedBuiltin {
        name: "missing",
        handler: crate::mainutils::essentials::do_missing,
    },
    EvaluatedBuiltin {
        name: "parent.frame",
        handler: crate::mainutils::essentials::do_parent_frame,
    },
    EvaluatedBuiltin {
        name: "sys.call",
        handler: crate::mainutils::essentials::do_sys_call,
    },
    EvaluatedBuiltin {
        name: "sys.frame",
        handler: crate::mainutils::essentials::do_sys_frame,
    },
    EvaluatedBuiltin {
        name: "getwd",
        handler: crate::mainutils::essentials::do_getwd,
    },
    EvaluatedBuiltin {
        name: "setwd",
        handler: crate::mainutils::essentials::do_setwd,
    },
    EvaluatedBuiltin {
        name: "basename",
        handler: crate::mainutils::essentials::do_basename,
    },
    EvaluatedBuiltin {
        name: "dirname",
        handler: crate::mainutils::essentials::do_dirname,
    },
    EvaluatedBuiltin {
        name: "file.path",
        handler: crate::mainutils::essentials::do_file_path,
    },
    EvaluatedBuiltin {
        name: "dir.exists",
        handler: crate::mainutils::platform::do_direxists,
    },
    EvaluatedBuiltin {
        name: "file.create",
        handler: crate::mainutils::essentials::do_file_create,
    },
    EvaluatedBuiltin {
        name: "dir.create",
        handler: crate::mainutils::platform::do_dircreate,
    },
    EvaluatedBuiltin {
        name: "unlink",
        handler: crate::mainutils::essentials::do_unlink,
    },
    EvaluatedBuiltin {
        name: "nzchar",
        handler: crate::mainutils::essentials::do_nzchar,
    },
    EvaluatedBuiltin {
        name: "lapply",
        handler: crate::mainutils::essentials::do_lapply,
    },
    EvaluatedBuiltin {
        name: "sapply",
        handler: crate::mainutils::essentials::do_sapply,
    },
    EvaluatedBuiltin {
        name: "vapply",
        handler: crate::mainutils::essentials::do_vapply,
    },
    EvaluatedBuiltin {
        name: "Map",
        handler: crate::mainutils::essentials::do_map,
    },
    EvaluatedBuiltin {
        name: "Filter",
        handler: crate::mainutils::essentials::do_filter,
    },
    EvaluatedBuiltin {
        name: "do.call",
        handler: crate::mainutils::essentials::do_do_call,
    },
    EvaluatedBuiltin {
        name: "is.atomic",
        handler: crate::mainutils::essentials::do_is_atomic,
    },
    EvaluatedBuiltin {
        name: "is.recursive",
        handler: crate::mainutils::essentials::do_is_recursive,
    },
    EvaluatedBuiltin {
        name: "is.object",
        handler: crate::mainutils::essentials::do_is_object,
    },
    EvaluatedBuiltin {
        name: "file",
        handler: crate::mainutils::connections::do_file,
    },
    EvaluatedBuiltin {
        name: "url",
        handler: crate::mainutils::essentials::do_url,
    },
    EvaluatedBuiltin {
        name: "textConnection",
        handler: crate::mainutils::connections::do_textConnection,
    },
    EvaluatedBuiltin {
        name: "textConnectionValue",
        handler: crate::mainutils::connections::do_textConnectionValue,
    },
    EvaluatedBuiltin {
        name: "rawConnection",
        handler: crate::mainutils::connections::do_rawConnection,
    },
    EvaluatedBuiltin {
        name: "close",
        handler: crate::mainutils::essentials::do_close,
    },
    EvaluatedBuiltin {
        name: "flush",
        handler: crate::mainutils::essentials::do_flush,
    },
    EvaluatedBuiltin {
        name: "gzfile",
        handler: crate::mainutils::essentials::do_gzfile,
    },
    EvaluatedBuiltin {
        name: "pipe",
        handler: crate::mainutils::essentials::do_pipe,
    },
    EvaluatedBuiltin {
        name: "fifo",
        handler: crate::mainutils::essentials::do_fifo,
    },
    EvaluatedBuiltin {
        name: "socketConnection",
        handler: crate::mainutils::essentials::do_socketConnection,
    },
    EvaluatedBuiltin {
        name: "isOpen",
        handler: crate::mainutils::essentials::do_isOpen,
    },
    EvaluatedBuiltin {
        name: "isIncomplete",
        handler: crate::mainutils::essentials::do_isIncomplete,
    },
    EvaluatedBuiltin {
        name: "isSeekable",
        handler: crate::mainutils::essentials::do_isSeekable,
    },
    EvaluatedBuiltin {
        name: "seek",
        handler: crate::mainutils::essentials::do_seek,
    },
    EvaluatedBuiltin {
        name: "pushBack",
        handler: crate::mainutils::essentials::do_pushBack,
    },
    EvaluatedBuiltin {
        name: "pushBackClear",
        handler: crate::mainutils::essentials::do_pushBackClear,
    },
    EvaluatedBuiltin {
        name: "pushBackLength",
        handler: crate::mainutils::essentials::do_pushBackLength,
    },
    EvaluatedBuiltin {
        name: "readBin",
        handler: crate::mainutils::essentials::do_readBin,
    },
    EvaluatedBuiltin {
        name: "writeBin",
        handler: crate::mainutils::essentials::do_writeBin,
    },
    EvaluatedBuiltin {
        name: "print.matrix",
        handler: crate::mainutils::essentials::do_print_matrix,
    },
    EvaluatedBuiltin {
        name: "print.list",
        handler: crate::mainutils::essentials::do_print_list,
    },
    EvaluatedBuiltin {
        name: "summary",
        handler: crate::mainutils::essentials::do_summary_default,
    },
    EvaluatedBuiltin {
        name: "str",
        handler: crate::mainutils::essentials::do_str,
    },
    EvaluatedBuiltin {
        name: "as.data.frame",
        handler: crate::mainutils::essentials::do_as_data_frame,
    },
    EvaluatedBuiltin {
        name: "c.list",
        handler: crate::mainutils::essentials::do_c_list,
    },
    EvaluatedBuiltin {
        name: "unlist",
        handler: crate::mainutils::essentials::do_unlist,
    },
    EvaluatedBuiltin {
        name: "list.get",
        handler: crate::mainutils::essentials::do_list_get,
    },
    EvaluatedBuiltin {
        name: "list.set",
        handler: crate::mainutils::essentials::do_list_set,
    },
    EvaluatedBuiltin {
        name: "print.default",
        handler: crate::mainutils::essentials::do_print_default,
    },
    EvaluatedBuiltin {
        name: "print.data.frame",
        handler: crate::mainutils::essentials::do_print_data_frame,
    },
    EvaluatedBuiltin {
        name: "print.table",
        handler: crate::mainutils::essentials::do_print_table,
    },
    EvaluatedBuiltin {
        name: "print.factor",
        handler: crate::mainutils::essentials::do_print_factor,
    },
    EvaluatedBuiltin {
        name: "summary.data.frame",
        handler: crate::mainutils::essentials::do_summary_data_frame,
    },
    EvaluatedBuiltin {
        name: "format.data.frame",
        handler: crate::mainutils::essentials::do_format_data_frame,
    },
    EvaluatedBuiltin {
        name: "crossprod",
        handler: crate::mainutils::essentials::do_crossprod,
    },
    EvaluatedBuiltin {
        name: "tcrossprod",
        handler: crate::mainutils::essentials::do_tcrossprod,
    },
    EvaluatedBuiltin {
        name: "det",
        handler: crate::mainutils::essentials::do_det,
    },
    EvaluatedBuiltin {
        name: "solve",
        handler: crate::mainutils::essentials::do_solve,
    },
    EvaluatedBuiltin {
        name: "emptyenv",
        handler: crate::mainutils::essentials::do_emptyenv,
    },
    EvaluatedBuiltin {
        name: "baseenv",
        handler: crate::mainutils::essentials::do_baseenv,
    },
    EvaluatedBuiltin {
        name: "globalenv",
        handler: crate::mainutils::essentials::do_globalenv,
    },
    EvaluatedBuiltin {
        name: "new.env",
        handler: crate::mainutils::essentials::do_new_env,
    },
    EvaluatedBuiltin {
        name: "environment",
        handler: crate::mainutils::essentials::do_environment,
    },
    EvaluatedBuiltin {
        name: "lockBinding",
        handler: crate::mainutils::essentials::do_lockBinding,
    },
    EvaluatedBuiltin {
        name: "unlockBinding",
        handler: crate::mainutils::essentials::do_unlockBinding,
    },
    EvaluatedBuiltin {
        name: "bindingIsLocked",
        handler: crate::mainutils::essentials::do_bindingIsLocked,
    },
    EvaluatedBuiltin {
        name: "makeActiveBinding",
        handler: crate::mainutils::essentials::do_makeActiveBinding,
    },
    EvaluatedBuiltin {
        name: "lockEnvironment",
        handler: crate::mainutils::essentials::do_lockEnvironment,
    },
    EvaluatedBuiltin {
        name: "environmentIsLocked",
        handler: crate::mainutils::essentials::do_environmentIsLocked,
    },
    EvaluatedBuiltin {
        name: "version",
        handler: crate::mainutils::essentials::do_version,
    },
    EvaluatedBuiltin {
        name: "R.version",
        handler: crate::mainutils::essentials::do_R_version,
    },
    EvaluatedBuiltin {
        name: "args",
        handler: crate::mainutils::essentials::do_args,
    },
    EvaluatedBuiltin {
        name: "formals",
        handler: crate::mainutils::essentials::do_formals,
    },
    EvaluatedBuiltin {
        name: "body",
        handler: crate::mainutils::essentials::do_body,
    },
    EvaluatedBuiltin {
        name: "charmatch",
        handler: crate::mainutils::essentials::do_charmatch,
    },
    EvaluatedBuiltin {
        name: "pmatch",
        handler: crate::mainutils::essentials::do_pmatch,
    },
    EvaluatedBuiltin {
        name: "strtoi",
        handler: crate::mainutils::essentials::do_strtoi,
    },
    EvaluatedBuiltin {
        name: "strtrim",
        handler: crate::mainutils::essentials::do_strtrim,
    },
    EvaluatedBuiltin {
        name: "round",
        handler: crate::mainutils::essentials::do_round,
    },
    EvaluatedBuiltin {
        name: "signif",
        handler: crate::mainutils::essentials::do_signif,
    },
    EvaluatedBuiltin {
        name: "trunc",
        handler: crate::mainutils::essentials::do_trunc,
    },
    EvaluatedBuiltin {
        name: "log2",
        handler: crate::mainutils::essentials::do_log2,
    },
    EvaluatedBuiltin {
        name: "eval",
        handler: crate::mainutils::essentials::do_eval,
    },
    EvaluatedBuiltin {
        name: "parse",
        handler: crate::mainutils::essentials::do_parse,
    },
    EvaluatedBuiltin {
        name: "conditionMessage",
        handler: crate::mainutils::essentials::do_conditionMessage,
    },
    EvaluatedBuiltin {
        name: "conditionCall",
        handler: crate::mainutils::essentials::do_conditionCall,
    },
    EvaluatedBuiltin {
        name: "simpleError",
        handler: crate::mainutils::essentials::do_simpleError,
    },
    EvaluatedBuiltin {
        name: "simpleWarning",
        handler: crate::mainutils::essentials::do_simpleWarning,
    },
    EvaluatedBuiltin {
        name: "withRestarts",
        handler: crate::mainutils::essentials::do_withRestarts,
    },
    EvaluatedBuiltin {
        name: "isS4",
        handler: crate::mainutils::essentials::do_isS4,
    },
    EvaluatedBuiltin {
        name: "is",
        handler: crate::mainutils::essentials::do_is,
    },
    EvaluatedBuiltin {
        name: "S3_class",
        handler: crate::mainutils::essentials::do_S3_class,
    },
    EvaluatedBuiltin {
        name: "setClass",
        handler: crate::mainutils::essentials::do_setClass,
    },
    EvaluatedBuiltin {
        name: "setValidity",
        handler: crate::mainutils::essentials::do_setValidity,
    },
    EvaluatedBuiltin {
        name: "isVirtualClass",
        handler: crate::mainutils::essentials::do_isVirtualClass,
    },
    EvaluatedBuiltin {
        name: "new",
        handler: crate::mainutils::essentials::do_new,
    },
    EvaluatedBuiltin {
        name: "show",
        handler: crate::mainutils::essentials::do_show,
    },
    EvaluatedBuiltin {
        name: "slotNames",
        handler: crate::mainutils::essentials::do_slotNames,
    },
    EvaluatedBuiltin {
        name: "slot",
        handler: crate::mainutils::essentials::do_slot,
    },
    EvaluatedBuiltin {
        name: "set_slot",
        handler: crate::mainutils::essentials::do_set_slot,
    },
    EvaluatedBuiltin {
        name: "extends",
        handler: crate::mainutils::essentials::do_extends,
    },
    EvaluatedBuiltin {
        name: "isSealedClass",
        handler: crate::mainutils::essentials::do_isSealedClass,
    },
    EvaluatedBuiltin {
        name: "sealClass",
        handler: crate::mainutils::essentials::do_sealClass,
    },
    EvaluatedBuiltin {
        name: "representation",
        handler: crate::mainutils::essentials::do_representation,
    },
    EvaluatedBuiltin {
        name: "containsClass",
        handler: crate::mainutils::essentials::do_containsClass,
    },
    EvaluatedBuiltin {
        name: "possibleExtends",
        handler: crate::mainutils::essentials::do_possibleExtends,
    },
    EvaluatedBuiltin {
        name: "setReplaceMethod",
        handler: crate::mainutils::essentials::do_setReplaceMethod,
    },
    EvaluatedBuiltin {
        name: "getMethod",
        handler: crate::mainutils::essentials::do_getMethod,
    },
    EvaluatedBuiltin {
        name: "removeGeneric",
        handler: crate::mainutils::essentials::do_removeGeneric,
    },
    EvaluatedBuiltin {
        name: "removeMethod",
        handler: crate::mainutils::essentials::do_removeMethod,
    },
    EvaluatedBuiltin {
        name: "isGeneric",
        handler: crate::mainutils::essentials::do_isGeneric,
    },
    EvaluatedBuiltin {
        name: "isMethod",
        handler: crate::mainutils::essentials::do_isMethod,
    },
    EvaluatedBuiltin {
        name: "findMethod",
        handler: crate::mainutils::essentials::do_findMethod,
    },
    EvaluatedBuiltin {
        name: "findMethods",
        handler: crate::mainutils::essentials::do_findMethods,
    },
    EvaluatedBuiltin {
        name: "showMethods",
        handler: crate::mainutils::essentials::do_showMethods,
    },
    EvaluatedBuiltin {
        name: "getGenerics",
        handler: crate::mainutils::essentials::do_getGenerics,
    },
    EvaluatedBuiltin {
        name: "getMethods",
        handler: crate::mainutils::essentials::do_getMethods,
    },
    EvaluatedBuiltin {
        name: "existsMethod",
        handler: crate::mainutils::essentials::do_existsMethod,
    },
    EvaluatedBuiltin {
        name: "hasMethod",
        handler: crate::mainutils::essentials::do_hasMethod,
    },
    EvaluatedBuiltin {
        name: "selectMethod",
        handler: crate::mainutils::essentials::do_selectMethod,
    },
    EvaluatedBuiltin {
        name: "scan",
        handler: crate::mainutils::essentials::do_scan,
    },
    EvaluatedBuiltin {
        name: "write.table",
        handler: crate::mainutils::essentials::do_write_table,
    },
    EvaluatedBuiltin {
        name: "readLines",
        handler: crate::mainutils::connections::do_readLines,
    },
    EvaluatedBuiltin {
        name: "writeLines",
        handler: crate::mainutils::essentials::do_writeLines,
    },
    EvaluatedBuiltin {
        name: "sink",
        handler: crate::mainutils::essentials::do_sink,
    },
    EvaluatedBuiltin {
        name: "order",
        handler: crate::mainutils::essentials::do_order,
    },
    EvaluatedBuiltin {
        name: "rank",
        handler: crate::mainutils::essentials::do_rank,
    },
    EvaluatedBuiltin {
        name: "duplicated",
        handler: crate::mainutils::essentials::do_duplicated,
    },
    EvaluatedBuiltin {
        name: "anyDuplicated",
        handler: crate::mainutils::essentials::do_anyDuplicated,
    },
    EvaluatedBuiltin {
        name: "duplicated.array",
        handler: crate::mainutils::essentials::do_duplicated_array,
    },
    EvaluatedBuiltin {
        name: "anyDuplicated.array",
        handler: crate::mainutils::essentials::do_anyDuplicated_array,
    },
    EvaluatedBuiltin {
        name: "match",
        handler: crate::mainutils::essentials::do_match,
    },
    EvaluatedBuiltin {
        name: "findInterval",
        handler: crate::mainutils::essentials::do_findInterval,
    },
    EvaluatedBuiltin {
        name: "cut",
        handler: crate::mainutils::essentials::do_cut,
    },
    EvaluatedBuiltin {
        name: "startsWith",
        handler: crate::mainutils::essentials::do_startsWith,
    },
    EvaluatedBuiltin {
        name: "endsWith",
        handler: crate::mainutils::essentials::do_endsWith,
    },
    EvaluatedBuiltin {
        name: "str_pad",
        handler: crate::mainutils::essentials::do_str_pad,
    },
    EvaluatedBuiltin {
        name: "str_count",
        handler: crate::mainutils::essentials::do_str_count,
    },
    EvaluatedBuiltin {
        name: "str_replace",
        handler: crate::mainutils::essentials::do_str_replace,
    },
    EvaluatedBuiltin {
        name: "is.language",
        handler: crate::mainutils::essentials::do_is_language,
    },
    EvaluatedBuiltin {
        name: "is.call",
        handler: crate::mainutils::essentials::do_is_call,
    },
    EvaluatedBuiltin {
        name: "is.symbol",
        handler: crate::mainutils::essentials::do_is_symbol,
    },
    EvaluatedBuiltin {
        name: "is.name",
        handler: crate::mainutils::essentials::do_is_name,
    },
    EvaluatedBuiltin {
        name: "is.pairlist",
        handler: crate::mainutils::essentials::do_is_pairlist,
    },
    EvaluatedBuiltin {
        name: "is.function",
        handler: crate::mainutils::essentials::do_is_function,
    },
    EvaluatedBuiltin {
        name: "is.expression",
        handler: crate::mainutils::essentials::do_is_expression,
    },
    EvaluatedBuiltin {
        name: "is.environment",
        handler: crate::mainutils::essentials::do_is_environment,
    },
    EvaluatedBuiltin {
        name: "setOldClass",
        handler: crate::mainutils::essentials::do_setOldClass,
    },
    EvaluatedBuiltin {
        name: "methods",
        handler: crate::mainutils::essentials::do_methods,
    },
    EvaluatedBuiltin {
        name: "lower.tri",
        handler: crate::mainutils::essentials::do_lower_tri,
    },
    EvaluatedBuiltin {
        name: "upper.tri",
        handler: crate::mainutils::essentials::do_upper_tri,
    },
    EvaluatedBuiltin {
        name: "colSums",
        handler: crate::mainutils::essentials::do_colSums,
    },
    EvaluatedBuiltin {
        name: "rowSums",
        handler: crate::mainutils::essentials::do_rowSums,
    },
    EvaluatedBuiltin {
        name: "colMeans",
        handler: crate::mainutils::essentials::do_colMeans,
    },
    EvaluatedBuiltin {
        name: "rowMeans",
        handler: crate::mainutils::essentials::do_rowMeans,
    },
    EvaluatedBuiltin {
        name: "col",
        handler: crate::mainutils::essentials::do_col,
    },
    EvaluatedBuiltin {
        name: "row",
        handler: crate::mainutils::essentials::do_row,
    },
    EvaluatedBuiltin {
        name: "cov",
        handler: crate::mainutils::essentials::do_cov,
    },
    EvaluatedBuiltin {
        name: "cor",
        handler: crate::mainutils::essentials::do_cor,
    },
    EvaluatedBuiltin {
        name: "scale",
        handler: crate::mainutils::essentials::do_scale,
    },
    EvaluatedBuiltin {
        name: "rle",
        handler: crate::mainutils::essentials::do_rle,
    },
    EvaluatedBuiltin {
        name: "inverse.rle",
        handler: crate::mainutils::essentials::do_inverse_rle,
    },
    EvaluatedBuiltin {
        name: "which_array",
        handler: crate::mainutils::essentials::do_which_array,
    },
    EvaluatedBuiltin {
        name: "commandArgs",
        handler: crate::mainutils::essentials::do_commandArgs,
    },
    EvaluatedBuiltin {
        name: "getOption",
        handler: crate::mainutils::essentials::do_getOption,
    },
    EvaluatedBuiltin {
        name: "options",
        handler: crate::mainutils::essentials::do_options,
    },
    EvaluatedBuiltin {
        name: "interactive",
        handler: crate::mainutils::essentials::do_interactive,
    },
    EvaluatedBuiltin {
        name: "is_interactive",
        handler: crate::mainutils::essentials::do_is_interactive,
    },
    EvaluatedBuiltin {
        name: "getRversion",
        handler: crate::mainutils::essentials::do_getRversion,
    },
    EvaluatedBuiltin {
        name: "R.version.string",
        handler: crate::mainutils::essentials::do_R_version_string,
    },
    EvaluatedBuiltin {
        name: "R.Version",
        handler: crate::mainutils::essentials::do_R_Version,
    },
    EvaluatedBuiltin {
        name: "list.append",
        handler: crate::mainutils::essentials::do_list_append,
    },
    EvaluatedBuiltin {
        name: "list.prepend",
        handler: crate::mainutils::essentials::do_list_prepend,
    },
    EvaluatedBuiltin {
        name: "compact",
        handler: crate::mainutils::essentials::do_compact,
    },
    EvaluatedBuiltin {
        name: "keep",
        handler: crate::mainutils::essentials::do_keep,
    },
    EvaluatedBuiltin {
        name: "discard",
        handler: crate::mainutils::essentials::do_discard,
    },
    EvaluatedBuiltin {
        name: "str_detect",
        handler: crate::mainutils::essentials::do_str_detect,
    },
    EvaluatedBuiltin {
        name: "str_extract",
        handler: crate::mainutils::essentials::do_str_extract,
    },
    EvaluatedBuiltin {
        name: "reshape",
        handler: crate::mainutils::essentials::do_reshape,
    },
    EvaluatedBuiltin {
        name: "complete.cases",
        handler: crate::mainutils::essentials::do_complete_cases,
    },
    EvaluatedBuiltin {
        name: "na.omit",
        handler: crate::mainutils::essentials::do_na_omit,
    },
    EvaluatedBuiltin {
        name: "na.exclude",
        handler: crate::mainutils::essentials::do_na_exclude,
    },
    EvaluatedBuiltin {
        name: "is_complete",
        handler: crate::mainutils::essentials::do_is_complete,
    },
    EvaluatedBuiltin {
        name: "str_interp",
        handler: crate::mainutils::essentials::do_str_interp,
    },
    EvaluatedBuiltin {
        name: "str_wrap",
        handler: crate::mainutils::essentials::do_str_wrap,
    },
    EvaluatedBuiltin {
        name: "path_package",
        handler: crate::mainutils::essentials::do_path_package,
    },
    EvaluatedBuiltin {
        name: "system.file",
        handler: crate::mainutils::essentials::do_system_file,
    },
    EvaluatedBuiltin {
        name: "ls_args",
        handler: crate::mainutils::essentials::do_ls_args,
    },
    EvaluatedBuiltin {
        name: "deparse1",
        handler: crate::mainutils::essentials::do_deparse1,
    },
    EvaluatedBuiltin {
        name: "dput",
        handler: crate::mainutils::essentials::do_dput,
    },
    EvaluatedBuiltin {
        name: "dget",
        handler: crate::mainutils::essentials::do_dget,
    },
    EvaluatedBuiltin {
        name: "bquote",
        handler: crate::mainutils::essentials::do_bquote,
    },
    EvaluatedBuiltin {
        name: "rownames_to_column",
        handler: crate::mainutils::essentials::do_rownames_to_column,
    },
    EvaluatedBuiltin {
        name: "column_to_rownames",
        handler: crate::mainutils::essentials::do_column_to_rownames,
    },
    EvaluatedBuiltin {
        name: "relocate",
        handler: crate::mainutils::essentials::do_relocate,
    },
    EvaluatedBuiltin {
        name: "cat_args",
        handler: crate::mainutils::essentials::do_cat_args,
    },
    EvaluatedBuiltin {
        name: "message_args",
        handler: crate::mainutils::essentials::do_message_args,
    },
    EvaluatedBuiltin {
        name: "packageStartupMessage",
        handler: crate::mainutils::essentials::do_package_startup_message,
    },
    EvaluatedBuiltin {
        name: "parent.env",
        handler: crate::mainutils::essentials::do_parent_env,
    },
    EvaluatedBuiltin {
        name: "set_parent.env",
        handler: crate::mainutils::essentials::do_set_parent_env,
    },
    EvaluatedBuiltin {
        name: "env_name",
        handler: crate::mainutils::essentials::do_env_name,
    },
    EvaluatedBuiltin {
        name: "environmentName",
        handler: crate::mainutils::essentials::do_environment_name,
    },
    EvaluatedBuiltin {
        name: "is_empty",
        handler: crate::mainutils::essentials::do_is_empty,
    },
    EvaluatedBuiltin {
        name: "print.integer",
        handler: crate::mainutils::essentials::do_print_integer,
    },
    EvaluatedBuiltin {
        name: "print.numeric",
        handler: crate::mainutils::essentials::do_print_numeric,
    },
    EvaluatedBuiltin {
        name: "print.logical",
        handler: crate::mainutils::essentials::do_print_logical,
    },
    EvaluatedBuiltin {
        name: "print.character",
        handler: crate::mainutils::essentials::do_print_character,
    },
    EvaluatedBuiltin {
        name: "print.complex",
        handler: crate::mainutils::essentials::do_print_complex,
    },
    EvaluatedBuiltin {
        name: "print.function",
        handler: crate::mainutils::essentials::do_print_function,
    },
    EvaluatedBuiltin {
        name: "print.environment",
        handler: crate::mainutils::essentials::do_print_environment,
    },
    EvaluatedBuiltin {
        name: "print.formula",
        handler: crate::mainutils::essentials::do_print_formula,
    },
    EvaluatedBuiltin {
        name: "print.call",
        handler: crate::mainutils::essentials::do_print_call,
    },
    EvaluatedBuiltin {
        name: "print.pairlist",
        handler: crate::mainutils::essentials::do_print_pairlist,
    },
    EvaluatedBuiltin {
        name: "print.raw",
        handler: crate::mainutils::essentials::do_print_raw,
    },
    EvaluatedBuiltin {
        name: "summary.numeric",
        handler: crate::mainutils::essentials::do_summary_numeric,
    },
    EvaluatedBuiltin {
        name: "summary.integer",
        handler: crate::mainutils::essentials::do_summary_integer,
    },
    EvaluatedBuiltin {
        name: "summary.logical",
        handler: crate::mainutils::essentials::do_summary_logical,
    },
    EvaluatedBuiltin {
        name: "summary.character",
        handler: crate::mainutils::essentials::do_summary_character,
    },
    EvaluatedBuiltin {
        name: "is.single",
        handler: crate::mainutils::essentials::do_is_single,
    },
    EvaluatedBuiltin {
        name: "is.vector",
        handler: crate::mainutils::essentials::do_is_vector,
    },
    EvaluatedBuiltin {
        name: "is.scalar",
        handler: crate::mainutils::essentials::do_is_scalar,
    },
    EvaluatedBuiltin {
        name: "is.named",
        handler: crate::mainutils::essentials::do_is_named,
    },
    EvaluatedBuiltin {
        name: "is.unsorted",
        handler: crate::mainutils::essentials::do_is_unsorted,
    },
    EvaluatedBuiltin {
        name: "is.loaded",
        handler: crate::mainutils::essentials::do_is_loaded,
    },
    EvaluatedBuiltin {
        name: "is.primitive",
        handler: crate::mainutils::essentials::do_is_primitive,
    },
    EvaluatedBuiltin {
        name: "is.generic",
        handler: crate::mainutils::essentials::do_is_generic,
    },
    EvaluatedBuiltin {
        name: "is.data.frame",
        handler: crate::mainutils::essentials::do_is_data_frame,
    },
    EvaluatedBuiltin {
        name: "as.complex",
        handler: crate::mainutils::essentials::do_as_complex,
    },
    EvaluatedBuiltin {
        name: "as.raw",
        handler: crate::mainutils::essentials::do_as_raw,
    },
    EvaluatedBuiltin {
        name: "as",
        handler: crate::mainutils::essentials::do_as,
    },
    EvaluatedBuiltin {
        name: "capture.output",
        handler: crate::mainutils::essentials::do_capture_output,
    },
    EvaluatedBuiltin {
        name: "withVisible",
        handler: crate::mainutils::essentials::do_with_visible,
    },
    EvaluatedBuiltin {
        name: "invisible",
        handler: crate::mainutils::essentials::do_invisible,
    },
    EvaluatedBuiltin {
        name: "suppressWarnings",
        handler: crate::mainutils::essentials::do_suppress_warnings,
    },
    EvaluatedBuiltin {
        name: "suppressMessages",
        handler: crate::mainutils::essentials::do_suppress_messages,
    },
    EvaluatedBuiltin {
        name: "force",
        handler: crate::mainutils::essentials::do_force,
    },
    EvaluatedBuiltin {
        name: "isTRUE",
        handler: crate::mainutils::essentials::do_is_true,
    },
    EvaluatedBuiltin {
        name: "isFALSE",
        handler: crate::mainutils::essentials::do_is_false,
    },
    EvaluatedBuiltin {
        name: "anyNA",
        handler: crate::mainutils::essentials::do_any_na,
    },
    EvaluatedBuiltin {
        name: "allNA",
        handler: crate::mainutils::essentials::do_all_na,
    },
    EvaluatedBuiltin {
        name: "anyNaN",
        handler: crate::mainutils::essentials::do_any_nan,
    },
    EvaluatedBuiltin {
        name: "allNaN",
        handler: crate::mainutils::essentials::do_all_nan,
    },
    EvaluatedBuiltin {
        name: "modifyList",
        handler: crate::mainutils::essentials::do_modify_list,
    },
    EvaluatedBuiltin {
        name: "splice",
        handler: crate::mainutils::essentials::do_splice,
    },
    EvaluatedBuiltin {
        name: "flatten",
        handler: crate::mainutils::essentials::do_flatten,
    },
    EvaluatedBuiltin {
        name: "split",
        handler: crate::mainutils::essentials::do_split,
    },
    EvaluatedBuiltin {
        name: "melt",
        handler: crate::mainutils::essentials::do_melt,
    },
    EvaluatedBuiltin {
        name: "cast",
        handler: crate::mainutils::essentials::do_cast,
    },
    EvaluatedBuiltin {
        name: "with",
        handler: crate::mainutils::essentials::do_with,
    },
    EvaluatedBuiltin {
        name: "within",
        handler: crate::mainutils::essentials::do_within,
    },
    EvaluatedBuiltin {
        name: "transform",
        handler: crate::mainutils::essentials::do_transform,
    },
    EvaluatedBuiltin {
        name: "prop.table",
        handler: crate::mainutils::essentials::do_prop_table,
    },
    EvaluatedBuiltin {
        name: "addmargins",
        handler: crate::mainutils::essentials::do_addmargins,
    },
    EvaluatedBuiltin {
        name: "ftable",
        handler: crate::mainutils::essentials::do_ftable,
    },
    EvaluatedBuiltin {
        name: "xtabs",
        handler: crate::mainutils::essentials::do_xtabs,
    },
    EvaluatedBuiltin {
        name: "aggregate",
        handler: crate::mainutils::essentials::do_aggregate,
    },
    EvaluatedBuiltin {
        name: "ave",
        handler: crate::mainutils::essentials::do_ave,
    },
    EvaluatedBuiltin {
        name: "by",
        handler: crate::mainutils::essentials::do_by,
    },
    EvaluatedBuiltin {
        name: "interaction",
        handler: crate::mainutils::essentials::do_interaction,
    },
    EvaluatedBuiltin {
        name: "relevel",
        handler: crate::mainutils::essentials::do_relevel,
    },
    EvaluatedBuiltin {
        name: "factor",
        handler: crate::mainutils::essentials::do_factor,
    },
    EvaluatedBuiltin {
        name: "is.factor",
        handler: crate::mainutils::essentials::do_is_factor,
    },
    EvaluatedBuiltin {
        name: "is.ordered",
        handler: crate::mainutils::essentials::do_is_ordered,
    },
    EvaluatedBuiltin {
        name: "levels",
        handler: crate::mainutils::essentials::do_levels,
    },
    EvaluatedBuiltin {
        name: "nlevels",
        handler: crate::mainutils::essentials::do_nlevels,
    },
    EvaluatedBuiltin {
        name: "str_locate",
        handler: crate::mainutils::essentials::do_str_locate,
    },
    EvaluatedBuiltin {
        name: "str_locate_all",
        handler: crate::mainutils::essentials::do_str_locate_all,
    },
    EvaluatedBuiltin {
        name: "str_sub",
        handler: crate::mainutils::essentials::do_str_sub,
    },
    EvaluatedBuiltin {
        name: "str_sub_all",
        handler: crate::mainutils::essentials::do_str_sub_all,
    },
    EvaluatedBuiltin {
        name: "R.home",
        handler: crate::mainutils::essentials::do_R_home,
    },
    EvaluatedBuiltin {
        name: "Sys.getenv",
        handler: crate::mainutils::essentials::do_Sys_getenv,
    },
    EvaluatedBuiltin {
        name: "Sys.setenv",
        handler: crate::mainutils::essentials::do_Sys_setenv,
    },
    EvaluatedBuiltin {
        name: "Sys.unsetenv",
        handler: crate::mainutils::essentials::do_Sys_unsetenv,
    },
    EvaluatedBuiltin {
        name: "Sys.time",
        handler: crate::mainutils::essentials::do_Sys_time,
    },
    EvaluatedBuiltin {
        name: "Sys.sleep",
        handler: crate::mainutils::essentials::do_Sys_sleep,
    },
    EvaluatedBuiltin {
        name: "Sys.Date",
        handler: crate::mainutils::essentials::do_Sys_Date,
    },
    EvaluatedBuiltin {
        name: "Sys.timezone",
        handler: crate::mainutils::essentials::do_Sys_timezone,
    },
    EvaluatedBuiltin {
        name: "Sys.localeconv",
        handler: crate::mainutils::essentials::do_Sys_localeconv,
    },
    EvaluatedBuiltin {
        name: "Sys.getlocale",
        handler: crate::mainutils::essentials::do_Sys_getlocale,
    },
    EvaluatedBuiltin {
        name: "Sys.setlocale",
        handler: crate::mainutils::essentials::do_Sys_setlocale,
    },
    EvaluatedBuiltin {
        name: "subset",
        handler: crate::mainutils::essentials::do_subset_named,
    },
    EvaluatedBuiltin {
        name: "cat_enhanced",
        handler: crate::mainutils::essentials::do_cat_enhanced,
    },
    EvaluatedBuiltin {
        name: "message_enhanced",
        handler: crate::mainutils::essentials::do_message_enhanced,
    },
    EvaluatedBuiltin {
        name: "warning_enhanced",
        handler: crate::mainutils::essentials::do_warning_enhanced,
    },
    EvaluatedBuiltin {
        name: "match.call",
        handler: crate::mainutils::essentials::do_match_call,
    },
    EvaluatedBuiltin {
        name: "sys.nframe",
        handler: crate::mainutils::essentials::do_sys_nframe,
    },
    EvaluatedBuiltin {
        name: "sys.function",
        handler: crate::mainutils::essentials::do_sys_function,
    },
    EvaluatedBuiltin {
        name: "read.csv",
        handler: crate::mainutils::essentials::do_read_csv,
    },
    EvaluatedBuiltin {
        name: "write.csv",
        handler: crate::mainutils::essentials::do_write_csv,
    },
    EvaluatedBuiltin {
        name: "read.table",
        handler: crate::mainutils::essentials::do_read_table,
    },
    EvaluatedBuiltin {
        name: "as.matrix",
        handler: crate::mainutils::essentials::do_as_matrix,
    },
    EvaluatedBuiltin {
        name: "as.numeric",
        handler: crate::mainutils::essentials::do_as_numeric,
    },
    EvaluatedBuiltin {
        name: "par",
        handler: crate::mainutils::essentials::do_par,
    },
    EvaluatedBuiltin {
        name: "getGraphicsEvent",
        handler: crate::mainutils::essentials::do_getGraphicsEvent,
    },
    EvaluatedBuiltin {
        name: "Rprof",
        handler: crate::mainutils::essentials::do_Rprof,
    },
    EvaluatedBuiltin {
        name: "Rprofmem",
        handler: crate::mainutils::essentials::do_Rprofmem,
    },
    EvaluatedBuiltin {
        name: "gc",
        handler: crate::mainutils::essentials::do_gc,
    },
    EvaluatedBuiltin {
        name: "gcinfo",
        handler: crate::mainutils::essentials::do_gcinfo,
    },
    EvaluatedBuiltin {
        name: "memory.size",
        handler: crate::mainutils::essentials::do_memory_size,
    },
    EvaluatedBuiltin {
        name: "object.size",
        handler: crate::mainutils::essentials::do_object_size,
    },
    EvaluatedBuiltin {
        name: "read.csv2",
        handler: crate::mainutils::essentials::do_read_csv2,
    },
    EvaluatedBuiltin {
        name: "write.csv2",
        handler: crate::mainutils::essentials::do_write_csv2,
    },
    EvaluatedBuiltin {
        name: "read.delim",
        handler: crate::mainutils::essentials::do_read_delim,
    },
    EvaluatedBuiltin {
        name: "read.fwf",
        handler: crate::mainutils::essentials::do_read_fwf,
    },
    EvaluatedBuiltin {
        name: "readChar",
        handler: crate::mainutils::essentials::do_readChar,
    },
    EvaluatedBuiltin {
        name: "writeChar",
        handler: crate::mainutils::essentials::do_writeChar,
    },
    EvaluatedBuiltin {
        name: "getS3method",
        handler: crate::mainutils::essentials::do_getS3method,
    },
    EvaluatedBuiltin {
        name: "hasS3method",
        handler: crate::mainutils::essentials::do_hasS3method,
    },
    EvaluatedBuiltin {
        name: "registerS3method",
        handler: crate::mainutils::essentials::do_registerS3method,
    },
    EvaluatedBuiltin {
        name: "setGeneric",
        handler: crate::mainutils::essentials::do_setGeneric,
    },
    EvaluatedBuiltin {
        name: "setMethod",
        handler: crate::mainutils::essentials::do_setMethod,
    },
    EvaluatedBuiltin {
        name: "Random.seed",
        handler: crate::mainutils::essentials::do_Random_seed,
    },
    EvaluatedBuiltin {
        name: "loadRDS",
        handler: crate::mainutils::essentials::do_loadRDS,
    },
    EvaluatedBuiltin {
        name: "saveRDS",
        handler: crate::mainutils::essentials::do_saveRDS,
    },
    EvaluatedBuiltin {
        name: "mclapply",
        handler: crate::mainutils::essentials::do_mclapply,
    },
    EvaluatedBuiltin {
        name: "future_lapply",
        handler: crate::mainutils::essentials::do_future_lapply,
    },
    EvaluatedBuiltin {
        name: "foreach",
        handler: crate::mainutils::essentials::do_foreach,
    },
    EvaluatedBuiltin {
        name: "withCallingHandlers",
        handler: crate::mainutils::essentials::do_withCallingHandlers,
    },
    EvaluatedBuiltin {
        name: "computeRestarts",
        handler: crate::mainutils::essentials::do_computeRestarts,
    },
    EvaluatedBuiltin {
        name: "findRestart",
        handler: crate::mainutils::essentials::do_findRestart,
    },
    EvaluatedBuiltin {
        name: "restarts",
        handler: crate::mainutils::essentials::do_restarts,
    },
    EvaluatedBuiltin {
        name: ".libPaths",
        handler: crate::mainutils::essentials::do_lib_paths,
    },
    EvaluatedBuiltin {
        name: "library",
        handler: crate::mainutils::essentials::do_library,
    },
    EvaluatedBuiltin {
        name: "require",
        handler: crate::mainutils::essentials::do_require,
    },
    EvaluatedBuiltin {
        name: "installed.packages",
        handler: crate::mainutils::essentials::do_installed_packages,
    },
    EvaluatedBuiltin {
        name: "find.package",
        handler: crate::mainutils::essentials::do_find_package,
    },
    EvaluatedBuiltin {
        name: "data",
        handler: crate::mainutils::essentials::do_data,
    },
    EvaluatedBuiltin {
        name: "detach",
        handler: crate::mainutils::envir::do_detach,
    },
    EvaluatedBuiltin {
        name: "search",
        handler: crate::mainutils::envir::do_search,
    },
    EvaluatedBuiltin {
        name: "source",
        handler: crate::mainutils::essentials::do_source,
    },
    EvaluatedBuiltin {
        name: "sys.source",
        handler: crate::mainutils::essentials::do_sys_source,
    },
    EvaluatedBuiltin {
        name: "demo",
        handler: crate::mainutils::essentials::do_demo,
    },
    EvaluatedBuiltin {
        name: "example",
        handler: crate::mainutils::essentials::do_example,
    },
    EvaluatedBuiltin {
        name: "dlnorm",
        handler: crate::mainutils::essentials::do_dlnorm,
    },
    EvaluatedBuiltin {
        name: "plnorm",
        handler: crate::mainutils::essentials::do_plnorm,
    },
    EvaluatedBuiltin {
        name: "qlnorm",
        handler: crate::mainutils::essentials::do_qlnorm,
    },
    EvaluatedBuiltin {
        name: "dlogis",
        handler: crate::mainutils::essentials::do_dlogis,
    },
    EvaluatedBuiltin {
        name: "plogis",
        handler: crate::mainutils::essentials::do_plogis,
    },
    EvaluatedBuiltin {
        name: "qlogis",
        handler: crate::mainutils::essentials::do_qlogis,
    },
    EvaluatedBuiltin {
        name: "dsignrank",
        handler: crate::mainutils::essentials::do_dsignrank,
    },
    EvaluatedBuiltin {
        name: "psignrank",
        handler: crate::mainutils::essentials::do_psignrank,
    },
    EvaluatedBuiltin {
        name: "qsignrank",
        handler: crate::mainutils::essentials::do_qsignrank,
    },
    EvaluatedBuiltin {
        name: "dwilcox",
        handler: crate::mainutils::essentials::do_dwilcox,
    },
    EvaluatedBuiltin {
        name: "pwilcox",
        handler: crate::mainutils::essentials::do_pwilcox,
    },
    EvaluatedBuiltin {
        name: "qwilcox",
        handler: crate::mainutils::essentials::do_qwilcox,
    },
    EvaluatedBuiltin {
        name: "dhyper",
        handler: crate::mainutils::essentials::do_dhyper,
    },
    EvaluatedBuiltin {
        name: "phyper",
        handler: crate::mainutils::essentials::do_phyper,
    },
    EvaluatedBuiltin {
        name: "qhyper",
        handler: crate::mainutils::essentials::do_qhyper,
    },
    EvaluatedBuiltin {
        name: "ptukey",
        handler: crate::mainutils::essentials::do_ptukey,
    },
    EvaluatedBuiltin {
        name: "qtukey",
        handler: crate::mainutils::essentials::do_qtukey,
    },
    EvaluatedBuiltin {
        name: "dmultinom",
        handler: crate::mainutils::essentials::do_dmultinom,
    },
    EvaluatedBuiltin {
        name: "cbind",
        handler: crate::mainutils::essentials::do_cbind,
    },
    EvaluatedBuiltin {
        name: "rbind",
        handler: crate::mainutils::essentials::do_rbind,
    },
    EvaluatedBuiltin {
        name: "t",
        handler: crate::mainutils::essentials::do_transpose,
    },
    EvaluatedBuiltin {
        name: "var",
        handler: crate::mainutils::essentials::do_var,
    },
    EvaluatedBuiltin {
        name: "sd",
        handler: crate::mainutils::essentials::do_sd,
    },
    EvaluatedBuiltin {
        name: "median",
        handler: crate::mainutils::essentials::do_median,
    },
    EvaluatedBuiltin {
        name: "cummin",
        handler: crate::mainutils::essentials::do_cummin,
    },
    EvaluatedBuiltin {
        name: "cummax",
        handler: crate::mainutils::essentials::do_cummax,
    },
    EvaluatedBuiltin {
        name: "dimnames",
        handler: crate::mainutils::essentials::do_dimnames,
    },
    EvaluatedBuiltin {
        name: "Re",
        handler: crate::mainutils::complex_cmath::do_cmathfuns,
    },
    EvaluatedBuiltin {
        name: "Im",
        handler: crate::mainutils::complex_cmath::do_cmathfuns,
    },
    EvaluatedBuiltin {
        name: "Mod",
        handler: crate::mainutils::complex_cmath::do_cmathfuns,
    },
    EvaluatedBuiltin {
        name: "Arg",
        handler: crate::mainutils::complex_cmath::do_cmathfuns,
    },
    EvaluatedBuiltin {
        name: "Conj",
        handler: crate::mainutils::complex_cmath::do_cmathfuns,
    },
    EvaluatedBuiltin {
        name: "pi",
        handler: crate::mainutils::essentials::do_pi,
    },
    EvaluatedBuiltin {
        name: "sin",
        handler: crate::mainutils::essentials::do_sin,
    },
    EvaluatedBuiltin {
        name: "cos",
        handler: crate::mainutils::essentials::do_cos,
    },
    EvaluatedBuiltin {
        name: "tan",
        handler: crate::mainutils::essentials::do_tan,
    },
    EvaluatedBuiltin {
        name: "asin",
        handler: crate::mainutils::essentials::do_asin,
    },
    EvaluatedBuiltin {
        name: "acos",
        handler: crate::mainutils::essentials::do_acos,
    },
    EvaluatedBuiltin {
        name: "atan",
        handler: crate::mainutils::essentials::do_atan,
    },
    EvaluatedBuiltin {
        name: "atan2",
        handler: crate::mainutils::essentials::do_atan2,
    },
    EvaluatedBuiltin {
        name: "lgamma",
        handler: crate::mainutils::essentials::do_lgamma,
    },
    EvaluatedBuiltin {
        name: "gamma",
        handler: crate::mainutils::essentials::do_gamma,
    },
    EvaluatedBuiltin {
        name: "digamma",
        handler: crate::mainutils::essentials::do_digamma,
    },
    EvaluatedBuiltin {
        name: "trigamma",
        handler: crate::mainutils::essentials::do_trigamma,
    },
    EvaluatedBuiltin {
        name: "psigamma",
        handler: crate::mainutils::essentials::do_psigamma,
    },
    EvaluatedBuiltin {
        name: "beta",
        handler: crate::mainutils::essentials::do_beta,
    },
    EvaluatedBuiltin {
        name: "lbeta",
        handler: crate::mainutils::essentials::do_lbeta,
    },
    EvaluatedBuiltin {
        name: "choose",
        handler: crate::mainutils::essentials::do_choose,
    },
    EvaluatedBuiltin {
        name: "lchoose",
        handler: crate::mainutils::essentials::do_lchoose,
    },
    EvaluatedBuiltin {
        name: "factorial",
        handler: crate::mainutils::essentials::do_factorial,
    },
    EvaluatedBuiltin {
        name: "lfactorial",
        handler: crate::mainutils::essentials::do_lfactorial,
    },
    EvaluatedBuiltin {
        name: "besselI",
        handler: crate::mainutils::essentials::do_besselI,
    },
    EvaluatedBuiltin {
        name: "besselJ",
        handler: crate::mainutils::essentials::do_besselJ,
    },
    EvaluatedBuiltin {
        name: "besselK",
        handler: crate::mainutils::essentials::do_besselK,
    },
    EvaluatedBuiltin {
        name: "besselY",
        handler: crate::mainutils::essentials::do_besselY,
    },
    EvaluatedBuiltin {
        name: "simplify2array",
        handler: crate::mainutils::essentials::do_simplify2array,
    },
    EvaluatedBuiltin {
        name: "match.arg",
        handler: crate::mainutils::essentials::do_match_arg,
    },
    EvaluatedBuiltin {
        name: "char.expand",
        handler: crate::mainutils::essentials::do_char_expand,
    },
    EvaluatedBuiltin {
        name: "type.convert",
        handler: crate::mainutils::essentials::do_type_convert,
    },
    EvaluatedBuiltin {
        name: "as.environment",
        handler: crate::mainutils::essentials::do_as_environment,
    },
    EvaluatedBuiltin {
        name: "sort.list",
        handler: crate::mainutils::essentials::do_sort_list,
    },
    EvaluatedBuiltin {
        name: "match.fun",
        handler: crate::mainutils::essentials::do_match_fun,
    },
    EvaluatedBuiltin {
        name: "any",
        handler: crate::mainutils::essentials::do_any,
    },
    EvaluatedBuiltin {
        name: "all",
        handler: crate::mainutils::essentials::do_all,
    },
    EvaluatedBuiltin {
        name: "cumsum",
        handler: crate::mainutils::essentials::do_cumsum,
    },
    EvaluatedBuiltin {
        name: "cumprod",
        handler: crate::mainutils::essentials::do_cumprod,
    },
    EvaluatedBuiltin {
        name: "seq_len",
        handler: crate::mainutils::essentials::do_seq_len,
    },
    EvaluatedBuiltin {
        name: "seq_along",
        handler: crate::mainutils::essentials::do_seq_along,
    },
    EvaluatedBuiltin {
        name: "diff",
        handler: crate::mainutils::essentials::do_diff,
    },
    EvaluatedBuiltin {
        name: "sort",
        handler: crate::mainutils::essentials::do_sort,
    },
    EvaluatedBuiltin {
        name: "rev",
        handler: crate::mainutils::essentials::do_rev,
    },
    EvaluatedBuiltin {
        name: "unique",
        handler: crate::mainutils::essentials::do_unique,
    },
    EvaluatedBuiltin {
        name: "matrix",
        handler: crate::mainutils::essentials::do_matrix,
    },
    EvaluatedBuiltin {
        name: "diag",
        handler: crate::mainutils::essentials::do_diag,
    },
    EvaluatedBuiltin {
        name: "dim",
        handler: crate::mainutils::essentials::do_dim,
    },
    EvaluatedBuiltin {
        name: "nrow",
        handler: crate::mainutils::essentials::do_nrow,
    },
    EvaluatedBuiltin {
        name: "ncol",
        handler: crate::mainutils::essentials::do_ncol,
    },
    EvaluatedBuiltin {
        name: "setNames",
        handler: crate::mainutils::essentials::do_setNames,
    },
    EvaluatedBuiltin {
        name: "names<-",
        handler: crate::mainutils::essentials::do_names_set,
    },
    EvaluatedBuiltin {
        name: "dimnames<-",
        handler: crate::mainutils::essentials::do_dimnames_set,
    },
    EvaluatedBuiltin {
        name: "rownames<-",
        handler: crate::mainutils::essentials::do_rownames_set,
    },
    EvaluatedBuiltin {
        name: "colnames<-",
        handler: crate::mainutils::essentials::do_colnames_set,
    },
    EvaluatedBuiltin {
        name: "exists",
        handler: crate::mainutils::essentials::do_exists,
    },
    EvaluatedBuiltin {
        name: "get",
        handler: crate::mainutils::essentials::do_get,
    },
    EvaluatedBuiltin {
        name: "assign",
        handler: crate::mainutils::essentials::do_assign,
    },
    EvaluatedBuiltin {
        name: "ls",
        handler: crate::mainutils::essentials::do_ls,
    },
    EvaluatedBuiltin {
        name: "rm",
        handler: crate::mainutils::essentials::do_rm,
    },
    EvaluatedBuiltin {
        name: "%in%",
        handler: crate::mainutils::essentials::do_in_operator,
    },
    EvaluatedBuiltin {
        name: "inherits",
        handler: crate::mainutils::objects::do_inherits,
    },
    EvaluatedBuiltin {
        name: "setattr",
        handler: crate::mainutils::essentials::do_setattr,
    },
    EvaluatedBuiltin {
        name: "stop",
        handler: crate::mainutils::essentials::do_stop,
    },
    EvaluatedBuiltin {
        name: "stopifnot",
        handler: crate::mainutils::essentials::do_stopifnot,
    },
    EvaluatedBuiltin {
        name: "warning",
        handler: crate::mainutils::essentials::do_warning,
    },
    EvaluatedBuiltin {
        name: "message",
        handler: crate::mainutils::essentials::do_message,
    },
    EvaluatedBuiltin {
        name: "tryCatch",
        handler: crate::mainutils::essentials::do_tryCatch,
    },
    EvaluatedBuiltin {
        name: "system",
        handler: crate::mainutils::essentials::do_system,
    },
    EvaluatedBuiltin {
        name: "tempdir",
        handler: crate::mainutils::essentials::do_tempdir,
    },
    EvaluatedBuiltin {
        name: "tempfile",
        handler: crate::mainutils::essentials::do_tempfile,
    },
    EvaluatedBuiltin {
        name: "file.exists",
        handler: crate::mainutils::platform::do_fileexists,
    },
    EvaluatedBuiltin {
        name: "list.files",
        handler: crate::mainutils::platform::do_listfiles,
    },
    EvaluatedBuiltin {
        name: "normalizePath",
        handler: crate::mainutils::essentials::do_normalizePath,
    },
    EvaluatedBuiltin {
        name: "rawToChar",
        handler: crate::mainutils::essentials::do_rawToChar,
    },
    EvaluatedBuiltin {
        name: "charToRaw",
        handler: crate::mainutils::essentials::do_charToRaw,
    },
    EvaluatedBuiltin {
        name: "toString",
        handler: crate::mainutils::essentials::do_toString,
    },
    EvaluatedBuiltin {
        name: "regexpr",
        handler: crate::mainutils::essentials::do_regexpr,
    },
    EvaluatedBuiltin {
        name: "sample.int",
        handler: crate::mainutils::essentials::do_sample_int,
    },
    EvaluatedBuiltin {
        name: "proc.time",
        handler: crate::mainutils::essentials::do_proc_time,
    },
    EvaluatedBuiltin {
        name: "as.list.generic",
        handler: crate::mainutils::essentials::do_as_list_generic,
    },
    EvaluatedBuiltin {
        name: "class<-",
        handler: crate::mainutils::essentials::do_class_set,
    },
    EvaluatedBuiltin {
        name: "dnorm",
        handler: crate::mainutils::essentials::do_dnorm,
    },
    EvaluatedBuiltin {
        name: "pnorm",
        handler: crate::mainutils::essentials::do_pnorm,
    },
    EvaluatedBuiltin {
        name: "qnorm",
        handler: crate::mainutils::essentials::do_qnorm,
    },
    EvaluatedBuiltin {
        name: "dpois",
        handler: crate::mainutils::essentials::do_dpois,
    },
    EvaluatedBuiltin {
        name: "ppois",
        handler: crate::mainutils::essentials::do_ppois,
    },
    EvaluatedBuiltin {
        name: "dbinom",
        handler: crate::mainutils::essentials::do_dbinom,
    },
    EvaluatedBuiltin {
        name: "pbinom",
        handler: crate::mainutils::essentials::do_pbinom,
    },
    EvaluatedBuiltin {
        name: "dexp",
        handler: crate::mainutils::essentials::do_dexp,
    },
    EvaluatedBuiltin {
        name: "pexp",
        handler: crate::mainutils::essentials::do_pexp,
    },
    EvaluatedBuiltin {
        name: "dgamma",
        handler: crate::mainutils::essentials::do_dgamma,
    },
    EvaluatedBuiltin {
        name: "pgamma",
        handler: crate::mainutils::essentials::do_pgamma,
    },
    EvaluatedBuiltin {
        name: "qgamma",
        handler: crate::mainutils::essentials::do_qgamma,
    },
    EvaluatedBuiltin {
        name: "dbeta",
        handler: crate::mainutils::essentials::do_dbeta,
    },
    EvaluatedBuiltin {
        name: "pbeta",
        handler: crate::mainutils::essentials::do_pbeta,
    },
    EvaluatedBuiltin {
        name: "qbeta",
        handler: crate::mainutils::essentials::do_qbeta,
    },
    EvaluatedBuiltin {
        name: "dt",
        handler: crate::mainutils::essentials::do_dt,
    },
    EvaluatedBuiltin {
        name: "pt",
        handler: crate::mainutils::essentials::do_pt,
    },
    EvaluatedBuiltin {
        name: "qt",
        handler: crate::mainutils::essentials::do_qt,
    },
    EvaluatedBuiltin {
        name: "dchisq",
        handler: crate::mainutils::essentials::do_dchisq,
    },
    EvaluatedBuiltin {
        name: "pchisq",
        handler: crate::mainutils::essentials::do_pchisq,
    },
    EvaluatedBuiltin {
        name: "qchisq",
        handler: crate::mainutils::essentials::do_qchisq,
    },
    EvaluatedBuiltin {
        name: "dcauchy",
        handler: crate::mainutils::essentials::do_dcauchy,
    },
    EvaluatedBuiltin {
        name: "pcauchy",
        handler: crate::mainutils::essentials::do_pcauchy,
    },
    EvaluatedBuiltin {
        name: "qcauchy",
        handler: crate::mainutils::essentials::do_qcauchy,
    },
    EvaluatedBuiltin {
        name: "dweibull",
        handler: crate::mainutils::essentials::do_dweibull,
    },
    EvaluatedBuiltin {
        name: "pweibull",
        handler: crate::mainutils::essentials::do_pweibull,
    },
    EvaluatedBuiltin {
        name: "qweibull",
        handler: crate::mainutils::essentials::do_qweibull,
    },
    EvaluatedBuiltin {
        name: "df",
        handler: crate::mainutils::essentials::do_df,
    },
    EvaluatedBuiltin {
        name: "pf",
        handler: crate::mainutils::essentials::do_pf,
    },
    EvaluatedBuiltin {
        name: "qf",
        handler: crate::mainutils::essentials::do_qf,
    },
    EvaluatedBuiltin {
        name: "dnbinom",
        handler: crate::mainutils::essentials::do_dnbinom,
    },
    EvaluatedBuiltin {
        name: "pnbinom",
        handler: crate::mainutils::essentials::do_pnbinom,
    },
    EvaluatedBuiltin {
        name: "qnbinom",
        handler: crate::mainutils::essentials::do_qnbinom,
    },
    EvaluatedBuiltin {
        name: "dgeom",
        handler: crate::mainutils::essentials::do_dgeom,
    },
    EvaluatedBuiltin {
        name: "pgeom",
        handler: crate::mainutils::essentials::do_pgeom,
    },
    EvaluatedBuiltin {
        name: "qgeom",
        handler: crate::mainutils::essentials::do_qgeom,
    },
];

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::*;
    use crate::sexp::session::RSession;

    #[test]
    fn fun_tab_points_to_canonical_table() {
        assert!(!R_FunTab().is_null());
        assert!(R_FunTabSize() > 100);
    }

    #[test]
    fn primfun_null() {
        unsafe {
            assert!(PRIMFUN(ptr::null_mut()).is_none());
        }
    }

    #[test]
    fn primname_uses_canonical_table() {
        let _session = RSession::new();
        let primitive = unsafe { crate::mainutils::names::R_Primitive(c"+".as_ptr()) };
        assert_eq!(unsafe { PRIMNAME(primitive) }, "+");
    }

    #[test]
    fn evaluated_builtin_table_has_unique_names() {
        let mut seen = std::collections::HashSet::new();
        for builtin in EVALUATED_BUILTINS {
            assert!(
                seen.insert(builtin.name),
                "duplicate evaluated builtin handler for {}",
                builtin.name
            );
        }
    }

    #[test]
    fn unevaluated_builtin_table_has_unique_names() {
        let mut seen = std::collections::HashSet::new();
        for builtin in UNEVALUATED_BUILTINS {
            assert!(
                seen.insert(builtin.name),
                "duplicate unevaluated builtin handler for {}",
                builtin.name
            );
        }
    }

    #[test]
    fn evaluated_builtin_lookup_covers_core_families() {
        for name in ["+", "log", "require", "subset", "dnorm", "qgeom"] {
            assert!(
                evaluated_builtin_handler(name).is_some(),
                "missing evaluated builtin handler for {name}"
            );
        }
        assert!(evaluated_builtin_handler("if").is_none());
    }

    #[test]
    fn unevaluated_builtin_lookup_covers_delayed_argument_forms() {
        let missing = unevaluated_builtin_handler("missing").expect("missing handler");
        assert!(missing.restore_visibility_always);

        for name in ["capture.output", "tryCatch", "with", "lapply", "do.call"] {
            assert!(
                unevaluated_builtin_handler(name).is_some(),
                "missing unevaluated builtin handler for {name}"
            );
        }
        assert!(unevaluated_builtin_handler("+").is_none());
    }
}
