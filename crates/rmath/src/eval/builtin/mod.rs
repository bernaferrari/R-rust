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

    unsafe {
        /* SAFETY: internal caller retains the live owner and its roots across this scoped operation. */
        memory::with_arena(|arena| {
            let prim = arena.alloc_node(sexptype);
            if !prim.is_null() {
                unsafe { SET_PRIMOFFSET(prim, offset) };
            }
            prim
        })
    }
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

pub(crate) fn builtin_primitive_kind(name: &str) -> Option<SEXPTYPE> {
    if is_hidden_builtin_name(name) {
        return None;
    }
    if unevaluated_builtin_handler(name).is_some() {
        Some(SEXPTYPE::SPECIALSXP)
    } else if EVALUATED_BUILTINS.iter().any(|b| b.name == name) {
        Some(SEXPTYPE::BUILTINSXP)
    } else {
        None
    }
}

/// Return whether the Rust evaluator has a builtin implementation for `name`.
///
/// Builtins are dispatched directly by the evaluator rather than being eagerly
/// materialized into every session environment, so reflective helpers such as
/// `exists()` need a shared view of this table.
pub(crate) fn has_builtin_handler(name: &str) -> bool {
    if is_hidden_builtin_name(name) {
        return false;
    }
    EVALUATED_BUILTINS
        .iter()
        .any(|builtin| builtin.name == name)
        || UNEVALUATED_BUILTINS
            .iter()
            .any(|builtin| builtin.name == name)
}

/// Names of builtins implemented directly by the Rust evaluator.
pub(crate) fn builtin_handler_names() -> impl Iterator<Item = &'static str> {
    EVALUATED_BUILTINS
        .iter()
        .map(|builtin| builtin.name)
        .chain(UNEVALUATED_BUILTINS.iter().map(|builtin| builtin.name))
        .filter(|name| !is_hidden_builtin_name(name))
}

pub(crate) fn is_hidden_builtin_name(name: &str) -> bool {
    matches!(
        name,
        "print.integer"
            | "print.numeric"
            | "print.logical"
            | "print.character"
            | "print.complex"
            | "print.environment"
            | "print.formula"
            | "print.call"
            | "print.pairlist"
            | "print.raw"
            | "summary.numeric"
            | "summary.integer"
            | "summary.logical"
            | "summary.character"
            // GNU: NextMethod is a .Internal closure, not a primitive.
            | "NextMethod"
            // GNU sweep.R is a closure over aperm/array; not a primitive.
            | "sweep"
            // GNU stats:::predict.lm is a closure; loess uses predict.loess.
            | "predict.lm"
    )
}

pub(super) const UNEVALUATED_BUILTINS: &[UnevaluatedBuiltin] = &[
    UnevaluatedBuiltin {
        name: "stopifnot",
        handler: crate::mainutils::essentials::do_stopifnot,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "callNextMethod",
        handler: crate::library::methods::methods_list_dispatch::do_callNextMethod,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "rep",
        handler: crate::mainutils::seq::do_rep,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "pretty.default",
        handler: crate::mainutils::pretty::do_pretty_default,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "pretty",
        handler: crate::mainutils::pretty::do_pretty,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "hist",
        handler: crate::mainutils::graphics_highlevel::do_hist,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "hist.default",
        handler: crate::mainutils::graphics_highlevel::do_hist_default,
        restore_visibility_always: false,
    },
    // hist.Date / hist.POSIXt / hist.POSIXlt / hist.POSIXct are intentionally
    // NOT builtins: the authenticated graphics namespace image already owns
    // the GNU closures, and S3 dispatch must resolve them there. A base-env
    // builtin copy shadows that environment and breaks calls the GNU bodies
    // make into their own namespace (axis.POSIXct, axis.Date, ...).
    UnevaluatedBuiltin {
        name: "barplot",
        handler: crate::mainutils::graphics_highlevel::do_barplot,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "barplot.default",
        handler: crate::mainutils::graphics_highlevel::do_barplot_default,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "boxplot",
        handler: crate::mainutils::graphics_highlevel::do_boxplot,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "boxplot.default",
        handler: crate::mainutils::graphics_highlevel::do_boxplot_default,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "unit",
        handler: crate::mainutils::portable_grid::do_unit,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "unit.c",
        handler: crate::mainutils::portable_grid::do_unit_c,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "Ops.unit",
        handler: crate::mainutils::portable_grid::do_ops_unit,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "Summary.unit",
        handler: crate::mainutils::portable_grid::do_summary_unit,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "is.unit",
        handler: crate::mainutils::portable_grid::do_is_unit,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "length.unit",
        handler: crate::mainutils::portable_grid::do_length_unit,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "gpar",
        handler: crate::mainutils::portable_grid::do_gpar,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "viewport",
        handler: crate::mainutils::portable_grid::do_viewport,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "vpPath",
        handler: crate::mainutils::portable_grid::do_vp_path,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "grid.layout",
        handler: crate::mainutils::portable_grid::do_grid_layout,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "pushViewport",
        handler: crate::mainutils::portable_grid::do_push_viewport,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "popViewport",
        handler: crate::mainutils::portable_grid::do_pop_viewport,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "upViewport",
        handler: crate::mainutils::portable_grid::do_up_viewport,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "downViewport",
        handler: crate::mainutils::portable_grid::do_down_viewport,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "seekViewport",
        handler: crate::mainutils::portable_grid::do_seek_viewport,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "grid.newpage",
        handler: crate::mainutils::portable_grid::do_grid_newpage,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "convertX",
        handler: crate::mainutils::portable_grid::do_convert_x,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "convertY",
        handler: crate::mainutils::portable_grid::do_convert_y,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "convertWidth",
        handler: crate::mainutils::portable_grid::do_convert_width,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "convertHeight",
        handler: crate::mainutils::portable_grid::do_convert_height,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "grid.draw",
        handler: crate::mainutils::portable_grid::do_grid_draw,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "gList",
        handler: crate::mainutils::portable_grid::do_glist,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "gTree",
        handler: crate::mainutils::portable_grid::do_gtree,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "grobTree",
        handler: crate::mainutils::portable_grid::do_grobtree,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "gPath",
        handler: crate::mainutils::portable_grid::do_gpath,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "getGrob",
        handler: crate::mainutils::portable_grid::do_get_grob,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "editGrob",
        handler: crate::mainutils::portable_grid::do_edit_grob,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "is.grob",
        handler: crate::mainutils::portable_grid::do_is_grob,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "grobWidth",
        handler: crate::mainutils::portable_grid::do_grob_width,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "grobHeight",
        handler: crate::mainutils::portable_grid::do_grob_height,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "rectGrob",
        handler: crate::mainutils::portable_grid::do_rect_grob,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "grid.rect",
        handler: crate::mainutils::portable_grid::do_grid_rect,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "circleGrob",
        handler: crate::mainutils::portable_grid::do_circle_grob,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "grid.circle",
        handler: crate::mainutils::portable_grid::do_grid_circle,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "linesGrob",
        handler: crate::mainutils::portable_grid::do_lines_grob,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "grid.lines",
        handler: crate::mainutils::portable_grid::do_grid_lines,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "polygonGrob",
        handler: crate::mainutils::portable_grid::do_polygon_grob,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "grid.polygon",
        handler: crate::mainutils::portable_grid::do_grid_polygon,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "segmentsGrob",
        handler: crate::mainutils::portable_grid::do_segments_grob,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "grid.segments",
        handler: crate::mainutils::portable_grid::do_grid_segments,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "textGrob",
        handler: crate::mainutils::portable_grid::do_text_grob,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "grid.text",
        handler: crate::mainutils::portable_grid::do_grid_text,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "pointsGrob",
        handler: crate::mainutils::portable_grid::do_points_grob,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "grid.points",
        handler: crate::mainutils::portable_grid::do_grid_points,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "rasterImage",
        handler: crate::mainutils::graphics_raster::do_raster_image,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "recordPlot",
        handler: crate::mainutils::graphics_recording::do_record,
        restore_visibility_always: false,
    },
    #[cfg(feature = "renderplot-device")]
    UnevaluatedBuiltin {
        name: "replayPlot",
        handler: crate::mainutils::graphics_recording::do_replay,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "missing",
        handler: crate::eval::missing::do_missing,
        restore_visibility_always: true,
    },
    UnevaluatedBuiltin {
        name: "forceAndCall",
        handler: crate::eval::missing::do_forceAndCall,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "declare",
        handler: crate::eval::jit::do_declare,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "Exec",
        handler: crate::eval::jit::do_tailcall,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "Tailcall",
        handler: crate::eval::jit::do_tailcall,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "@",
        handler: crate::mainutils::essentials::do_at,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "$<-",
        handler: crate::mainutils::essentials::do_dollar_set,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "@<-",
        handler: crate::mainutils::essentials::do_at_set,
        restore_visibility_always: false,
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
        name: "withAutoprint",
        handler: crate::mainutils::essentials::do_with_autoprint,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "example",
        handler: crate::mainutils::essentials::do_example,
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
        name: "withCallingHandlers",
        handler: crate::mainutils::essentials::do_withCallingHandlers,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "withRestarts",
        handler: crate::mainutils::essentials::do_withRestarts,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "with",
        handler: crate::mainutils::essentials::do_with,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "transform",
        handler: crate::mainutils::essentials::do_transform,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "assertError",
        handler: crate::mainutils::essentials::do_assertError,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "assertWarning",
        handler: crate::mainutils::essentials::do_assertWarning,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "assertCondition",
        handler: crate::mainutils::essentials::do_assertCondition,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "save",
        handler: crate::mainutils::saveload::do_save_user,
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
        name: "replicate",
        handler: crate::mainutils::essentials::do_replicate,
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
        name: "loess",
        handler: crate::mainutils::loess::do_loess,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "loess.control",
        handler: crate::mainutils::loess::do_control,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "predict",
        handler: crate::mainutils::loess::do_predict,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "predict.loess",
        handler: crate::mainutils::loess::do_predict_loess,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "lines",
        handler: crate::mainutils::essentials::do_lines,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "points",
        handler: crate::mainutils::essentials::do_points,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "text",
        handler: crate::mainutils::essentials::do_text,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "plot",
        handler: crate::mainutils::essentials::do_plot,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "preplot",
        handler: crate::mainutils::essentials::do_preplot,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "profile",
        handler: crate::mainutils::essentials::do_profile,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "tsdiag",
        handler: crate::mainutils::essentials::do_tsdiag,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "biplot",
        handler: crate::mainutils::essentials::do_biplot,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "screeplot",
        handler: crate::mainutils::essentials::do_screeplot,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "free1way",
        handler: crate::mainutils::essentials::do_free1way,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "monthplot",
        handler: crate::mainutils::essentials::do_monthplot,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "Filter",
        handler: crate::mainutils::essentials::do_filter,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        // `local(expr, envir)` must receive `expr` UNEVALUATED (like
        // `with`): the evaluated table would eval the block in the caller
        // env first, leaking assignments to globalenv. The handler builds
        // the child env itself and Rf_evals the raw expression in it.
        name: "local",
        handler: crate::mainutils::essentials::do_local,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        // `rm` upstream is a closure that captures `...` via
        // match.call(expand.dots=FALSE): dots must be SYMBOLS or character
        // strings naming bindings, never evaluated (`rm(x, envir=e)` would
        // otherwise evaluate `x` in the caller and fail). The handler
        // converts raw symbols/strings to names itself and evaluates only
        // the named formals (list/pos/envir/inherits).
        name: "rm",
        handler: crate::mainutils::essentials::do_rm,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "remove",
        handler: crate::mainutils::essentials::do_rm,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "substitute",
        handler: crate::mainutils::coerce::do_substitute,
        restore_visibility_always: true,
    },
    UnevaluatedBuiltin {
        name: "match.arg",
        handler: crate::mainutils::essentials::do_match_arg,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "model.extract",
        handler: crate::library::stats::filter::do_model_extract,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "quote",
        handler: crate::mainutils::essentials::do_quote,
        restore_visibility_always: true,
    },
    UnevaluatedBuiltin {
        name: "system.time",
        handler: crate::mainutils::essentials::do_system_time,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "call",
        handler: crate::mainutils::coerce::do_call,
        restore_visibility_always: true,
    },
    UnevaluatedBuiltin {
        name: "bquote",
        handler: crate::mainutils::essentials::do_bquote,
        restore_visibility_always: true,
    },
    UnevaluatedBuiltin {
        name: "::",
        handler: crate::mainutils::essentials::do_namespace_get,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: ":::",
        handler: crate::mainutils::essentials::do_namespace_get,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "~",
        handler: crate::mainutils::names::do_tilde,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "&&",
        handler: crate::mainutils::logic::do_logic2,
        restore_visibility_always: false,
    },
    UnevaluatedBuiltin {
        name: "||",
        handler: crate::mainutils::logic::do_logic2,
        restore_visibility_always: false,
    },
];

#[derive(Clone, Copy)]
pub(super) struct EvaluatedBuiltin {
    pub name: &'static str,
    pub handler: EvaluatedBuiltinHandler,
}

/// Find the Rust implementation for an evaluated builtin name.
pub(crate) fn evaluated_builtin_handler(name: &str) -> Option<EvaluatedBuiltinHandler> {
    EVALUATED_BUILTINS
        .iter()
        .find(|builtin| builtin.name == name)
        .map(|builtin| builtin.handler)
}

mod base_arith;
mod base_datetime;
mod base_io;
mod base_stats;
mod base_strings;
mod conditions;
mod graphics_external;
mod methods_dispatch;
mod misc;
mod namespace;

macro_rules! concat_evaluated_tables {
    ($fill:expr; $($slice:expr),+ $(,)?) => {{
        const __TOTAL: usize = 0 $(+ $slice.len())+;
        let mut __out = [$fill; __TOTAL];
        let mut __i = 0usize;
        $(
            let mut __j = 0usize;
            while __j < $slice.len() {
                __out[__i] = $slice[__j];
                __i += 1;
                __j += 1;
            }
        )+
        __out
    }};
}

pub(super) const EVALUATED_BUILTINS: &[EvaluatedBuiltin] = &concat_evaluated_tables!(
    base_arith::BASE_ARITH[0];
base_arith::BASE_ARITH,
    base_stats::BASE_STATS,
    base_strings::BASE_STRINGS,
    base_io::BASE_IO,
    base_datetime::BASE_DATETIME,
    graphics_external::GRAPHICS_EXTERNAL,
    methods_dispatch::METHODS_DISPATCH,
    namespace::NAMESPACE,
    conditions::CONDITIONS,
    misc::MISC,
);

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
        for name in ["+", "log", ".rport_require", "subset", "dnorm", "qgeom"] {
            assert!(
                evaluated_builtin_handler(name).is_some(),
                "missing evaluated builtin handler for {name}"
            );
        }
        assert!(evaluated_builtin_handler("if").is_none());
        assert!(evaluated_builtin_handler("require").is_none());
        assert!(evaluated_builtin_handler("library").is_none());
        assert!(unevaluated_builtin_handler("require").is_none());
        assert!(unevaluated_builtin_handler("library").is_none());
    }

    #[test]
    fn unevaluated_builtin_lookup_covers_delayed_argument_forms() {
        let missing = unevaluated_builtin_handler("missing").expect("missing handler");
        assert!(missing.restore_visibility_always);

        for name in ["capture.output", "tryCatch", "with", "lapply"] {
            assert!(
                unevaluated_builtin_handler(name).is_some(),
                "missing unevaluated builtin handler for {name}"
            );
        }
        assert!(unevaluated_builtin_handler("+").is_none());
    }
}
#[cfg(test)]
mod wasm_m1_tests {
    use super::EVALUATED_BUILTINS;

    /// Native gate check: all 21 stats/fourier entries stay wired to
    /// their real handlers on native (wasm stubs are wasm-only).
    #[test]
    fn stats_fourier_entries_use_native_handlers() {
        for name in [
            "runif",
            "rnorm",
            "fft",
            "mvfft",
            "rpois",
            "rexp",
            "rchisq",
            "rgeom",
            "rt",
            "rsignrank",
            "rbeta",
            "rbinom",
            "rcauchy",
            "rf",
            "rgamma",
            "rlnorm",
            "rlogis",
            "rnbinom",
            "rweibull",
            "rwilcox",
            "rhyper",
        ] {
            assert!(
                EVALUATED_BUILTINS
                    .iter()
                    .any(|builtin| builtin.name == name),
                "missing evaluated builtin handler for {name}"
            );
        }
        assert_eq!(
            EVALUATED_BUILTINS
                .iter()
                .filter(|b| [
                    "runif",
                    "rnorm",
                    "fft",
                    "mvfft",
                    "rpois",
                    "rexp",
                    "rchisq",
                    "rgeom",
                    "rt",
                    "rsignrank",
                    "rbeta",
                    "rbinom",
                    "rcauchy",
                    "rf",
                    "rgamma",
                    "rlnorm",
                    "rlogis",
                    "rnbinom",
                    "rweibull",
                    "rwilcox",
                    "rhyper",
                ]
                .contains(&b.name))
                .count(),
            21
        );
    }
}
