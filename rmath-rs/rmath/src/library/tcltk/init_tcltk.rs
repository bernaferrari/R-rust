
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2000--2023  The R Core Team
 *
 *  Ported from r-source/src/library/tcltk/src/init.c
 *
 *  Registration table for the tcltk package.
 */

use std::os::raw::c_int;

use crate::main::registration::DllInfo;

/// Initialize the tcltk package's registered routines.
///
/// In the full C implementation this registers:
///
/// CEntries (.C interface):
///   - Unix:  tcltk_init(1), RTcl_ActivateConsole(0)
///   - Windows: tcltk_start(0), tcltk_end(0)
///
/// ExternEntries (.External interface):
///   - dotTcl(-1), dotTclObjv(1), dotTclcallback(-1),
///   - RTcl_ObjFromVar(1), RTcl_AssignObjToVar(2),
///   - RTcl_StringFromObj(1), RTcl_ObjAsCharVector(1),
///   - RTcl_ObjAsDoubleVector(1), RTcl_ObjAsIntVector(1),
///   - RTcl_ObjAsRawVector(1), RTcl_ObjFromCharVector(2),
///   - RTcl_ObjFromDoubleVector(2), RTcl_ObjFromIntVector(2),
///   - RTcl_ObjFromRawVector(1), RTcl_ServiceMode(1),
///   - RTcl_GetArrayElem(2), RTcl_RemoveArrayElem(2),
///   - RTcl_SetArrayElem(3)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_init_tcltk(_dll: *mut DllInfo) {
    // Stub: actual registration deferred until registration tables are filled in.
    // In the full implementation this would call:
    //   R_registerRoutines(dll, CEntries, NULL, NULL, ExternEntries);
    //   R_useDynamicSymbols(dll, FALSE);
    //   R_forceSymbols(dll, TRUE);
}
