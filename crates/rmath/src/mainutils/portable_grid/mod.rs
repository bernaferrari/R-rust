//! Portable grid frontend. Runtime values are decoded before borrowing a device;
//! viewport transforms and graphical parameters are owned session state.
mod layout;
use crate::eval::attrib_core::{R_ClassSymbol, R_NamesSymbol, getAttrib, setAttrib};
use crate::mainutils::errors::Rf_warning1;
use crate::mainutils::essentials::{arg_by_name_or_position, base_error, elt_to_string};
use crate::sexp::{
    accessors::*,
    constructors::*,
    ffi::{SEXP, SEXPTYPE},
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
