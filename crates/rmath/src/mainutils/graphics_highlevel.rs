//! Bounded, public base graphics methods implemented as ordinary R closures.
//!
//! Keeping these methods in R is intentional: the upstream methods own the
//! argument matching, defaults, and return-object contracts, while plotting
//! is delegated to the portable `plot.*` primitives. The wrappers are cached
//! per session by `base_wrappers::apply`, just like the LOESS adapters.
//!
//! The bounded surface intentionally covers numeric `hist` (including
//! explicit/Sturges/Scott/FD breaks), vector/matrix `barplot`, and numeric
//! vector/list `boxplot`. Hatch-density fills, legends, notch/varwidth box
//! geometry, formula methods, and panel hooks fail explicitly in the R
//! closures instead of producing a misleading partial plot.

use crate::sexp::ffi::SEXP;

/// The public `hist` generic.
pub unsafe fn do_hist(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "hist",
            include_str!("graphics_highlevel/hist_generic.R"),
            args,
            rho,
            false,
        )
    }
}

/// The numeric `hist.default` method.
pub unsafe fn do_hist_default(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "hist.default",
            include_str!("graphics_highlevel/hist_default.R"),
            args,
            rho,
            false,
        )
    }
}

/// The public `barplot` generic.
pub unsafe fn do_barplot(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "barplot",
            include_str!("graphics_highlevel/barplot_generic.R"),
            args,
            rho,
            false,
        )
    }
}

/// The vector/matrix `barplot.default` method.
pub unsafe fn do_barplot_default(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "barplot.default",
            include_str!("graphics_highlevel/barplot_default.R"),
            args,
            rho,
            false,
        )
    }
}

/// The public `boxplot` generic.
pub unsafe fn do_boxplot(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "boxplot",
            include_str!("graphics_highlevel/boxplot_generic.R"),
            args,
            rho,
            false,
        )
    }
}

/// The vector/list `boxplot.default` method.
pub unsafe fn do_boxplot_default(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "boxplot.default",
            include_str!("graphics_highlevel/boxplot_default.R"),
            args,
            rho,
            false,
        )
    }
}
