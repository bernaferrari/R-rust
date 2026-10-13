/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Port of R's src/library/grid/src/grid.c (5470 lines)
 *
 *  grid -- main grid drawing primitives and state management.
 */

use std::ffi::c_void;
use std::os::raw::{c_char, c_double, c_int, c_uint};
use std::ptr;

use crate::attrib_core::{R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib};
use crate::main::duplicate::duplicate as Rf_duplicate;
use crate::mainutils::coerce::asBool;
use crate::mainutils::colors::RGBpar3;
use crate::mainutils::engine::{
    GECap, GELine, GEMode, GENewPage, GEPath, GEPolygon, GEPolyline, GEPretty, GERaster, GESetClip,
    GEStrMetric, GEdeviceDirty, Rf_eval_with_gd, fromDeviceX, fromDeviceY, toDeviceHeight,
    toDeviceWidth, toDeviceX, toDeviceY,
};
use crate::mainutils::errors::{Rf_error, Rf_error1, Rf_warning};
use crate::mainutils::graphics_ffi::rmath_grid_release_definitions as release_grid_definitions;
use crate::mainutils::objects::inherits2 as Rf_inherits;
use crate::mainutils::subset::installTrChar;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::constructors::{Rf_lang2 as lang2, Rf_lang3 as lang3};
use crate::sexp::envir::{R_findVar as findVar, defineVar, findFun};
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::memory_ext::R_alloc;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use super::clippath::{isClipPath, resolveClipPath};
use super::gpar::{
    GP_ALPHA, GP_CEX, GP_COL, GP_FILL, GP_FONT, GP_FONTFAMILY, GP_FONTSIZE, GP_GAMMA, GP_LEX,
    GP_LINEEND, GP_LINEHEIGHT, GP_LINEJOIN, GP_LINEMITRE, GP_LTY, GP_LWD, gcontextFromgpar,
    gpFillSXP, initGContext, initGPar, pGEDevDesc, pGEcontext, resolveGPar, updateGContext,
};
use super::just::{justification, justifyX, justifyY};
use super::layout::calcViewportLocationFromLayout;
use super::mask::{isMask, resolveMask};
use super::state::{gridStateElement, initDL, setGridStateElement};
use super::types::*;
use super::unit::{
    L_INCHES, L_NATIVE, L_NPC, transformDimn, transformHeighttoINCHES, transformLocn,
    transformWHfromNPC, transformWHtoNPC, transformWidthHeightFromINCHES, transformWidthtoINCHES,
    transformXYFromINCHES, transformXYfromNPC, transformXYtoNPC, transformXtoINCHES,
    transformYtoINCHES, unit, unitLength, unitUnit, unitValue,
};
use super::util::{copyRect, getListElement, intersect, rect, setListElement, textRect};
use super::viewport::*;

mod display;
mod edges;
mod helpers;
mod lines;
mod misc;
mod raster;
mod shapes;
mod text;
mod transform;
mod viewport;

#[cfg(test)]
mod tests;
pub use display::*;
pub use edges::*;
pub use helpers::*;
pub use lines::*;
pub use misc::*;
pub use raster::*;
pub use shapes::*;
pub use text::*;
pub use transform::*;
pub use viewport::*;
