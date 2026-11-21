/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2001-2025 The R Core Team
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/
 */

//! Port of R's src/library/grid/src/clippath.c
//!
//! Grid clip path type checking and resolution.

use crate::mainutils::engine as ge;
use crate::sexp::constructors::Rf_lang2;
use crate::sexp::envir::findFun;
use crate::sexp::ffi::SEXP;

use super::state::setGridStateElement;
use super::types::{pGEDevDesc, *};
use super::{inherits, install, protect, scalar_logical};

struct ResolvingPathGuard {
    dd: pGEDevDesc,
}

impl ResolvingPathGuard {
    fn enter(dd: pGEDevDesc) -> Self {
        setGridStateElement(dd, GSS_RESOLVINGPATH, scalar_logical(1));
        Self { dd }
    }
}

impl Drop for ResolvingPathGuard {
    fn drop(&mut self) {
        setGridStateElement(self.dd, GSS_RESOLVINGPATH, scalar_logical(0));
    }
}

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// isClipPath — check if object is a GridClipPath
// ---------------------------------------------------------------------------

pub fn isClipPath(clip: SEXP) -> bool {
    inherits(clip, c"GridClipPath")
}

// ---------------------------------------------------------------------------
// resolveClipPath — resolve a clip path via R callback
// ---------------------------------------------------------------------------

pub fn resolveClipPath(path: SEXP, dd: pGEDevDesc) -> SEXP {
    let _scope = ResolvingPathGuard::enter(dd);
    // Use the shared grid eval env so clip-path callbacks see the same
    // initialization state as the rest of grid.
    let env = grid_eval_env();
    let resolve_sym = install("resolveClipPath");
    let resolve_fn = findFun(resolve_sym, env);
    let _resolve_fn_guard = protect(resolve_fn);
    // SAFETY: both arguments are live objects lent for the call.
    let r_fcall = Rf_lang2(resolve_fn, path);
    let _r_fcall_guard = protect(r_fcall);
    // SAFETY: R-eval bridge — `r_fcall` is protected above, `env` and `dd`
    // are the live evaluation context and device.
    ge::Rf_eval_with_gd(r_fcall, env, dd)
}
