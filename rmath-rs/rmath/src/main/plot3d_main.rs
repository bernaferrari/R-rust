#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1995, 1996  Robert Gentleman and Ross Ihaka
 *  Copyright (C) 1997--2014  The R Core Team
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
 *  Ported from r-source/src/main/plot3d.c
 *
 *  filled contours and perspective plots were originally here,
 *  now in ../library/graphics/src/plot3d.c .
 *
 *  This file provides GEcontourLines() and do_contourLines().
 */

use std::os::raw::{c_char, c_double, c_int, c_void};

use crate::attrib_core::{R_NamesSymbol, setAttrib};
use crate::main::coerce::coerceVector;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory_ext::{vmaxget, vmaxset};
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

/// PROTECT / UNPROTECT convenience wrappers
#[inline(always)]
unsafe fn PROTECT(s: SEXP) -> SEXP {
    unsafe { Rf_protect(s) }
}
#[inline(always)]
unsafe fn UNPROTECT(n: c_int) {
    unsafe {
        Rf_unprotect(n);
    }
}

/// Helper: write to REAL(x)[i]
#[inline]
unsafe fn SET_REAL(x: SEXP, i: usize, val: c_double) {
    unsafe {
        std::ptr::write(REAL(x).add(i), val);
    }
}

/// Helper: read from REAL(x)[i]
#[inline]
unsafe fn GET_REAL(x: SEXP, i: usize) -> c_double {
    unsafe { std::ptr::read(REAL(x).add(i)) }
}

/* Contour list component indices */
const CONTOUR_LIST_STEP: c_int = 100;
const CONTOUR_LIST_LEVEL: i64 = 0;
const CONTOUR_LIST_X: i64 = 1;
const CONTOUR_LIST_Y: i64 = 2;

/// max_contour_segments: safety limit for contour tracing loops.
const max_contour_segments: c_int = 25000;

/// SEG: contour line segment (from contour-common.h).
/// SEGP is a pointer to SEG.
#[repr(C)]
struct SEG {
    next: *mut SEG,
    x0: c_double,
    y0: c_double,
    x1: c_double,
    y1: c_double,
}

type SEGP = *mut SEG;
type SegmentDB = *mut SEGP;

/// R_FINITE -- check if a double is finite.
#[inline]
fn R_FINITE(x: c_double) -> bool {
    x.is_finite()
}

/* Extern declarations for contour-common functions (compiled in library/graphics) */
unsafe extern "C" {
    fn ctr_segdir(
        xend: c_double,
        yend: c_double,
        x: *const c_double,
        y: *const c_double,
        ii: *mut c_int,
        jj: *mut c_int,
        nx: c_int,
        ny: c_int,
    ) -> c_int;

    fn ctr_segupdate(
        xend: c_double,
        yend: c_double,
        dir: c_int,
        tail: c_int,
        seglist: *mut c_void,
        seg: *mut *mut c_void,
    ) -> *mut c_void;
}

/// growList -- grow a VECSXP by CONTOUR_LIST_STEP elements.
unsafe fn growList(oldlist: SEXP) -> SEXP {
    unsafe {
        let len = LENGTH(oldlist);
        let templist = PROTECT(Rf_allocVector(
            SEXPTYPE::VECSXP.0 as c_int,
            len + CONTOUR_LIST_STEP as c_int,
        ));
        for i in 0..len {
            SET_VECTOR_ELT(templist, i as i64, VECTOR_ELT(oldlist, i as i64));
        }
        UNPROTECT(1);
        templist
    }
}

/// addContourLines -- store the list of segments for a single level.
unsafe fn addContourLines(
    x: *const c_double,
    nx: c_int,
    y: *const c_double,
    ny: c_int,
    z: *const c_double,
    zc: c_double,
    atom: c_double,
    segmentDB: SegmentDB,
    mut nlines: c_int,
    container: SEXP,
) -> c_int {
    unsafe {
        let mut xend: c_double;
        let mut yend: c_double;
        let mut i: c_int;
        let mut ii: c_int;
        let mut j: c_int;
        let mut jj: c_int;
        let mut ns: c_int;
        let mut dir: c_int;
        let mut nc: c_int;
        let mut seglist: SEGP;
        let mut seg: SEGP;
        let mut s: SEGP;
        let mut start: SEGP;
        let mut end: SEGP;

        for i_idx in 0..(nx - 1) {
            for j_idx in 0..(ny - 1) {
                i = i_idx;
                j = j_idx;
                loop {
                    seglist = *segmentDB.add((i + j * nx) as usize);
                    if seglist.is_null() {
                        break;
                    }
                    ii = i;
                    jj = j;
                    start = seglist;
                    end = seglist;
                    *segmentDB.add((i + j * nx) as usize) = (*seglist).next;
                    xend = (*seglist).x1;
                    yend = (*seglist).y1;
                    loop {
                        dir = ctr_segdir(xend, yend, x, y, &mut ii, &mut jj, nx, ny);
                        if dir == 0 {
                            break;
                        }
                        let mut seg_void: *mut c_void = std::ptr::null_mut();
                        *segmentDB.add((ii + jj * nx) as usize) = ctr_segupdate(
                            xend,
                            yend,
                            dir,
                            1, /* = tail */
                            *segmentDB.add((ii + jj * nx) as usize) as *mut c_void,
                            &mut seg_void,
                        ) as SEGP;
                        seg = seg_void as SEGP;
                        if seg.is_null() {
                            break;
                        }
                        (*end).next = seg;
                        end = seg;
                        xend = (*end).x1;
                        yend = (*end).y1;
                    }
                    (*end).next = std::ptr::null_mut();
                    ii = i;
                    jj = j;
                    xend = (*seglist).x0;
                    yend = (*seglist).y0;
                    loop {
                        dir = ctr_segdir(xend, yend, x, y, &mut ii, &mut jj, nx, ny);
                        if dir == 0 {
                            break;
                        }
                        let mut seg_void: *mut c_void = std::ptr::null_mut();
                        *segmentDB.add((ii + jj * nx) as usize) = ctr_segupdate(
                            xend,
                            yend,
                            dir,
                            0, /* = head */
                            *segmentDB.add((ii + jj * nx) as usize) as *mut c_void,
                            &mut seg_void,
                        ) as SEGP;
                        seg = seg_void as SEGP;
                        if seg.is_null() {
                            break;
                        }
                        (*seg).next = start;
                        start = seg;
                        xend = (*start).x0;
                        yend = (*start).y0;
                    }

                    /* ns := #{segments of polyline} */
                    s = start;
                    ns = 0;
                    while !s.is_null() && ns < max_contour_segments {
                        ns += 1;
                        s = (*s).next;
                    }
                    if ns == max_contour_segments {
                        crate::main::errors::Rf_warning(
                        b"contour(): circular/long seglist -- set options(\"max.contour.segments\") > 25000?\0"
                            .as_ptr()
                            as *const c_char,
                    );
                    }

                    /* "write" the contour locations into the list */
                    let ctr = PROTECT(Rf_allocVector(SEXPTYPE::VECSXP.0 as c_int, 3));
                    let level = PROTECT(Rf_allocVector(SEXPTYPE::REALSXP.0 as c_int, 1));
                    let xsxp = PROTECT(Rf_allocVector(SEXPTYPE::REALSXP.0 as c_int, ns + 1));
                    let ysxp = PROTECT(Rf_allocVector(SEXPTYPE::REALSXP.0 as c_int, ns + 1));
                    SET_REAL(level, 0, zc);
                    SET_VECTOR_ELT(ctr, CONTOUR_LIST_LEVEL, level);
                    s = start;
                    SET_REAL(xsxp, 0, (*s).x0);
                    SET_REAL(ysxp, 0, (*s).y0);
                    ns = 1;
                    while !(*s).next.is_null() && ns < max_contour_segments {
                        s = (*s).next;
                        SET_REAL(xsxp, ns as usize, (*s).x0);
                        SET_REAL(ysxp, ns as usize, (*s).y0);
                        ns += 1;
                    }
                    SET_REAL(xsxp, ns as usize, (*s).x1);
                    SET_REAL(ysxp, ns as usize, (*s).y1);
                    SET_VECTOR_ELT(ctr, CONTOUR_LIST_X, xsxp);
                    SET_VECTOR_ELT(ctr, CONTOUR_LIST_Y, ysxp);

                    /* Set names attribute */
                    let names = PROTECT(Rf_allocVector(SEXPTYPE::STRSXP.0 as c_int, 3));
                    SET_STRING_ELT(names, 0, Rf_mkChar(b"level\0".as_ptr() as *const c_char));
                    SET_STRING_ELT(names, 1, Rf_mkChar(b"x\0".as_ptr() as *const c_char));
                    SET_STRING_ELT(names, 2, Rf_mkChar(b"y\0".as_ptr() as *const c_char));
                    setAttrib(ctr, R_NamesSymbol(), names);

                    nlines += 1;
                    nc = LENGTH(VECTOR_ELT(container, 0));
                    if nlines == nc {
                        SET_VECTOR_ELT(container, 0, growList(VECTOR_ELT(container, 0)));
                    }
                    SET_VECTOR_ELT(VECTOR_ELT(container, 0), (nlines - 1) as i64, ctr);
                    UNPROTECT(5);
                }
            }
        }
        nlines
    }
}

unsafe extern "C" {
    fn contourLines(
        x: *const c_double,
        nx: c_int,
        y: *const c_double,
        ny: c_int,
        z: *const c_double,
        zc: c_double,
        atom: c_double,
    ) -> *mut c_void;
}

/// GEcontourLines -- given nx x values, ny y values, nx*ny z values,
/// and nl cut-values in z, produce a list of contour lines.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn GEcontourLines(
    x: *const c_double,
    nx: c_int,
    y: *const c_double,
    ny: c_int,
    z: *const c_double,
    levels: *const c_double,
    nl: c_int,
) -> SEXP {
    unsafe {
        let mut i: c_int;
        let mut nlines: c_int;
        let len: c_int;
        let atom: c_double;
        let mut zmin: c_double;
        let mut zmax: c_double;
        let mut segmentDB: SegmentDB;
        let container: SEXP;
        let mut mainlist: SEXP;
        let templist: SEXP;

        /* "tie-breaker" values */
        zmin = f64::MAX;
        zmax = f64::MIN;
        i = 0;
        while i < nx * ny {
            if R_FINITE(*z.add(i as usize)) {
                if zmax < *z.add(i as usize) {
                    zmax = *z.add(i as usize);
                }
                if zmin > *z.add(i as usize) {
                    zmin = *z.add(i as usize);
                }
            }
            i += 1;
        }

        if zmin >= zmax {
            if zmin == zmax {
                crate::main::errors::Rf_warning(
                    b"all z values are equal\0".as_ptr() as *const c_char
                );
            } else {
                crate::main::errors::Rf_warning(b"all z values are NA\0".as_ptr() as *const c_char);
            }
            return R_NilValue();
        }

        atom = 1e-3 * (zmax - zmin);

        /* Create a "container" which is a list with only 1 element.
         * The element is the list of lines that will be built up. */
        container = PROTECT(Rf_allocVector(SEXPTYPE::VECSXP.0 as c_int, 1));
        SET_VECTOR_ELT(
            container,
            0,
            Rf_allocVector(SEXPTYPE::VECSXP.0 as c_int, CONTOUR_LIST_STEP as c_int),
        );
        nlines = 0;

        /* Add lines for each contour level */
        i = 0;
        while i < nl {
            let vmax = vmaxget();

            /* Generate a segment database */
            segmentDB = contourLines(x, nx, y, ny, z, *levels.add(i as usize), atom) as SegmentDB;

            /* Add lines to the list based on the segment database */
            nlines = addContourLines(
                x,
                nx,
                y,
                ny,
                z,
                *levels.add(i as usize),
                atom,
                segmentDB,
                nlines,
                container,
            );

            vmaxset(vmax);
            i += 1;
        }

        /* Trim the list of lines to the appropriate length. */
        len = LENGTH(VECTOR_ELT(container, 0));
        if nlines < len {
            mainlist = VECTOR_ELT(container, 0);
            templist = PROTECT(Rf_allocVector(SEXPTYPE::VECSXP.0 as c_int, nlines));
            i = 0;
            while i < nlines {
                SET_VECTOR_ELT(templist, i as i64, VECTOR_ELT(mainlist, i as i64));
                i += 1;
            }
            mainlist = templist;
            UNPROTECT(1); /* templist */
        } else {
            mainlist = VECTOR_ELT(container, 0);
        }
        UNPROTECT(1); /* container */
        mainlist
    }
}

/// do_contourLines -- .Internal(contourLines(x, y, z, levels))
/// This is for contourLines() in package grDevices.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_contourLines(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let c: SEXP;
        let x: SEXP;
        let y: SEXP;
        let z: SEXP;
        let nx: c_int;
        let ny: c_int;
        let nc: c_int;

        x = PROTECT(coerceVector(CAR(args), SEXPTYPE::REALSXP.0 as c_int));
        nx = LENGTH(x);
        let mut args = CDR(args);

        y = PROTECT(coerceVector(CAR(args), SEXPTYPE::REALSXP.0 as c_int));
        ny = LENGTH(y);
        args = CDR(args);

        z = PROTECT(coerceVector(CAR(args), SEXPTYPE::REALSXP.0 as c_int));
        args = CDR(args);

        /* levels */
        c = PROTECT(coerceVector(CAR(args), SEXPTYPE::REALSXP.0 as c_int));
        nc = LENGTH(c);

        let res = GEcontourLines(REAL(x), nx, REAL(y), ny, REAL(z), REAL(c), nc);
        UNPROTECT(4);
        res
    }
}
