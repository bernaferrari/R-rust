//! Portable grid frontend. Runtime values are decoded before borrowing a device;
//! viewport transforms and graphical parameters are owned session state.
mod layout;
pub(crate) mod replay;
use crate::eval::attrib_core::{R_ClassSymbol, R_NamesSymbol, getAttrib, setAttrib};
use crate::mainutils::errors::Rf_warning1;
use crate::mainutils::essentials::{
    arg_by_name_or_position, base_error, elt_to_string, sexp_has_class,
};
use crate::sexp::{
    accessors::*,
    ffi::{R_xlen_t, SEXP, SEXPTYPE},
    globals::R_NilValue,
    instance::with_required_current_instance,
};
use r_graphics_engine::{
    Color, DashPattern, DrawTarget, Path, PathCommand, PlotParameters, Point, Stroke, TextAnchor,
};
use serde::{Deserialize, Serialize};
// Unit computations keep R allocations rooted and numeric work in owned vectors.
// Mixed dimensions remain deferred expressions until viewport conversion.

mod drawing;
#[cfg(test)]
mod snapshot_tests;
mod state;
mod units;
mod viewports;

pub(crate) use drawing::*;
pub(crate) use state::*;
pub(crate) use units::*;
pub(crate) use viewports::*;

/// Checked node factory bound to the active runtime session. Portable grid
/// helpers only run while the evaluator holds a session, so the ownership
/// boundary cannot lapse between the check and the wrap.
fn node_factory() -> crate::sexp::object::SessionNodeFactory<'static> {
    crate::sexp::object::SessionNodeFactory::new(
        // SAFETY: portable grid helpers only run inside evaluated R calls.
        unsafe { crate::sexp::owner::OwnerToken::current() }
            .unwrap_or_else(|error| base_error(error.to_string())),
    )
}

/// Audited boundary: read-only named-attribute traversal of a live object.
fn attribute(x: SEXP, symbol: SEXP) -> SEXP {
    // SAFETY: getAttrib walks the attribute pairlist without mutation.
    unsafe { getAttrib(x, symbol) }
}

/// Audited boundary: named-attribute write on a live object.
fn set_attribute(x: SEXP, symbol: SEXP, value: SEXP) {
    // SAFETY: setAttrib mutates one named slot of a live object.
    unsafe { setAttrib(x, symbol, value) }
}

/// Audited boundary: the names symbol is a process-global singleton.
fn names_symbol() -> SEXP {
    // SAFETY: returns the shared R_NamesSymbol singleton.
    unsafe { R_NamesSymbol() }
}

/// Audited boundary: the class symbol is a process-global singleton.
fn class_symbol() -> SEXP {
    // SAFETY: returns the shared R_ClassSymbol singleton.
    unsafe { R_ClassSymbol() }
}

/// Audited boundary: read-only class check on a live object.
fn has_class(x: SEXP, class: &str) -> bool {
    sexp_has_class(x, class)
}
