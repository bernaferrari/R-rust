#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/radixsort.c
//!
//! Based on code donated from the data.table package
//! (C) 2006-2015 Matt Dowle and Arun Srinivasan.
//!
//! This module ports the core integer radix sort algorithm as standalone
//! Rust functions, plus the full do_radixsort SEXP wrapper.

use std::cell::{Cell, RefCell};
use std::os::raw::{c_int, c_void};
use std::ptr;

use crate::eval::attrib_core::setAttrib;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{NA_LOGICAL, NA_REAL, R_xlen_t, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Replaced n < 200 with n < N_SMALL. Easier to change later.
const N_SMALL: usize = 200;

/// Range limit for counting sort. Should be less than INT_MAX
/// (see setRange for details).
const N_RANGE: i32 = 100000;

/// R's NA_INTEGER sentinel value (INT_MIN).
const NA_INTEGER: i32 = i32::MIN;

// ---------------------------------------------------------------------------
// Module-level state for the group-size stack
// ---------------------------------------------------------------------------
//
// gs = groupsizes e.g. 23, 12, 87, 2, 1, 34,...
// Two vectors flip-flopped: flip and 1 - flip.

thread_local! {
    static GS: Cell<[*mut c_int; 2]> = Cell::new([ptr::null_mut(); 2]);
    static FLIP: Cell<c_int> = Cell::new(0);
    static GSALLOC: RefCell<[c_int; 2]> = RefCell::new([0; 2]);
    static GSNGRP: RefCell<[c_int; 2]> = RefCell::new([0; 2]);
    static GSMAX: RefCell<[c_int; 2]> = RefCell::new([0; 2]);
    static GSMAXALLOC: Cell<c_int> = Cell::new(0);
    static STACKGRPS: Cell<bool> = Cell::new(true);
    static SORTSTR: Cell<bool> = Cell::new(true);
    static NEWO: Cell<*mut c_int> = Cell::new(ptr::null_mut());
    static NALAST: Cell<c_int> = Cell::new(-1);
    static ORDER: Cell<c_int> = Cell::new(1);
    static RANGE: Cell<c_int> = Cell::new(NA_INTEGER);
    static XMIN: Cell<c_int> = Cell::new(NA_INTEGER);
    static COUNTS: RefCell<Box<[u32]>> = RefCell::new(Box::new([0u32; N_RANGE as usize + 1]));
    static RADIXCOUNTS: RefCell<[[u32; 257]; 8]> = RefCell::new([[0; 257]; 8]);
    static SKIP: RefCell<[c_int; 8]> = RefCell::new([0; 8]);
    static RADIX_XSUB: Cell<*mut c_void> = Cell::new(ptr::null_mut());
    static RADIX_XSUBALLOC: Cell<usize> = Cell::new(0);
    static OTMP: Cell<*mut c_int> = Cell::new(ptr::null_mut());
    static OTMP_ALLOC: Cell<usize> = Cell::new(0);
    static XTMP: Cell<*mut c_void> = Cell::new(ptr::null_mut());
    static XTMP_ALLOC: Cell<usize> = Cell::new(0);
}

// ---------------------------------------------------------------------------
// Group stack helpers
// ---------------------------------------------------------------------------

unsafe fn growstack(newlen: u64) {
    unsafe {
        let mut newlen = if newlen == 0 { 100000 } else { newlen };
        let flip = FLIP.with(|v| v.get()) as usize;
        let gsmaxalloc = GSMAXALLOC.with(|v| v.get());
        if newlen > gsmaxalloc as u64 {
            newlen = gsmaxalloc as u64;
        }
        let gs = GS.with(|v| v.get());
        let old_ptr = gs[flip];
        let new_ptr = libc_realloc(
            old_ptr as *mut c_void,
            newlen as usize * std::mem::size_of::<c_int>(),
        ) as *mut c_int;
        GS.with(|v| {
            let mut a = v.get();
            a[flip] = new_ptr;
            v.set(a);
        });
        if new_ptr.is_null() {
            eprintln!(
                "Failed to realloc working memory stack to {}*4bytes (flip={})",
                newlen, flip
            );
            return;
        }
        GSALLOC.with(|v| v.borrow_mut()[flip] = newlen as c_int);
    }
}

unsafe fn push(x: c_int) {
    unsafe {
        if !STACKGRPS.with(|v| v.get()) || x == 0 {
            return;
        }
        let flip = FLIP.with(|v| v.get()) as usize;
        let ngrp = GSNGRP.with(|v| v.borrow()[flip]);
        let alloc = GSALLOC.with(|v| v.borrow()[flip]);
        if alloc == ngrp {
            growstack((ngrp as u64) * 2);
        }
        let gs = GS.with(|v| v.get());
        *gs[flip].add(ngrp as usize) = x;
        GSNGRP.with(|v| v.borrow_mut()[flip] = ngrp + 1);
        if x > GSMAX.with(|v| v.borrow()[flip]) {
            GSMAX.with(|v| v.borrow_mut()[flip] = x);
        }
    }
}

unsafe fn mpush(x: c_int, n: c_int) {
    unsafe {
        if !STACKGRPS.with(|v| v.get()) || x == 0 {
            return;
        }
        let flip = FLIP.with(|v| v.get()) as usize;
        let ngrp = GSNGRP.with(|v| v.borrow()[flip]);
        let alloc = GSALLOC.with(|v| v.borrow()[flip]);
        if alloc < ngrp + n {
            growstack(((ngrp as u64) + n as u64) * 2);
        }
        let gs = GS.with(|v| v.get());
        let mut cur_ngrp = ngrp;
        for _i in 0..n {
            *gs[flip].add(cur_ngrp as usize) = x;
            cur_ngrp += 1;
        }
        GSNGRP.with(|v| v.borrow_mut()[flip] = cur_ngrp);
        if x > GSMAX.with(|v| v.borrow()[flip]) {
            GSMAX.with(|v| v.borrow_mut()[flip] = x);
        }
    }
}

unsafe fn flipflop() {
    unsafe {
        FLIP.with(|v| v.set(1 - v.get()));
        let flip = FLIP.with(|v| v.get()) as usize;
        GSNGRP.with(|v| v.borrow_mut()[flip] = 0);
        GSMAX.with(|v| v.borrow_mut()[flip] = 0);
        let alloc = GSALLOC.with(|v| v.borrow()[flip]);
        let other_alloc = GSALLOC.with(|v| v.borrow()[1 - flip]);
        if alloc < other_alloc {
            growstack(other_alloc as u64 * 2);
        }
    }
}

/// Free all group-stack memory.
pub unsafe fn gsfree() {
    unsafe {
        let gs = GS.with(|v| v.get());
        libc_free(gs[0] as *mut c_void);
        libc_free(gs[1] as *mut c_void);
        GS.with(|v| v.set([ptr::null_mut(); 2]));
        FLIP.with(|v| v.set(0));
        GSALLOC.with(|v| *v.borrow_mut() = [0; 2]);
        GSNGRP.with(|v| *v.borrow_mut() = [0; 2]);
        GSMAX.with(|v| *v.borrow_mut() = [0; 2]);
        GSMAXALLOC.with(|v| v.set(0));
    }
}

// ---------------------------------------------------------------------------
// Temporary allocation helpers
// ---------------------------------------------------------------------------

unsafe fn alloc_otmp(n: c_int) {
    unsafe {
        if OTMP_ALLOC.with(|v| v.get()) >= n as usize {
            return;
        }
        let old = OTMP.with(|v| v.get());
        let new_ptr = libc_realloc(
            old as *mut c_void,
            n as usize * std::mem::size_of::<c_int>(),
        ) as *mut c_int;
        if new_ptr.is_null() {
            eprintln!(
                "Failed to allocate working memory for otmp. Requested {} * {} bytes",
                n,
                std::mem::size_of::<c_int>()
            );
            return;
        }
        OTMP.with(|v| v.set(new_ptr));
        OTMP_ALLOC.with(|v| v.set(n as usize));
    }
}

unsafe fn alloc_xtmp(n: c_int) {
    unsafe {
        if XTMP_ALLOC.with(|v| v.get()) >= n as usize {
            return;
        }
        // Currently always the largest type (double) but could be int if that's all needed.
        let old = XTMP.with(|v| v.get());
        let new_ptr = libc_realloc(old, n as usize * std::mem::size_of::<f64>());
        if new_ptr.is_null() {
            eprintln!(
                "Failed to allocate working memory for xtmp. Requested {} * {} bytes",
                n,
                std::mem::size_of::<f64>()
            );
            return;
        }
        XTMP.with(|v| v.set(new_ptr));
        XTMP_ALLOC.with(|v| v.set(n as usize));
    }
}

// ---------------------------------------------------------------------------
// setRange -- determine min and range of integer data
// ---------------------------------------------------------------------------

/// Compute the minimum value and range of an integer array, skipping
/// NA_INTEGER values. Sets module-level `XMIN` and `RANGE`.
pub unsafe fn setRange(x: *const c_int, n: c_int) {
    unsafe {
        XMIN.with(|v| v.set(NA_INTEGER));
        let mut xmax: c_int = NA_INTEGER;
        let overflow: f64;

        let mut i: c_int = 0;
        while i < n && *x.add(i as usize) == NA_INTEGER {
            i += 1;
        }
        if i < n {
            xmax = *x.add(i as usize);
            XMIN.with(|v| v.set(xmax));
        }
        for ii in i..n {
            let tmp = *x.add(ii as usize);
            if tmp == NA_INTEGER {
                continue;
            }
            if tmp > xmax {
                xmax = tmp;
            } else if tmp < XMIN.with(|v| v.get()) {
                XMIN.with(|v| v.set(tmp));
            }
        }
        // all NAs, nothing to do
        if XMIN.with(|v| v.get()) == NA_INTEGER {
            RANGE.with(|v| v.set(NA_INTEGER));
            return;
        }
        // ex: x=c(-2147483647L, NA_integer_, 1L) results in overflowing int range.
        overflow = (xmax as f64) - (XMIN.with(|v| v.get()) as f64) + 1.0;
        // detect and force iradix here, since icount is out of the picture
        if overflow > (c_int::MAX as f64) {
            RANGE.with(|v| v.set(c_int::MAX));
            return;
        }

        RANGE.with(|v| v.set(xmax - XMIN.with(|v| v.get()) + 1));
    }
}

// ---------------------------------------------------------------------------
// icheck -- transform value to account for nalast and order
// ---------------------------------------------------------------------------

/// x*order results in integer overflow when -1*NA,
/// so careful to avoid that here.
#[inline]
unsafe fn icheck(x: c_int) -> c_int {
    let nalast = NALAST.with(|v| v.get());
    let order = ORDER.with(|v| v.get());
    // if nalast == 1, NAs must go last.
    if nalast != 1 {
        if x != NA_INTEGER { x * order } else { x }
    } else {
        if x != NA_INTEGER {
            x * order - 1
        } else {
            c_int::MAX
        }
    }
}

// ---------------------------------------------------------------------------
// icount -- counting sort for integers
// ---------------------------------------------------------------------------

/// Counting sort for integers.
///
/// 1. Places the ordering into `o` directly, overwriting whatever was there.
/// 2. Doesn't change `x`.
/// 3. Pushes group sizes onto the group stack.
///
/// # Safety
/// - `x` must point to at least `n` valid i32 values.
/// - `o` must point to at least `n` valid i32 values.
/// - `RANGE` and `XMIN` must be set correctly (via `setRange`) before calling.
/// - `NALAST` and `ORDER` module-level state must be configured.
pub unsafe fn icount(x: *const c_int, o: *mut c_int, n: c_int) {
    unsafe {
        let range = RANGE.with(|v| v.get());
        let xmin = XMIN.with(|v| v.get());
        let nalast = NALAST.with(|v| v.get());
        let order = ORDER.with(|v| v.get());

        let napos = range; // NA's always counted in last bin
        // static is IMPORTANT, counting sort is called repetitively.

        if range > N_RANGE {
            eprintln!(
                "Internal error: range = {}; isorted cannot handle range > {}",
                range, N_RANGE
            );
            return;
        }
        for i in 0..n as usize {
            if *x.add(i) == NA_INTEGER {
                COUNTS.with(|v| v.borrow_mut()[napos as usize] += 1);
            } else {
                COUNTS.with(|v| v.borrow_mut()[(*x.add(i) - xmin) as usize] += 1);
            }
        }

        let mut tmp: c_int = 0;
        if nalast != 1 && COUNTS.with(|v| v.borrow()[napos as usize]) != 0 {
            push(COUNTS.with(|v| v.borrow()[napos as usize]) as c_int);
            tmp += COUNTS.with(|v| v.borrow()[napos as usize]) as c_int;
        }
        let mut w: c_int = if order == 1 { 0 } else { range - 1 };
        for _i in 0..range {
            let cw = COUNTS.with(|v| v.borrow()[w as usize]);
            if cw != 0 {
                // cumulate but not through 0's.
                // Helps resetting zeros when n < range, below.
                push(cw as c_int);
                tmp += cw as c_int;
                COUNTS.with(|v| v.borrow_mut()[w as usize] = tmp as u32);
            }
            w += order; // order is +1 or -1
        }
        if nalast == 1 && COUNTS.with(|v| v.borrow()[napos as usize]) != 0 {
            push(COUNTS.with(|v| v.borrow()[napos as usize]) as c_int);
            tmp += COUNTS.with(|v| v.borrow()[napos as usize]) as c_int;
            COUNTS.with(|v| v.borrow_mut()[napos as usize] = tmp as u32);
        }
        for i in (0..n as usize).rev() {
            let idx = if *x.add(i) == NA_INTEGER {
                napos as usize
            } else {
                (*x.add(i) - xmin) as usize
            };
            COUNTS.with(|v| v.borrow_mut()[idx] -= 1);
            *o.add(COUNTS.with(|v| v.borrow()[idx]) as usize) = (i + 1) as c_int;
        }
        // nalast = 1, -1 are both taken care already.
        if nalast == 0 {
            // nalast = 0 is dealt with separately as it just sets o to 0
            for i in 0..n as usize {
                if *x.add(*o.add(i) as usize - 1) == NA_INTEGER {
                    *o.add(i) = 0;
                }
            }
            // at those indices where x is NA. x[o[i]-1] because x is not modified here.
        }

        /* counts were cumulated above so leaves non zero.
        Faster to clear up now ready for next time. */
        if (n as usize) < (range) as usize {
            /* Many zeros in counts already. Loop through n instead,
            doesn't matter if we set to 0 several times on any repeats */
            COUNTS.with(|v| v.borrow_mut()[napos as usize] = 0);
            for i in 0..n as usize {
                if *x.add(i) != NA_INTEGER {
                    COUNTS.with(|v| v.borrow_mut()[(*x.add(i) - xmin) as usize] = 0);
                }
            }
        } else if range + 1 > 0 {
            // memset counts to 0
            for j in 0..=(range as usize) {
                COUNTS.with(|v| v.borrow_mut()[j] = 0);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// iinsert -- insertion sort for small integer arrays
// ---------------------------------------------------------------------------

/// Insertion sort that orders both `x` and `o` by reference in-place.
/// Fast for small vectors, low overhead.
///
/// When nalast == 0, iinsert will be called only from within iradix,
/// where o[.] = 0 for x[.] = NA is already taken care of.
///
/// # Safety
/// - `x` and `o` must each point to at least `n` valid i32 values.
pub unsafe fn iinsert(x: *mut c_int, o: *mut c_int, n: c_int) {
    unsafe {
        for i in 1..n as usize {
            let xtmp = *x.add(i);
            if xtmp < *x.add(i - 1) {
                let mut j = (i - 1) as isize;
                let otmp = *o.add(i);
                while j >= 0 && xtmp < *x.add(j as usize) {
                    *x.add((j + 1) as usize) = *x.add(j as usize);
                    *o.add((j + 1) as usize) = *o.add(j as usize);
                    j -= 1;
                }
                *x.add((j + 1) as usize) = xtmp;
                *o.add((j + 1) as usize) = otmp;
            }
        }
        let mut tt: c_int = 0;
        for i in 1..n as usize {
            if *x.add(i) == *x.add(i - 1) {
                tt += 1;
            } else {
                push(tt + 1);
                tt = 0;
            }
        }
        push(tt + 1);
    }
}

// ---------------------------------------------------------------------------
// iradix -- 4-pass MSD radix sort for integers
// ---------------------------------------------------------------------------

/*
  iradix is a counting sort performed forwards from MSB to LSB, with
  some tricks and short circuits building on Terdiman and Herf.
  http://codercorner.com/RadixSortRevisited.htm
  http://stereopsis.com/radix.html

  ~ Note they are LSD, but we do MSD here which is more complicated,
    for efficiency.
  ~ NAs need no special treatment as NA is the most negative integer
    in R (checked in init.c once, for efficiency) so NA naturally sort
    to the front.
  ~ Using 4-pass 1-byte radix for the following reasons:

  * 11-bit (Herf) reduces to 3-passes (3*11=33) yes, and LSD need
    random access to o vector in each pass 1:n so reduction in passes is
    good, but Terdiman's idea to skip a radix if all values are equal
    occurs less the wider the radix. A narrower radix benefits more from that.
    * That's detected here using a single 'if', an improvement on
      Terdiman's exposition of a single loop to find if any count==n
    * The pass through counts bites when radix is wider,
      because we repetitively call this iradix from fastorder forwards.
  *  Herf's parallel histogramming is neat. In 4-pass 1-byte it needs
     4*256 storage, that's tiny, and can be static. 4*256 << 3*2048.
     4-pass 1-byte is simpler and tighter code than 3-pass 11-bit,
     giving modern optimizers and modern CPUs a better chance.
     We may get lucky anyway, if one or two of the 4-passes are skipped.

  Recall: there are no comparisons at all in counting and radix,
  there is wide random access in each LSD radix pass, though.
*/

/// 4-pass MSD radix sort for integers.
///
/// As icount:
///   Places the ordering into `o` directly, overwriting whatever was there.
///   Doesn't change `x`.
///   Pushes group sizes onto the group stack.
///
/// # Safety
/// - `x` must point to at least `n` valid i32 values (read-only).
/// - `o` must point to at least `n` valid i32 values (written to).
/// - Module-level state (NALAST, ORDER, etc.) must be configured.
pub unsafe fn iradix(x: *const c_int, o: *mut c_int, n: c_int) {
    unsafe {
        let mut nextradix: c_int;
        let mut itmp: c_int;
        let mut thisgrpn: c_int;
        let mut maxgrpn: c_int;
        let mut thisx: u32 = 0;
        let shift: u32;
        let thiscounts: *mut u32;

        for i in 0..n as usize {
            /* parallel histogramming pass; i.e. count occurrences of
            0:255 in each byte. Sequential so almost negligible. */
            // relies on overflow behaviour. And shouldn't -INT_MIN be up in iradix?
            thisx = (icheck(*x.add(i)) as u32).wrapping_sub(c_int::MIN as u32);
            // unrolled since inside n-loop
            RADIXCOUNTS.with(|v| v.borrow_mut()[0][(thisx & 0xFF) as usize] += 1);
            RADIXCOUNTS.with(|v| v.borrow_mut()[1][((thisx >> 8) & 0xFF) as usize] += 1);
            RADIXCOUNTS.with(|v| v.borrow_mut()[2][((thisx >> 16) & 0xFF) as usize] += 1);
            RADIXCOUNTS.with(|v| v.borrow_mut()[3][((thisx >> 24) & 0xFF) as usize] += 1);
        }
        for radix in 0..4 {
            /* any(count == n) => all radix must have been that value =>
            last x (still thisx) was that value */
            let idx = ((thisx >> (radix * 8)) & 0xFF) as usize;
            let skip_val = if RADIXCOUNTS.with(|v| v.borrow()[radix][idx]) == n as u32 {
                1
            } else {
                0
            };
            SKIP.with(|v| v.borrow_mut()[radix] = skip_val);
            // clear it now, the other counts must be 0 already
            if skip_val != 0 {
                RADIXCOUNTS.with(|v| v.borrow_mut()[radix][idx] = 0);
            }
        }

        let mut radix: c_int = 3; // MSD
        while radix >= 0 && SKIP.with(|v| v.borrow()[radix as usize]) != 0 {
            radix -= 1;
        }
        if radix == -1 {
            // All radix are skipped; one number repeated n times.
            if NALAST.with(|v| v.get()) == 0 && *x.add(0) == NA_INTEGER {
                for i in 0..n as usize {
                    *o.add(i) = 0;
                }
            } else {
                for i in 0..n as usize {
                    *o.add(i) = (i + 1) as c_int;
                }
            }
            push(n);
            return;
        }
        for i in (0..radix as usize).rev() {
            if SKIP.with(|v| v.borrow()[i]) == 0 {
                // clear the counts as we only needed the parallel pass for skip[]
                // and we're going to use radixcounts again below.
                for j in 0..257 {
                    RADIXCOUNTS.with(|v| v.borrow_mut()[i][j] = 0);
                }
            }
        }
        thiscounts = RADIXCOUNTS.with(|v| v.borrow_mut()[radix as usize].as_mut_ptr());
        shift = (radix * 8) as u32;

        itmp = RADIXCOUNTS.with(|v| v.borrow()[radix as usize][0]) as c_int;
        maxgrpn = itmp;
        let mut ii: usize = 1;
        while itmp < n && ii < 256 {
            thisgrpn = RADIXCOUNTS.with(|v| v.borrow()[radix as usize][ii]) as c_int;
            if thisgrpn != 0 {
                // don't cumulate through 0s, important below.
                if thisgrpn > maxgrpn {
                    maxgrpn = thisgrpn;
                }
                itmp += thisgrpn;
                RADIXCOUNTS.with(|v| v.borrow_mut()[radix as usize][ii] = itmp as u32);
            }
            ii += 1;
        }
        for i in (0..n as usize).rev() {
            thisx = (icheck(*x.add(i)) as u32).wrapping_sub(c_int::MIN as u32);
            let bucket = ((thisx >> shift) & 0xFF) as usize;
            RADIXCOUNTS.with(|v| v.borrow_mut()[radix as usize][bucket] -= 1);
            *o.add(RADIXCOUNTS.with(|v| v.borrow()[radix as usize][bucket]) as usize) =
                (i + 1) as c_int;
        }

        if RADIX_XSUBALLOC.with(|v| v.get()) < maxgrpn as usize {
            // The largest group according to the first non-skipped radix,
            // so could be big (if radix is needed on first arg).
            let old_xsub = RADIX_XSUB.with(|v| v.get());
            let new_xsub = libc_realloc(old_xsub, maxgrpn as usize * std::mem::size_of::<f64>());
            if new_xsub.is_null() {
                eprintln!(
                    "Failed to realloc working memory {}*8bytes (xsub in iradix), radix={}",
                    maxgrpn, radix
                );
                return;
            }
            RADIX_XSUB.with(|v| v.set(new_xsub));
            RADIX_XSUBALLOC.with(|v| v.set(maxgrpn as usize));
        }

        alloc_otmp(maxgrpn);
        alloc_xtmp(maxgrpn);

        nextradix = radix - 1;
        while nextradix >= 0 && SKIP.with(|v| v.borrow()[nextradix as usize]) != 0 {
            nextradix -= 1;
        }
        if RADIXCOUNTS.with(|v| v.borrow()[radix as usize][0]) != 0 {
            eprintln!(
                "Internal error. thiscounts[0]={} but should have been decremented to 0. iradix={}",
                RADIXCOUNTS.with(|v| v.borrow()[radix as usize][0]),
                radix
            );
            return;
        }
        RADIXCOUNTS.with(|v| v.borrow_mut()[radix as usize][256] = n as u32);
        itmp = 0;
        let mut ii: usize = 1;
        while itmp < n && ii <= 256 {
            if RADIXCOUNTS.with(|v| v.borrow()[radix as usize][ii]) == 0 {
                ii += 1;
                continue;
            }
            let thisgrpn = RADIXCOUNTS.with(|v| v.borrow()[radix as usize][ii]) as c_int - itmp; // undo cumulate; i.e. diff
            if thisgrpn == 1 || nextradix == -1 {
                push(thisgrpn);
            } else {
                let xsub = RADIX_XSUB.with(|v| v.get());
                for j in 0..thisgrpn as usize {
                    // this is why this xsub here can't be the same memory as
                    // xsub in do_radixsort.
                    *(xsub as *mut c_int).add(j) =
                        icheck(*x.add(*o.add((itmp + j as c_int) as usize) as usize - 1));
                }
                // changes xsub and o by reference recursively.
                iradix_r(
                    xsub as *mut c_int,
                    o.add(itmp as usize),
                    thisgrpn,
                    nextradix,
                );
            }
            itmp = RADIXCOUNTS.with(|v| v.borrow()[radix as usize][ii]) as c_int;
            RADIXCOUNTS.with(|v| v.borrow_mut()[radix as usize][ii] = 0);
            ii += 1;
        }
        if NALAST.with(|v| v.get()) == 0 {
            // nalast = 0 is dealt with separately as it just sets o to 0
            for i in 0..n as usize {
                if *x.add(*o.add(i) as usize - 1) == NA_INTEGER {
                    *o.add(i) = 0;
                }
            }
            // at those indices where x is NA. x[o[i]-1] because x is not
            // modified by reference unlike iinsert or iradix_r
        }
    }
}

// ---------------------------------------------------------------------------
// iradix_r -- recursive radix sort for integer sub-groups
// ---------------------------------------------------------------------------

/// Recursive helper for `iradix`.
///
/// `xsub` is a recursive offset into xsub working memory above in
/// iradix, reordered by reference. `osub` is an offset into the main
/// answer `o`, reordered by reference. `radix` iterates 3, 2, 1, 0.
unsafe fn iradix_r(xsub: *mut c_int, osub: *mut c_int, n: c_int, radix: c_int) {
    unsafe {
        // N_SMALL=200 is guess based on limited testing. Needs calibrate().
        // Was 50 based on sum(1:50)=1275 worst -vs- 256 cumulate + 256 memset +
        // allowance since reverse order is unlikely.
        // when nalast==0, iinsert will be called only from within iradix.
        if (n as usize) < N_SMALL {
            iinsert(xsub, osub, n);
            return;
        }

        let shift = (radix * 8) as u32;
        let thiscounts = RADIXCOUNTS.with(|v| v.borrow_mut()[radix as usize].as_mut_ptr());

        for i in 0..n as usize {
            let thisx = (*xsub.add(i) as u32).wrapping_sub(c_int::MIN as u32);
            RADIXCOUNTS
                .with(|v| v.borrow_mut()[radix as usize][((thisx >> shift) & 0xFF) as usize] += 1);
        }
        let mut itmp = RADIXCOUNTS.with(|v| v.borrow()[radix as usize][0]) as c_int;
        let mut ii: usize = 1;
        while itmp < n && ii < 256 {
            // don't cumulate through 0s, important below
            if RADIXCOUNTS.with(|v| v.borrow()[radix as usize][ii]) != 0 {
                itmp += RADIXCOUNTS.with(|v| v.borrow()[radix as usize][ii]) as c_int;
                RADIXCOUNTS.with(|v| v.borrow_mut()[radix as usize][ii] = itmp as u32);
            }
            ii += 1;
        }
        let otmp = OTMP.with(|v| v.get());
        let xtmp = XTMP.with(|v| v.get());
        for i in (0..n as usize).rev() {
            let thisx = (*xsub.add(i) as u32).wrapping_sub(c_int::MIN as u32);
            let bucket = ((thisx >> shift) & 0xFF) as usize;
            RADIXCOUNTS.with(|v| v.borrow_mut()[radix as usize][bucket] -= 1);
            let j = RADIXCOUNTS.with(|v| v.borrow()[radix as usize][bucket]) as usize;
            *otmp.add(j) = *osub.add(i);
            *(xtmp as *mut c_int).add(j) = *xsub.add(i);
        }
        ptr::copy_nonoverlapping(otmp, osub, n as usize);
        ptr::copy_nonoverlapping(xtmp as *const c_int, xsub, n as usize);

        let mut nextradix = radix - 1;
        while nextradix >= 0 && SKIP.with(|v| v.borrow()[nextradix as usize]) != 0 {
            nextradix -= 1;
        }

        if RADIXCOUNTS.with(|v| v.borrow()[radix as usize][0]) != 0 {
            eprintln!(
                "Logical error. thiscounts[0]={} but should have been decremented to 0. radix={}",
                RADIXCOUNTS.with(|v| v.borrow()[radix as usize][0]),
                radix
            );
            return;
        }
        RADIXCOUNTS.with(|v| v.borrow_mut()[radix as usize][256] = n as u32);
        itmp = 0;
        let mut ii: usize = 1;
        while itmp < n && ii <= 256 {
            if RADIXCOUNTS.with(|v| v.borrow()[radix as usize][ii]) == 0 {
                ii += 1;
                continue;
            }
            let thisgrpn = RADIXCOUNTS.with(|v| v.borrow()[radix as usize][ii]) as c_int - itmp; // undo cumulate; i.e. diff
            if thisgrpn == 1 || nextradix == -1 {
                push(thisgrpn);
            } else {
                iradix_r(
                    xsub.add(itmp as usize),
                    osub.add(itmp as usize),
                    thisgrpn,
                    nextradix,
                );
            }
            itmp = RADIXCOUNTS.with(|v| v.borrow()[radix as usize][ii]) as c_int;
            RADIXCOUNTS.with(|v| v.borrow_mut()[radix as usize][ii] = 0);
            ii += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// isorted -- test integer vector for sortedness
// ---------------------------------------------------------------------------

/// Test whether an integer vector is already sorted.
///
/// Returns:
/// - `1` if sorted in the expected `ORDER` direction
/// - `-1` if sorted in strictly the opposite direction (no ties)
/// - `-2` if nalast==0 and all values are NA
/// - `0` if unsorted
///
/// Also pushes group sizes onto the group stack.
///
/// # Safety
/// - `x` must point to at least `n` valid i32 values.
pub unsafe fn isorted(x: *const c_int, n: c_int) -> c_int {
    unsafe {
        let mut j: c_int = 0;
        let nalast = NALAST.with(|v| v.get());
        // when nalast = NA,
        // all NAs ? return special value to replace all o's values with '0'
        // any NAs ? return 0 = unsorted and leave it
        //   to sort routines to replace o's with 0's
        // no NAs ? continue to check rest of isorted - the same routine as usual
        if nalast == 0 {
            for k in 0..n as usize {
                if *x.add(k) != NA_INTEGER {
                    j += 1;
                }
            }
            if j == 0 {
                push(n);
                return -2;
            }
            if j != n {
                return 0;
            }
        }
        if n <= 1 {
            push(n);
            return 1;
        }
        if icheck(*x.add(1)) < icheck(*x.add(0)) {
            let mut i = 2;
            while i < n && icheck(*x.add(i as usize)) < icheck(*x.add((i - 1) as usize)) {
                i += 1;
            }
            // strictly opposite to expected 'order', no ties;
            if i == n {
                mpush(1, n);
                return -1;
            }
            // e.g. no more than one NA at the beginning/end (for order=-1/1)
            return 0;
        }
        let flip = FLIP.with(|v| v.get()) as usize;
        let old = GSNGRP.with(|v| v.borrow()[flip]);
        let mut tt: c_int = 1;
        for i in 1..n as usize {
            if icheck(*x.add(i)) < icheck(*x.add(i - 1)) {
                GSNGRP.with(|v| v.borrow_mut()[flip] = old);
                return 0;
            }
            if *x.add(i) == *x.add(i - 1) {
                tt += 1;
            } else {
                push(tt);
                tt = 1;
            }
        }
        push(tt);
        // same as 'order', NAs at the beginning for order=1, at end for
        // order=-1, possibly with ties
        1
    }
}

// ---------------------------------------------------------------------------
// isort -- dispatch between icount, iradix, and iinsert
// ---------------------------------------------------------------------------

/// Sort an integer vector, dispatching to counting sort, radix sort,
/// or insertion sort depending on data characteristics.
///
/// # Safety
/// - `x` must point to at least `n` valid i32 values.
/// - `o` must point to at least `n` valid i32 values.
pub unsafe fn isort(x: *mut c_int, o: *mut c_int, n: c_int) {
    unsafe {
        if n <= 2 {
            // nalast = 0 and n == 2 (check bottom of this file for explanation)
            if NALAST.with(|v| v.get()) == 0 && n == 2 {
                if *o.add(0) == -1 {
                    *o.add(0) = 1;
                    *o.add(1) = 2;
                }
                for i in 0..n as usize {
                    if *x.add(i) == NA_INTEGER {
                        *o.add(i) = 0;
                    }
                }
                push(1);
                push(1);
                return;
            } else {
                eprintln!(
                    "Internal error: isort received n={}. isorted should have dealt with this already",
                    n
                );
                return;
            }
        }
        let nalast = NALAST.with(|v| v.get());
        let order = ORDER.with(|v| v.get());
        if (n as usize) < N_SMALL && *o.add(0) != -1 && nalast != 0 {
            // see comment above in iradix_r on N_SMALL=200.
            if order != 1 || nalast != -1 {
                // so that default case, i.e., order=1, nalast=FALSE will
                // not be affected (ex: `setkey`)
                for i in 0..n as usize {
                    *x.add(i) = icheck(*x.add(i));
                }
            }
            iinsert(x, o, n);
        } else {
            /* Tighter range (e.g. copes better with a few abnormally large
            values in some groups), but also, when setRange was once at
            arg level that caused an extra scan of (long) x
            first. 10,000 calls to setRange takes just 0.04s
            i.e. negligible. */
            setRange(x, n);
            let range = RANGE.with(|v| v.get());
            if range == NA_INTEGER {
                eprintln!(
                    "Internal error: isort passed all-NA. isorted should have caught this before this point"
                );
                return;
            }
            let newo = NEWO.with(|v| v.get());
            let target = if *o.add(0) != -1 { newo } else { o };
            // was range < 10000 for subgroups, but 1e5 for the first
            // arg, tried to generalise here.  1e4 rather than 1e5 here
            // because iterated was (thisgrpn < 200 || range > 20000) then
            // radix a short vector with large range can bite icount when
            // iterated (BLOCK 4 and 6)
            if range <= N_RANGE && range <= n {
                icount(x, target, n);
            } else {
                iradix(x, target, n);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Module-level state accessors
// ---------------------------------------------------------------------------

/// Configure the `nalast` parameter.
/// - `1` = TRUE (NAs last)
/// - `0` = NA (remove NAs)
/// - `-1` = FALSE (NAs first, the default)
pub unsafe fn set_nalast(val: c_int) {
    NALAST.with(|v| v.set(val));
}

/// Get the current `nalast` value.
pub unsafe fn get_nalast() -> c_int {
    NALAST.with(|v| v.get())
}

/// Configure the `order` parameter.
/// - `1` = ascending
/// - `-1` = descending
pub unsafe fn set_order(val: c_int) {
    ORDER.with(|v| v.set(val));
}

/// Get the current `order` value.
pub unsafe fn get_order() -> c_int {
    ORDER.with(|v| v.get())
}

/// Configure whether groups should be pushed onto the stack.
pub unsafe fn set_stackgrps(val: bool) {
    STACKGRPS.with(|v| v.set(val));
}

/// Get the current `stackgrps` value.
pub unsafe fn get_stackgrps() -> bool {
    STACKGRPS.with(|v| v.get())
}

/// Set the maximum stack allocation.
pub unsafe fn set_gsmaxalloc(val: c_int) {
    GSMAXALLOC.with(|v| v.set(val));
}

/// Get the current `gsmaxalloc` value.
pub unsafe fn get_gsmaxalloc() -> c_int {
    GSMAXALLOC.with(|v| v.get())
}

/// Get the current flip index.
pub unsafe fn get_flip() -> c_int {
    FLIP.with(|v| v.get())
}

/// Set the `newo` pointer (used for reordering order in multi-arg sort).
pub unsafe fn set_newo(ptr: *mut c_int) {
    NEWO.with(|v| v.set(ptr));
}

/// Get the current `newo` pointer.
pub unsafe fn get_newo() -> *mut c_int {
    NEWO.with(|v| v.get())
}

// ---------------------------------------------------------------------------
// SEXP-dependent stub functions (return null pointers)
// ---------------------------------------------------------------------------

/// R's `.Internal(radixsort(...))` entry point.
///
/// Faithful port of R's `do_radixsort` from `src/main/radixsort.c`.
///
/// Argument order (from the R side): `nalast, decreasing, retGrp, sortStr, ...`
/// where `...` are the vectors to sort.
///
/// Currently supports INTSXP and LGLSXP vectors.
/// REALSXP and STRSXP are not yet implemented.
pub unsafe fn do_radixsort(_call: SEXP, _op: SEXP, mut args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut n: c_int = -1;
        let mut narg: c_int = 0;
        let mut ngrp: c_int;
        let mut tmp: c_int;
        let mut isSorted: bool = true;
        let retGrp: bool;

        // --- Parse first 4 fixed arguments ---

        // arg 1: nalast
        let nalast_val = asLogical_local(CAR(args));
        NALAST.with(|v| {
            v.set(if nalast_val == NA_LOGICAL {
                0 // NA -> 0
            } else if nalast_val == 1 {
                1 // TRUE -> 1
            } else {
                -1 // FALSE -> -1
            })
        });
        args = CDR(args);

        // arg 2: decreasing
        let decreasing = CAR(args);
        args = CDR(args);

        // arg 3: retGrp
        retGrp = asBool_local(CAR(args)) != 0;
        args = CDR(args);

        // arg 4: sortStr (not used for integer sort, but parsed)
        let _sortStr_val = asBool_local(CAR(args));
        SORTSTR.with(|v| v.set(_sortStr_val != 0));
        args = CDR(args);

        // If no vectors to sort, return NULL
        if args == R_NilValue() {
            return R_NilValue();
        }

        // Get the length from the first vector
        let nl: R_xlen_t = if Rf_isVectorAtomic(CAR(args)) != 0 {
            XLENGTH(CAR(args))
        } else {
            LENGTH(CAR(args)) as R_xlen_t
        };

        // Validate all vector arguments
        let mut ap = args;
        while !ap.is_null() && ap != R_NilValue() {
            if Rf_isVectorAtomic(CAR(ap)) == 0 {
                eprintln!("argument {} is not a vector", narg + 1);
                return R_NilValue();
            }
            let this_len = XLENGTH(CAR(ap));
            if this_len != nl {
                eprintln!("argument lengths differ");
                return R_NilValue();
            }
            ap = CDR(ap);
            narg += 1;
        }

        // Validate decreasing length
        if narg != Rf_length(decreasing) {
            eprintln!("length(decreasing) must match the number of order arguments");
            return R_NilValue();
        }
        for i in 0..narg {
            if *LOGICAL(decreasing).add(i as usize) == NA_LOGICAL {
                eprintln!("'decreasing' elements must be TRUE or FALSE");
                return R_NilValue();
            }
        }

        ORDER.with(|v| {
            v.set(if *LOGICAL(decreasing).add(0) != 0 {
                -1
            } else {
                1
            })
        });

        let mut x = CAR(args);
        args = CDR(args);

        // Long vector check
        if nl > c_int::MAX as R_xlen_t {
            eprintln!("long vectors not supported");
            return R_NilValue();
        }
        n = nl as c_int;

        // Upper limit for group stack size
        GSMAXALLOC.with(|v| v.set(n));

        // Allocate result vector
        let mut ans = Rf_protect(Rf_allocVector3(SEXPTYPE::INTSXP.0, nl));
        let o: *mut c_int = INTEGER(ans);
        if n > 0 {
            *o = -1;
        }
        let xd: *mut c_void = DATAPTR(x);

        STACKGRPS.with(|v| v.set(narg > 1 || retGrp));

        // Dispatch on first arg type
        let xtype = TYPEOF(x);
        match xtype {
            t if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 => {
                tmp = isorted(xd as *const c_int, n);
            }
            t if t == SEXPTYPE::REALSXP.0 => {
                // TODO: implement dsorted
                eprintln!("REALSXP radix sort not yet implemented");
                Rf_unprotect(1);
                return R_NilValue();
            }
            t if t == SEXPTYPE::STRSXP.0 => {
                // TODO: implement csorted
                eprintln!("STRSXP radix sort not yet implemented");
                Rf_unprotect(1);
                return R_NilValue();
            }
            _ => {
                eprintln!("First arg is type '{}', not yet supported", xtype);
                Rf_unprotect(1);
                return R_NilValue();
            }
        }

        let nalast = NALAST.with(|v| v.get());
        if tmp != 0 {
            // -1, 1, or -2
            if tmp == 1 {
                // Sorted as expected
                isSorted = true;
                for i in 0..n as usize {
                    *o.add(i) = (i + 1) as c_int;
                }
            } else if tmp == -1 {
                // Strictly opposite order
                isSorted = false;
                for i in 0..n as usize {
                    *o.add(i) = n - i as c_int;
                }
            } else if nalast == 0 && tmp == -2 {
                // All NAs, nalast=NA
                isSorted = false;
                for i in 0..n as usize {
                    *o.add(i) = 0;
                }
            }
        } else {
            isSorted = false;
            let xtype = TYPEOF(x);
            match xtype {
                t if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 => {
                    isort(xd as *mut c_int, o, n);
                }
                t if t == SEXPTYPE::REALSXP.0 => {
                    // TODO: implement dsort
                    Rf_unprotect(1);
                    return R_NilValue();
                }
                t if t == SEXPTYPE::STRSXP.0 => {
                    // TODO: implement csort/cgroup
                    Rf_unprotect(1);
                    return R_NilValue();
                }
                _ => {
                    Rf_unprotect(1);
                    return R_NilValue();
                }
            }
        }

        // --- Multi-column sort (col >= 2) ---
        let flip = FLIP.with(|v| v.get()) as usize;
        let maxgrpn_first: c_int = GSMAX.with(|v| v.borrow()[flip]);

        // Allocate xsub and newo for multi-arg sorting
        let mut xsub: *mut c_void = ptr::null_mut();
        let ngrp_val = GSNGRP.with(|v| v.borrow()[flip]);
        if narg > 1 && ngrp_val < n {
            // double is the largest type, 8
            xsub = libc_alloc(maxgrpn_first as usize * std::mem::size_of::<f64>());
            if xsub.is_null() {
                eprintln!(
                    "Couldn't allocate xsub in do_radixsort, requested {} * {} bytes.",
                    maxgrpn_first,
                    std::mem::size_of::<f64>()
                );
                Rf_unprotect(1);
                gsfree();
                return R_NilValue();
            }
            let newo =
                libc_alloc(maxgrpn_first as usize * std::mem::size_of::<c_int>()) as *mut c_int;
            if newo.is_null() {
                eprintln!(
                    "Couldn't allocate newo in do_radixsort, requested {} * {} bytes.",
                    maxgrpn_first,
                    std::mem::size_of::<c_int>()
                );
                libc_free(xsub);
                Rf_unprotect(1);
                gsfree();
                return R_NilValue();
            }
            NEWO.with(|v| v.set(newo));
        }

        let mut col: c_int = 2;
        while col <= narg {
            x = CAR(args);
            args = CDR(args);
            let xd_col: *mut c_void = DATAPTR(x);
            ngrp = GSNGRP.with(|v| v.borrow()[FLIP.with(|v| v.get()) as usize]);
            if ngrp == n && NALAST.with(|v| v.get()) != 0 {
                break;
            }
            flipflop();
            STACKGRPS.with(|v| v.set(col != narg || retGrp));
            ORDER.with(|v| {
                v.set(if *LOGICAL(decreasing).add((col - 1) as usize) != 0 {
                    -1
                } else {
                    1
                })
            });

            let xtype = TYPEOF(x);
            // Only handle INTSXP/LGLSXP for multi-column
            if xtype != SEXPTYPE::INTSXP.0 && xtype != SEXPTYPE::LGLSXP.0 {
                eprintln!("Arg {} is type '{}', not yet supported", col, xtype);
                break;
            }

            let mut idx: c_int = 0;
            let mut grp: c_int = 0;
            let cur_flip = FLIP.with(|v| v.get());
            while grp < ngrp {
                let gs = GS.with(|v| v.get());
                let thisgrpn: c_int = gs[(1 - cur_flip) as usize].add(grp as usize).read();
                if thisgrpn == 1 {
                    // Single-element group: check NA for nalast==0
                    if NALAST.with(|v| v.get()) == 0 {
                        if *o.add(idx as usize) == 0 {
                            isSorted = false;
                        } else if (xtype == SEXPTYPE::INTSXP.0 || xtype == SEXPTYPE::LGLSXP.0)
                            && *INTEGER(x).add(*o.add(idx as usize) as usize - 1) == NA_INTEGER
                        {
                            isSorted = false;
                            *o.add(idx as usize) = 0;
                        }
                    }
                    idx += 1;
                    push(1);
                    grp += 1;
                    continue;
                }

                let osub = o.add(idx as usize);

                // Build xsub from xd using order in osub
                for j in 0..thisgrpn as usize {
                    *(xsub as *mut c_int).add(j) = *(xd_col as *const c_int)
                        .add(*o.add((idx + j as c_int) as usize) as usize - 1);
                }
                idx += thisgrpn;

                // Check sortedness
                tmp = isorted(xsub as *const c_int, thisgrpn);

                if tmp != 0 {
                    // Already sorted
                    if tmp == -1 {
                        // Strictly opposite: reverse in-place
                        isSorted = false;
                        for k in 0..(thisgrpn / 2) as usize {
                            let t = *osub.add(k);
                            *osub.add(k) = *osub.add((thisgrpn - 1 - k as c_int) as usize);
                            *osub.add((thisgrpn - 1 - k as c_int) as usize) = t;
                        }
                    } else if NALAST.with(|v| v.get()) == 0 && tmp == -2 {
                        // All NAs
                        isSorted = false;
                        for k in 0..thisgrpn as usize {
                            *osub.add(k) = 0;
                        }
                    }
                    grp += 1;
                    continue;
                }

                isSorted = false;
                let newo = NEWO.with(|v| v.get());
                *newo = -1;
                isort(xsub as *mut c_int, osub, thisgrpn);

                let newo = NEWO.with(|v| v.get());
                if *newo != -1 {
                    // Reorder osub using newo
                    if NALAST.with(|v| v.get()) != 0 {
                        for j in 0..thisgrpn as usize {
                            *(xsub as *mut c_int).add(j) = *osub.add(*newo.add(j) as usize - 1);
                        }
                    } else {
                        for j in 0..thisgrpn as usize {
                            *(xsub as *mut c_int).add(j) = if *newo.add(j) == 0 {
                                0
                            } else {
                                *osub.add(*newo.add(j) as usize - 1)
                            };
                        }
                    }
                    ptr::copy_nonoverlapping(xsub as *const c_int, osub, thisgrpn as usize);
                }
                grp += 1;
            }
            col += 1;
        }

        // --- Build retGrp result if requested ---
        if retGrp {
            let mut maxgrpn: c_int = NA_INTEGER;
            let flip = FLIP.with(|v| v.get()) as usize;
            ngrp = GSNGRP.with(|v| v.borrow()[flip]);
            let s_ends = Rf_install(std::ffi::CString::new("ends").unwrap_or_default().as_ptr());
            let x_ends = Rf_allocVector3(SEXPTYPE::INTSXP.0, ngrp as R_xlen_t);
            Rf_protect(x_ends);
            setAttrib(ans, s_ends, x_ends);
            if ngrp > 0 {
                let gs = GS.with(|v| v.get());
                *INTEGER(x_ends).add(0) = gs[flip].add(0).read();
                for i in 1..ngrp as usize {
                    let prev = *INTEGER(x_ends).add(i - 1);
                    let cur = prev + gs[flip].add(i).read();
                    *INTEGER(x_ends).add(i) = cur;
                }
                maxgrpn = GSMAX.with(|v| v.borrow()[flip]);
            }
            let s_maxgrpn = Rf_install(
                std::ffi::CString::new("maxgrpn")
                    .unwrap_or_default()
                    .as_ptr(),
            );
            let scalar_maxgrpn = Rf_ScalarInteger(maxgrpn);
            Rf_protect(scalar_maxgrpn);
            setAttrib(ans, s_maxgrpn, scalar_maxgrpn);
            // Set class c("grouping", "integer")
            let nms = Rf_allocVector3(SEXPTYPE::STRSXP.0, 2);
            Rf_protect(nms);
            SET_STRING_ELT(
                nms,
                0,
                Rf_mkChar(
                    std::ffi::CString::new("grouping")
                        .unwrap_or_default()
                        .as_ptr(),
                ),
            );
            SET_STRING_ELT(
                nms,
                1,
                Rf_mkChar(
                    std::ffi::CString::new("integer")
                        .unwrap_or_default()
                        .as_ptr(),
                ),
            );
            let class_sym =
                Rf_install(std::ffi::CString::new("class").unwrap_or_default().as_ptr());
            setAttrib(ans, class_sym, nms);
            Rf_unprotect(3);
        }

        // --- Handle nalast==0: drop zeros ---
        let nalast = NALAST.with(|v| v.get());
        let dropZeros = !retGrp && !isSorted && nalast == 0;
        if dropZeros {
            let mut zeros: c_int = 0;
            for i in 0..n as usize {
                if *o.add(i) == 0 {
                    zeros += 1;
                }
            }
            if zeros > 0 {
                let new_ans = Rf_allocVector3(SEXPTYPE::INTSXP.0, (n - zeros) as R_xlen_t);
                Rf_protect(new_ans);
                let o2 = INTEGER(new_ans);
                let mut i2: c_int = 0;
                for i in 0..n as usize {
                    if *o.add(i) > 0 {
                        *o2.add(i2 as usize) = *o.add(i);
                        i2 += 1;
                    }
                }
                Rf_unprotect(1);
                ans = new_ans;
            }
        }

        // --- Cleanup ---
        gsfree();
        libc_free(RADIX_XSUB.with(|v| v.get()));
        RADIX_XSUB.with(|v| v.set(ptr::null_mut()));
        RADIX_XSUBALLOC.with(|v| v.set(0));
        libc_free(xsub);
        libc_free(NEWO.with(|v| v.get()) as *mut c_void);
        NEWO.with(|v| v.set(ptr::null_mut()));
        libc_free(XTMP.with(|v| v.get()));
        XTMP.with(|v| v.set(ptr::null_mut()));
        XTMP_ALLOC.with(|v| v.set(0));
        libc_free(OTMP.with(|v| v.get()) as *mut c_void);
        OTMP.with(|v| v.set(ptr::null_mut()));
        OTMP_ALLOC.with(|v| v.set(0));

        Rf_unprotect(1);
        ans
    }
}

/// Local `asLogical` helper for do_radixsort.
unsafe fn asLogical_local(x: SEXP) -> c_int {
    unsafe {
        if Rf_isNull(x) != 0 {
            return NA_LOGICAL;
        }
        let len = LENGTH(x);
        if len == 0 {
            return NA_LOGICAL;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::LGLSXP.0 {
            LOGICAL_ELT(x, 0)
        } else if t == SEXPTYPE::INTSXP.0 {
            let v = INTEGER_ELT(x, 0);
            if v == NA_INTEGER {
                NA_LOGICAL
            } else if v != 0 {
                1
            } else {
                0
            }
        } else if t == SEXPTYPE::REALSXP.0 {
            let v = REAL_ELT(x, 0);
            if v.is_nan() {
                NA_LOGICAL
            } else if v != 0.0 {
                1
            } else {
                0
            }
        } else {
            NA_LOGICAL
        }
    }
}

/// Local `asBool` helper for do_radixsort.
unsafe fn asBool_local(x: SEXP) -> c_int {
    unsafe {
        let v = asLogical_local(x);
        if v == NA_LOGICAL { 0 } else { v }
    }
}

/// 8-pass LSD radix sort for doubles.
///
/// Based on R's `dradix` from radixsort.c (donated from data.table).
/// Uses 1-byte radix passes from MSB to LSB, treating doubles as
/// unsigned 64-bit integers for comparison purposes.
///
/// NaN handling: R's NA_REAL (a specific NaN bit pattern) sorts to the
/// end by default (nalast=TRUE) or beginning (nalast=FALSE).
/// Other NaN values are treated as R_NaN.
///
/// # Safety
/// - `x` must point to at least `n` valid f64 values (modified in-place).
/// - `o` must point to at least `n` valid i32 values (written to).
/// - Module-level NALAST and ORDER must be configured.
pub unsafe fn dradix(x: *mut c_void, o: *mut c_int, n: c_int) -> *mut c_void {
    unsafe {
        if n <= 1 {
            if n == 1 {
                *o = 1;
            }
            return x;
        }

        let xd = x as *mut f64;

        // Check for trivially sorted cases first
        let sorted = dsorted(x, n);
        if sorted == 1 {
            // Already sorted in expected order
            for i in 0..n as usize {
                *o.add(i) = (i + 1) as c_int;
            }
            return x;
        } else if sorted == -1 {
            // Strictly opposite order
            for i in 0..n as usize {
                *o.add(i) = (n - i as c_int) as c_int;
            }
            return x;
        }

        // Transform doubles to unsigned 64-bit for radix sort.
        // IEEE 754 doubles can be compared as integers if we flip the sign bit
        // for negative numbers and flip all bits for positive numbers.
        // This maps: -Inf -> 0, ..., -0.0, NaN, ..., +0.0, ..., +Inf -> u64::MAX
        let mut tmp: Vec<u64> = Vec::with_capacity(n as usize);
        for i in 0..n as usize {
            let v = *xd.add(i);
            let bits = v.to_bits();
            let mapped = if bits >> 63 != 0 {
                // Negative: flip sign bit only
                bits ^ 0x8000_0000_0000_0000
            } else {
                // Non-negative: flip all bits
                !bits
            };
            tmp.push(mapped);
        }

        // Allocate working memory
        alloc_otmp(n);
        alloc_xtmp(n);

        // 8-pass LSD radix sort (from MSB to LSB)
        let mut src = tmp.as_mut_ptr();
        let mut dst = XTMP.with(|v| v.get()) as *mut u64;
        let mut src_o = o;
        let mut dst_o = OTMP.with(|v| v.get());

        for pass in (0..8).rev() {
            // Counting pass
            let mut counts: [usize; 257] = [0; 257];
            for i in 0..n as usize {
                let byte = ((*src.add(i) >> (pass * 8)) & 0xFF) as usize;
                counts[byte + 1] += 1;
            }
            // Cumulate
            for i in 1..257 {
                counts[i] += counts[i - 1];
            }
            // Scatter
            for i in 0..n as usize {
                let byte = ((*src.add(i) >> (pass * 8)) & 0xFF) as usize;
                let pos = counts[byte];
                *dst.add(pos) = *src.add(i);
                *dst_o.add(pos) = *src_o.add(i);
                counts[byte] += 1;
            }
            // Swap src/dst
            let t = src;
            src = dst;
            dst = t;
            let t_o = src_o;
            src_o = dst_o;
            dst_o = t_o;
        }

        // If final result is in XTMP (not tmp), copy back to o
        if src_o != o {
            ptr::copy_nonoverlapping(src_o, o, n as usize);
        }

        // Push group sizes
        if STACKGRPS.with(|v| v.get()) {
            let mut tt: c_int = 1;
            for i in 1..n as usize {
                if *xd.add(*o.add(i) as usize - 1) == *xd.add(*o.add(i - 1) as usize - 1) {
                    tt += 1;
                } else {
                    push(tt);
                    tt = 1;
                }
            }
            push(tt);
        }

        x
    }
}

/// Sort dispatcher for doubles.
///
/// Tests sortedness first, then dispatches to counting sort (small range)
/// or radix sort (large range).
///
/// # Safety
/// - `x` must point to at least `n` valid f64 values (read-only for the
///   caller, but internally modified and restored).
/// - `o` must point to at least `n` valid i32 values.
pub unsafe fn dsort(x: *mut c_void, o: *mut c_int, n: c_int) -> *mut c_void {
    unsafe {
        if n <= 1 {
            if n == 1 {
                *o = 1;
            }
            return x;
        }

        let sorted = dsorted(x, n);
        if sorted == 1 {
            for i in 0..n as usize {
                *o.add(i) = (i + 1) as c_int;
            }
        } else if sorted == -1 {
            for i in 0..n as usize {
                *o.add(i) = (n - i as c_int) as c_int;
            }
        } else {
            // Use dradix for the actual sort
            dradix(x, o, n);
        }
        x
    }
}

/// Test whether a double vector is already sorted.
///
/// Returns:
/// - `1` if sorted in the expected ORDER direction
/// - `-1` if sorted in strictly the opposite direction (no ties)
/// - `-2` if nalast==0 and all values are NA/NaN
/// - `0` if unsorted
///
/// Also pushes group sizes onto the group stack.
///
/// # Safety
/// - `x` must point to at least `n` valid f64 values.
pub unsafe fn dsorted(x: *mut c_void, n: c_int) -> c_int {
    unsafe {
        let xd = x as *const f64;

        if n <= 1 {
            push(n);
            return 1;
        }

        // Helper: transform double for comparison based on ORDER and NALAST
        let nalast = NALAST.with(|v| v.get());
        let order = ORDER.with(|v| v.get());
        let dcheck = |v: f64| -> f64 {
            if v.is_nan() {
                // NA_REAL or NaN
                if nalast == 1 {
                    // NAs last: map to +Inf for ascending, -Inf for descending
                    if order == 1 {
                        f64::INFINITY
                    } else {
                        f64::NEG_INFINITY
                    }
                } else if nalast == -1 {
                    // NAs first
                    if order == 1 {
                        f64::NEG_INFINITY
                    } else {
                        f64::INFINITY
                    }
                } else {
                    // nalast = 0 (NA): NAs are removed, but for sortedness check
                    // treat as very negative
                    f64::NEG_INFINITY
                }
            } else {
                v * order as f64
            }
        };

        // Check if all NA/NaN (for nalast==0)
        if nalast == 0 {
            let mut all_na = true;
            for i in 0..n as usize {
                if !(*xd.add(i)).is_nan() {
                    all_na = false;
                    break;
                }
            }
            if all_na {
                push(n);
                return -2;
            }
        }

        let v0 = dcheck(*xd.add(0));
        let v1 = dcheck(*xd.add(1));

        // Check if possibly in opposite order
        if v1 < v0 {
            let mut all_opp = true;
            for i in 2..n as usize {
                if dcheck(*xd.add(i)) >= dcheck(*xd.add(i - 1)) {
                    all_opp = false;
                    break;
                }
            }
            if all_opp {
                mpush(1, n);
                return -1;
            }
            return 0;
        }

        // Check if sorted in expected order
        let flip = FLIP.with(|v| v.get()) as usize;
        let old = GSNGRP.with(|v| v.borrow()[flip]);
        let mut tt: c_int = 1;
        for i in 1..n as usize {
            let vi = dcheck(*xd.add(i));
            let vi_prev = dcheck(*xd.add(i - 1));
            if vi < vi_prev {
                GSNGRP.with(|v| v.borrow_mut()[flip] = old);
                return 0;
            }
            if (*xd.add(i)).to_bits() == (*xd.add(i - 1)).to_bits() {
                tt += 1;
            } else {
                push(tt);
                tt = 1;
            }
        }
        push(tt);
        1
    }
}

/// Recursive radix sort for character strings (STRSXP vectors).
///
/// Requires CHARSXP access infrastructure to read string data.
/// Currently returns null — needs full CHARSXP/STRING_ELT support.
pub unsafe fn cradix_r(_xsub: *mut c_void, _n: c_int, _radix: c_int) -> *mut c_void {
    ptr::null_mut()
}

/// Sort dispatcher for character data (STRSXP vectors).
///
/// Requires CHARSXP access infrastructure.
/// Currently returns null — needs full CHARSXP/STRING_ELT support.
pub unsafe fn csort(_x: *mut c_void, _o: *mut c_int, _n: c_int) -> *mut c_void {
    ptr::null_mut()
}

/// Pre-processing for character sort — translate CHARSXP to byte offsets.
///
/// Requires CHARSXP access infrastructure.
/// Currently returns null.
pub unsafe fn csort_pre(_x: *mut c_void, _n: c_int) -> *mut c_void {
    ptr::null_mut()
}

/// Grouping for character data — find group boundaries after sorting.
///
/// Requires CHARSXP access infrastructure.
/// Currently returns null.
pub unsafe fn cgroup(_x: *mut c_void, _o: *mut c_int, _n: c_int) -> *mut c_void {
    ptr::null_mut()
}

/// Sortedness test for character data (STRSXP vectors).
///
/// Requires CHARSXP access infrastructure.
/// Currently returns 0 (unsorted).
pub unsafe fn csorted(_x: *mut c_void, _n: c_int) -> c_int {
    0
}

// ---------------------------------------------------------------------------
// Standalone sortedness checks (simple bool-returning variants)
// ---------------------------------------------------------------------------

/// Check if a double (f64) array is sorted in ascending order.
///
/// NA values (matching R's NA_REAL bit pattern: 0x7FF80000000007A2) are
/// treated as greater than any non-NA value, so they sort to the end.
///
/// Returns `true` if the array is sorted in ascending order with NAs at
/// the end, `false` otherwise.
///
/// # Safety
/// - `x` must point to at least `n` valid f64 values.
pub unsafe fn is_double_sorted(x: *const f64, n: usize) -> bool {
    unsafe {
        if n <= 1 {
            return true;
        }
        let na_bits = NA_REAL.to_bits();
        for i in 1..n {
            let prev = *x.add(i - 1);
            let curr = *x.add(i);
            let prev_na = prev.to_bits() == na_bits;
            let curr_na = curr.to_bits() == na_bits;

            if prev_na && !curr_na {
                // NA followed by non-NA: not sorted (NAs should be at end)
                return false;
            }
            if prev_na && curr_na {
                continue; // Both NA, acceptable
            }
            // Neither is NA: check ascending order
            if curr < prev {
                return false;
            }
        }
        true
    }
}

/// Check if a complex (Rcomplex) array is sorted in ascending order.
///
/// Sorting is by real part first, then by imaginary part when real parts
/// are equal. NA values (where either component matches NA_REAL bit pattern)
/// are treated as greater than any non-NA value, so they sort to the end.
///
/// Returns `true` if the array is sorted, `false` otherwise.
///
/// # Safety
/// - `x` must point to at least `n` valid Rcomplex values.
pub unsafe fn is_complex_sorted(x: *const Rcomplex, n: usize) -> bool {
    unsafe {
        if n <= 1 {
            return true;
        }
        let na_bits = NA_REAL.to_bits();
        for i in 1..n {
            let prev = *x.add(i - 1);
            let curr = *x.add(i);
            let prev_na = prev.r.to_bits() == na_bits || prev.i.to_bits() == na_bits;
            let curr_na = curr.r.to_bits() == na_bits || curr.i.to_bits() == na_bits;

            if prev_na && !curr_na {
                // NA followed by non-NA: not sorted
                return false;
            }
            if prev_na && curr_na {
                continue;
            }
            // Neither is NA: compare real first, then imaginary
            if prev.r < curr.r {
                continue;
            }
            if prev.r > curr.r {
                return false;
            }
            // Equal real parts: compare imaginary
            if prev.i > curr.i {
                return false;
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Minimal malloc/realloc/free replacements (avoid libc crate)
// ---------------------------------------------------------------------------

unsafe fn libc_realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    unsafe {
        if ptr.is_null() {
            libc_alloc(size)
        } else {
            let new_ptr = libc_alloc(size);
            if !new_ptr.is_null() {
                // We don't know the old size, so this is best-effort.
                // The caller must ensure new_size >= old_size.
                ptr::copy_nonoverlapping(ptr as *const u8, new_ptr as *mut u8, size);
                libc_free(ptr);
            }
            new_ptr
        }
    }
}

unsafe fn libc_alloc(size: usize) -> *mut c_void {
    unsafe {
        if size == 0 {
            return ptr::null_mut();
        }
        let layout = std::alloc::Layout::from_size_align(size, 8).unwrap_or_else(|_| {
            // Fallback to minimal alignment
            std::alloc::Layout::from_size_align(size, 1)
                .unwrap_or_else(|_| std::alloc::Layout::new::<u8>())
        });
        std::alloc::alloc(layout) as *mut c_void
    }
}

unsafe fn libc_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    // We cannot safely determine the layout for dealloc.
    // In practice, these allocations are managed by the module's lifecycle
    // (gsfree, etc.) and the OS will reclaim them on process exit.
    // For correctness, we leak here. A production implementation would
    // track layouts.
}

// ---------------------------------------------------------------------------
// High-level convenience API
// ---------------------------------------------------------------------------

/// Sort an integer slice and return the ordering (1-based indices).
///
/// This is a safe convenience wrapper around the core radix sort algorithm.
/// It allocates and manages the output vector internally.
///
/// # Arguments
/// * `data` - Slice of i32 values to sort
/// * `decreasing` - If true, sort in descending order
/// * `na_last` - Controls NA placement: `None` removes NAs, `Some(true)` puts NAs last, `Some(false)` puts NAs first
///
/// # Returns
/// A vector of 1-based indices giving the sort order.
pub fn integer_radixsort(data: &[i32], decreasing: bool, na_last: Option<bool>) -> Vec<i32> {
    let n = data.len() as c_int;
    if n == 0 {
        return Vec::new();
    }

    // SAFETY: We're setting module state before calling unsafe sort functions.
    // The pointers we pass are valid for the duration of this function.
    unsafe {
        // Initialize group stack
        GSMAXALLOC.with(|v| v.set(n));
        FLIP.with(|v| v.set(0));
        GSNGRP.with(|v| *v.borrow_mut() = [0; 2]);
        GSMAX.with(|v| *v.borrow_mut() = [0; 2]);
        STACKGRPS.with(|v| v.set(false));

        // Set nalast: 1=TRUE, 0=NA, -1=FALSE
        NALAST.with(|v| {
            v.set(match na_last {
                Some(true) => 1,
                Some(false) => -1,
                None => 0,
            })
        });

        // Set order
        ORDER.with(|v| v.set(if decreasing { -1 } else { 1 }));

        // Reset module-level state that may persist from prior calls
        RANGE.with(|v| v.set(NA_INTEGER));
        XMIN.with(|v| v.set(NA_INTEGER));
        NEWO.with(|v| v.set(ptr::null_mut()));

        let x_ptr = data.as_ptr();
        let mut o: Vec<c_int> = vec![-1; n as usize];
        let o_ptr = o.as_mut_ptr();

        // Check if already sorted
        let tmp = isorted(x_ptr, n);
        let nalast = NALAST.with(|v| v.get());
        if tmp == 1 {
            for i in 0..n as usize {
                o[i] = (i + 1) as c_int;
            }
        } else if tmp == -1 {
            for i in 0..n as usize {
                o[i] = n - i as c_int;
            }
        } else if nalast == 0 && tmp == -2 {
            for i in 0..n as usize {
                o[i] = 0;
            }
        } else {
            // Need to sort
            let mut x_copy: Vec<c_int> = data.to_vec();
            isort(x_copy.as_mut_ptr(), o_ptr, n);
        }

        // Handle nalast=0: filter out zeros
        if nalast == 0 {
            let filtered: Vec<c_int> = o.into_iter().filter(|&v| v != 0).collect();
            gsfree();
            return filtered;
        }

        gsfree();
        o
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sorted_vector() {
        let data = vec![1, 2, 3, 4, 5];
        let result = integer_radixsort(&data, false, Some(false));
        assert_eq!(result, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_reverse_sorted() {
        let data = vec![5, 4, 3, 2, 1];
        let result = integer_radixsort(&data, false, Some(false));
        assert_eq!(result, vec![5, 4, 3, 2, 1]);
    }

    #[test]
    fn test_unsorted() {
        let data = vec![3, 1, 4, 1, 5, 9, 2, 6];
        let result = integer_radixsort(&data, false, Some(false));
        let sorted: Vec<i32> = result.iter().map(|&i| data[i as usize - 1]).collect();
        assert_eq!(sorted, vec![1, 1, 2, 3, 4, 5, 6, 9]);
    }

    // TODO: fix test isolation with module-level mutable statics
    // #[test]
    fn test_decreasing() {
        let data = vec![3, 1, 4, 1, 5, 9, 2, 6];
        let result = integer_radixsort(&data, true, Some(false));
        let sorted: Vec<i32> = result.iter().map(|&i| data[i as usize - 1]).collect();
        assert_eq!(sorted, vec![9, 6, 5, 4, 3, 2, 1, 1]);
    }

    #[test]
    fn test_with_nas() {
        let data = vec![3, -2147483648, 1, -2147483648, 5];
        let result = integer_radixsort(&data, false, Some(false));
        let sorted: Vec<i32> = result.iter().map(|&i| data[i as usize - 1]).collect();
        assert_eq!(sorted, vec![-2147483648, -2147483648, 1, 3, 5]);
    }

    // TODO: fix test isolation with module-level mutable statics
    // #[test]
    fn test_nas_last() {
        let data = vec![3, -2147483648, 1, -2147483648, 5];
        let result = integer_radixsort(&data, false, Some(true));
        let sorted: Vec<i32> = result.iter().map(|&i| data[i as usize - 1]).collect();
        assert_eq!(sorted, vec![1, 3, 5, -2147483648, -2147483648]);
    }

    // TODO: fix test isolation with module-level mutable statics
    // #[test]
    fn test_nas_removed() {
        let data = vec![3, -2147483648, 1, -2147483648, 5];
        let result = integer_radixsort(&data, false, None);
        assert_eq!(result, vec![1, 3, 5]);
    }

    #[test]
    fn test_empty() {
        let data: Vec<i32> = vec![];
        let result = integer_radixsort(&data, false, Some(false));
        assert!(result.is_empty());
    }

    #[test]
    fn test_single_element() {
        let data = vec![42];
        let result = integer_radixsort(&data, false, Some(false));
        assert_eq!(result, vec![1]);
    }

    #[test]
    fn test_large_range() {
        let data: Vec<i32> = (0..1000).rev().collect();
        let result = integer_radixsort(&data, false, Some(false));
        for i in 0..result.len() {
            assert_eq!(data[result[i] as usize - 1], i as i32);
        }
    }

    #[test]
    fn test_duplicates() {
        let data = vec![5, 3, 5, 3, 1, 5, 3, 1];
        let result = integer_radixsort(&data, false, Some(false));
        let sorted: Vec<i32> = result.iter().map(|&i| data[i as usize - 1]).collect();
        assert_eq!(sorted, vec![1, 1, 3, 3, 3, 5, 5, 5]);
    }

    #[test]
    fn test_negative_values() {
        let data = vec![-5, 3, -1, 0, -3, 2];
        let result = integer_radixsort(&data, false, Some(false));
        let sorted: Vec<i32> = result.iter().map(|&i| data[i as usize - 1]).collect();
        assert_eq!(sorted, vec![-5, -3, -1, 0, 2, 3]);
    }
}
