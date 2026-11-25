//! Port of R's src/library/grid/src/register.c -- grid package routine registration.
//!
//! Registers all grid .Call methods with R's dynamic loading system.
//! Currently a no-op stub; the actual grid functions (L_initGrid, L_killGrid, etc.)
//! and the R_CallMethodDef table will be filled in when those functions are ported.

use crate::mainutils::registration::DllInfo;

