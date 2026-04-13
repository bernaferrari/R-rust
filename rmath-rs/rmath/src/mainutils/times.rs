/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2000-2016  The R Core Team.
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
 *
 *
 *      Interfaces to time functions.
 *
 *  Ported from R source: src/main/times.c
 */

#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

use std::os::raw::c_uint;
use std::time::SystemTime;

/// Returns the current time as a double (seconds since the Unix epoch,
/// with sub-second precision).
///
/// This is a Rust port of `currentTime()` from src/main/times.c.
/// It uses `SystemTime` which provides nanosecond precision on supported
/// platforms.
///
/// Note: the `n_leapseconds` subtraction from the original C code is
/// intentionally omitted, as POSIX leap seconds are disallowed.
pub fn currentTime() -> f64 {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => dur.as_secs() as f64 + dur.subsec_nanos() as f64 * 1e-9,
        Err(_) => f64::NAN, // system clock is before UNIX epoch
    }
}

/// Returns an unsigned int seed derived from the current time and process ID.
///
/// This is a Rust port of `TimeToSeed()` from src/main/times.c, used by
/// the RNG, main, and mkdtemp code. The seed is computed by combining
/// sub-second time components with the full seconds via XOR, then mixing
/// in the process ID shifted left by 16 bits.
pub fn TimeToSeed() -> c_uint {
    let pid: c_uint = std::process::id();
    let mut seed: c_uint;

    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => {
            let secs = dur.as_secs() as c_uint;
            // Replicate the C logic: (subsecond_part << 16) ^ secs
            // The C code uses nanoseconds when clock_gettime is available.
            seed = ((dur.subsec_nanos() as u64) << 16) as c_uint ^ secs;
        }
        Err(_) => {
            // System clock before UNIX epoch -- fall back to zero
            seed = 0;
        }
    }

    seed ^= pid << 16;
    seed
}

/// Returns the current system time as a floating-point number of seconds
/// since the Unix epoch.
///
/// Ported from `do_systime()` in `src/main/times.c`. In the full R runtime
/// this is the R-level `Sys.time()` builtin — it checks argument arity then
/// wraps `ScalarReal(currentTime())`.
///
/// The simplified signature (no SEXP arguments) is retained because this
/// port does not yet wire into the SEXP call-dispatch table.
pub unsafe fn do_systime() -> f64 {
    currentTime()
}
