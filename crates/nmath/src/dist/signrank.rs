// Wilcoxon signed rank distribution: dsignrank, psignrank, qsignrank, rsignrank
// Ported from R's nmath/signrank.c
//
// Original by R Core Team

use crate::constants::*;
use crate::dpq::*;
use crate::error::*;
use crate::utils::*;
use libm::*;
use std::collections::HashMap;
use std::os::raw::{c_double, c_int};

// Constants
const M_LN2: f64 = 0.693147180559945309417232121458;
const DBL_EPSILON: f64 = 2.220446049250313e-16;

// Per-session cached workspace for csignrank.
// w[n] is a Vec<f64> of size (c+1) where c = n*(n+1)/4 (truncated).
// A value of -1.0 means "not yet computed".
