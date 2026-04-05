
//! Port of R's src/library/grid/src/register.c -- grid package routine registration.
//!
//! Registers all grid .Call methods with R's dynamic loading system.
//! Currently a no-op stub; the actual grid functions (L_initGrid, L_killGrid, etc.)
//! and the R_CallMethodDef table will be filled in when those functions are ported.

use crate::main::registration::DllInfo;

/// Initialize the grid package's registered routines.
///
/// In the full C implementation, this calls:
///   R_registerRoutines(dll, NULL, callMethods, NULL, NULL);
///   R_useDynamicSymbols(dll, FALSE);
///   R_forceSymbols(dll, TRUE);
///
/// The callMethods table registers ~70 .Call entries including:
///   L_initGrid, L_killGrid, L_gridDirty, L_currentViewport, L_setviewport,
///   L_downviewport, L_downvppath, L_unsetviewport, L_upviewport,
///   L_getDisplayList, L_setDisplayList, L_getDLelt, L_setDLelt,
///   L_getDLindex, L_setDLindex, L_getDLon, L_setDLon,
///   L_getEngineDLon, L_setEngineDLon, L_setGridState, L_getCurrentGrob,
///   L_setCurrentGrob, L_getEngineRecording, L_setEngineRecording,
///   L_currentGPar, L_newpagerecording, L_newpage, L_clearDefinitions,
///   L_initGPar, L_initViewportStack, L_initDisplayList,
///   L_moveTo, L_lineTo, L_lines, L_segments, L_arrows, L_path,
///   L_polygon, L_xspline, L_circle, L_rect, L_raster, L_cap, L_text,
///   L_points, L_clip, L_pretty, L_pretty2, L_locator, L_convert,
///   L_devLoc, L_devDim, L_layoutRegion, L_getGPar, L_setGPar,
///   L_circleBounds, L_locnBounds, L_rectBounds, L_textBounds,
///   L_xsplineBounds, L_xsplinePoints, L_pointsPoints, L_stringMetric,
///   L_stroke, L_fill, L_fillStroke, L_glyph,
///   validUnits, constructUnits, asUnit, conformingUnits, matchUnit,
///   addUnits, multUnits, flipUnits, absoluteUnits, summaryUnits
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_init_grid(_dll: *mut DllInfo) {
    // Stub: actual registration deferred until grid .Call functions are ported.
}
