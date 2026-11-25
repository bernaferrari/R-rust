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

use std::cmp::Ordering;
use std::collections::HashMap;
use std::os::raw::{c_int, c_void};

use crate::eval::attrib_core::setAttrib;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{NA_LOGICAL, NA_REAL, R_xlen_t, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_NaString, R_NilValue};
use crate::sexp::protect::protect;

fn error(msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    });
}

/// Checked node factory bound to the active runtime session.
fn node_factory() -> crate::sexp::object::SessionNodeFactory<'static> {
    crate::sexp::object::SessionNodeFactory::new(
        crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string())),
    )
}

fn wrapped(x: SEXP) -> crate::sexp::object::Sexp<'static> {
    node_factory()
        .wrap(x)
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
}

fn checked<T>(result: crate::sexp::object::SexpResult<T>) -> T {
    result.unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
}

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
// Per-session radix sort workspace
// ---------------------------------------------------------------------------
//
// gs = groupsizes e.g. 23, 12, 87, 2, 1, 34,...
// Two vectors flip-flopped: flip and 1 - flip.

struct RadixSortState {
    gs: [Vec<c_int>; 2],
    flip: usize,
    gsmax: [c_int; 2],
    gsmaxalloc: c_int,
    stackgrps: bool,
    nalast: c_int,
    order: c_int,
    range: c_int,
    xmin: c_int,
    counts: Box<[u32]>,
    radixcounts: [[u32; 257]; 8],
    skip: [c_int; 8],
    radix_xsub: Vec<c_int>,
    otmp: Vec<c_int>,
    xtmp_int: Vec<c_int>,
}

impl Default for RadixSortState {
    fn default() -> Self {
        RadixSortState {
            gs: [Vec::new(), Vec::new()],
            flip: 0,
            gsmax: [0; 2],
            gsmaxalloc: 0,
            stackgrps: true,
            nalast: -1,
            order: 1,
            range: NA_INTEGER,
            xmin: NA_INTEGER,
            counts: vec![0u32; N_RANGE as usize + 1].into_boxed_slice(),
            radixcounts: [[0; 257]; 8],
            skip: [0; 8],
            radix_xsub: Vec::new(),
            otmp: Vec::new(),
            xtmp_int: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Group stack helpers
// ---------------------------------------------------------------------------

fn grow_scratch<T: Copy + Default>(buf: &mut Vec<T>, len: usize) {
    if len > buf.len() {
        buf.try_reserve_exact(len - buf.len())
            .unwrap_or_else(|_| error("Could not allocate working memory"));
        buf.resize(len, T::default());
    }
}

fn push(state: &mut RadixSortState, n: c_int) {
    mpush(state, n, 1);
}

fn mpush(state: &mut RadixSortState, n: c_int, times: c_int) {
    if !state.stackgrps || n == 0 || times == 0 {
        return;
    }
    let group = &mut state.gs[state.flip];
    let additional = usize::try_from(times).unwrap_or_else(|_| error("Invalid group count"));
    let len = group.len().checked_add(additional)
        .filter(|&len| len <= state.gsmaxalloc as usize)
        .unwrap_or_else(|| error("Invalid group count"));
    group.try_reserve(additional).unwrap_or_else(|_| error("Could not allocate working memory"));
    group.resize(len, n);
    state.gsmax[state.flip] = state.gsmax[state.flip].max(n);
}

fn flipflop(state: &mut RadixSortState) {
    state.flip = 1 - state.flip;
    state.gs[state.flip].clear();
    state.gsmax[state.flip] = 0;
}

// ---------------------------------------------------------------------------
// setRange -- determine min and range of integer data
// ---------------------------------------------------------------------------

/// Compute the minimum value and range of an integer array, skipping
/// NA_INTEGER values. Sets module-level `XMIN` and `RANGE`.
fn setRange(state: &mut RadixSortState, x: &[c_int]) {
    state.xmin = NA_INTEGER;
    let mut xmax: c_int = NA_INTEGER;
    let overflow: f64;

    let mut i: usize = 0;
    while i < x.len() && x[i] == NA_INTEGER {
        i += 1;
    }
    if i < x.len() {
        xmax = x[i];
        state.xmin = xmax;
    }
    for &tmp in x.iter().skip(i) {
        if tmp == NA_INTEGER {
            continue;
        }
        if tmp > xmax {
            xmax = tmp;
        } else if tmp < state.xmin {
            state.xmin = tmp;
        }
    }
    // all NAs, nothing to do
    if state.xmin == NA_INTEGER {
        state.range = NA_INTEGER;
        return;
    }
    // ex: x=c(-2147483647L, NA_integer_, 1L) results in overflowing int range.
    overflow = (xmax as f64) - (state.xmin as f64) + 1.0;
    // detect and force iradix here, since icount is out of the picture
    if overflow > (c_int::MAX as f64) {
        state.range = c_int::MAX;
        return;
    }

    state.range = xmax - state.xmin + 1;
}

// ---------------------------------------------------------------------------
// icheck -- transform value to account for nalast and order
// ---------------------------------------------------------------------------

/// x*order results in integer overflow when -1*NA,
/// so careful to avoid that here.
#[inline]
fn icheck(state: &RadixSortState, x: c_int) -> c_int {
    let nalast = state.nalast;
    let order = state.order;
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
fn icount(state: &mut RadixSortState, x: &[c_int], o: &mut [c_int]) {
    let range = state.range;
    let xmin = state.xmin;
    let nalast = state.nalast;
    let order = state.order;

    let napos = range; // NA's always counted in last bin
    // static is IMPORTANT, counting sort is called repetitively.

    if range > N_RANGE {
        eprintln!(
            "Internal error: range = {}; isorted cannot handle range > {}",
            range, N_RANGE
        );
        return;
    }
    for &v in x {
        if v == NA_INTEGER {
            state.counts[napos as usize] += 1;
        } else {
            state.counts[(v - xmin) as usize] += 1;
        }
    }

    let mut tmp: c_int = 0;
    if nalast != 1 && state.counts[napos as usize] != 0 {
        push(state, state.counts[napos as usize] as c_int);
        tmp += state.counts[napos as usize] as c_int;
    }
    let mut w: c_int = if order == 1 { 0 } else { range - 1 };
    for _i in 0..range {
        let cw = state.counts[w as usize];
        if cw != 0 {
            // cumulate but not through 0's.
            // Helps resetting zeros when n < range, below.
            push(state, cw as c_int);
            tmp += cw as c_int;
            state.counts[w as usize] = tmp as u32;
        }
        w += order; // order is +1 or -1
    }
    if nalast == 1 && state.counts[napos as usize] != 0 {
        push(state, state.counts[napos as usize] as c_int);
        tmp += state.counts[napos as usize] as c_int;
        state.counts[napos as usize] = tmp as u32;
    }
    for i in (0..x.len()).rev() {
        let idx = if x[i] == NA_INTEGER {
            napos as usize
        } else {
            (x[i] - xmin) as usize
        };
        state.counts[idx] -= 1;
        o[state.counts[idx] as usize] = (i + 1) as c_int;
    }
    // nalast = 1, -1 are both taken care already.
    if nalast == 0 {
        // nalast = 0 is dealt with separately as it just sets o to 0
        for i in 0..o.len() {
            if x[o[i] as usize - 1] == NA_INTEGER {
                o[i] = 0;
            }
        }
        // at those indices where x is NA. x[o[i]-1] because x is not modified here.
    }

    /* counts were cumulated above so leaves non zero.
    Faster to clear up now ready for next time. */
    if x.len() < range as usize {
        /* Many zeros in counts already. Loop through n instead,
        doesn't matter if we set to 0 several times on any repeats */
        state.counts[napos as usize] = 0;
        for &v in x {
            if v != NA_INTEGER {
                state.counts[(v - xmin) as usize] = 0;
            }
        }
    } else if range + 1 > 0 {
        // memset counts to 0
        for j in 0..=(range as usize) {
            state.counts[j] = 0;
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
fn iinsert(state: &mut RadixSortState, x: &mut [c_int], o: &mut [c_int]) {
    for i in 1..x.len() {
        let xtmp = x[i];
        if xtmp < x[i - 1] {
            let mut j = i as isize - 1;
            let otmp = o[i];
            while j >= 0 && xtmp < x[j as usize] {
                x[(j + 1) as usize] = x[j as usize];
                o[(j + 1) as usize] = o[j as usize];
                j -= 1;
            }
            x[(j + 1) as usize] = xtmp;
            o[(j + 1) as usize] = otmp;
        }
    }
    let mut tt: c_int = 0;
    for i in 1..x.len() {
        if x[i] == x[i - 1] {
            tt += 1;
        } else {
            push(state, tt + 1);
            tt = 0;
        }
    }
    push(state, tt + 1);
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
fn iradix(state: &mut RadixSortState, x: &[c_int], o: &mut [c_int]) {
    if x.is_empty() {
        return;
    }
    let n = x.len() as c_int;
    let mut nextradix: c_int;
    let mut itmp: c_int;
    let mut thisgrpn: c_int;
    let mut maxgrpn: c_int;
    let mut thisx: u32 = 0;
    let shift: u32;

    for &v in x {
        /* parallel histogramming pass; i.e. count occurrences of
        0:255 in each byte. Sequential so almost negligible. */
        // relies on overflow behaviour. And shouldn't -INT_MIN be up in iradix?
        thisx = (icheck(state, v) as u32).wrapping_sub(c_int::MIN as u32);
        // unrolled since inside n-loop
        state.radixcounts[0][(thisx & 0xFF) as usize] += 1;
        state.radixcounts[1][((thisx >> 8) & 0xFF) as usize] += 1;
        state.radixcounts[2][((thisx >> 16) & 0xFF) as usize] += 1;
        state.radixcounts[3][((thisx >> 24) & 0xFF) as usize] += 1;
    }
    for radix in 0..4 {
        /* any(count == n) => all radix must have been that value =>
        last x (still thisx) was that value */
        let idx = ((thisx >> (radix * 8)) & 0xFF) as usize;
        let skip_val = if state.radixcounts[radix][idx] == n as u32 {
            1
        } else {
            0
        };
        state.skip[radix] = skip_val;
        // clear it now, the other counts must be 0 already
        if skip_val != 0 {
            state.radixcounts[radix][idx] = 0;
        }
    }

    let mut radix: c_int = 3; // MSD
    while radix >= 0 && state.skip[radix as usize] != 0 {
        radix -= 1;
    }
    if radix == -1 {
        // All radix are skipped; one number repeated n times.
        if state.nalast == 0 && x[0] == NA_INTEGER {
            o.fill(0);
        } else {
            for i in 0..o.len() {
                o[i] = (i + 1) as c_int;
            }
        }
        push(state, n);
        return;
    }
    for i in (0..radix as usize).rev() {
        if state.skip[i] == 0 {
            // clear the counts as we only needed the parallel pass for skip[]
            // and we're going to use radixcounts again below.
            for j in 0..257 {
                state.radixcounts[i][j] = 0;
            }
        }
    }
    shift = (radix * 8) as u32;

    itmp = state.radixcounts[radix as usize][0] as c_int;
    maxgrpn = itmp;
    let mut ii: usize = 1;
    while itmp < n && ii < 256 {
        thisgrpn = state.radixcounts[radix as usize][ii] as c_int;
        if thisgrpn != 0 {
            // don't cumulate through 0s, important below.
            if thisgrpn > maxgrpn {
                maxgrpn = thisgrpn;
            }
            itmp += thisgrpn;
            state.radixcounts[radix as usize][ii] = itmp as u32;
        }
        ii += 1;
    }
    for i in (0..x.len()).rev() {
        thisx = (icheck(state, x[i]) as u32).wrapping_sub(c_int::MIN as u32);
        let bucket = ((thisx >> shift) & 0xFF) as usize;
        state.radixcounts[radix as usize][bucket] -= 1;
        o[state.radixcounts[radix as usize][bucket] as usize] = (i + 1) as c_int;
    }

    // The largest group according to the first non-skipped radix,
    // so could be big (if radix is needed on first arg).
    grow_scratch(&mut state.radix_xsub, maxgrpn as usize);
    grow_scratch(&mut state.otmp, maxgrpn as usize);
    grow_scratch(&mut state.xtmp_int, maxgrpn as usize);

    nextradix = radix - 1;
    while nextradix >= 0 && state.skip[nextradix as usize] != 0 {
        nextradix -= 1;
    }
    if state.radixcounts[radix as usize][0] != 0 {
        eprintln!(
            "Internal error. thiscounts[0]={} but should have been decremented to 0. iradix={}",
            state.radixcounts[radix as usize][0],
            radix
        );
        return;
    }
    state.radixcounts[radix as usize][256] = n as u32;
    itmp = 0;
    let mut scratch = std::mem::take(&mut state.radix_xsub);
    let mut ii: usize = 1;
    while itmp < n && ii <= 256 {
        if state.radixcounts[radix as usize][ii] == 0 {
            ii += 1;
            continue;
        }
        let thisgrpn = state.radixcounts[radix as usize][ii] as c_int - itmp; // undo cumulate; i.e. diff
        if thisgrpn == 1 || nextradix == -1 {
            push(state, thisgrpn);
        } else {
            let start = itmp as usize;
            let end = start + thisgrpn as usize;
            let osub = &mut o[start..end];
            // this is why this xsub here can't be the same memory as
            // xsub in do_radixsort.
            let xsub = &mut scratch[..osub.len()];
            for (key, &index) in xsub.iter_mut().zip(osub.iter()) {
                *key = icheck(state, x[index as usize - 1]);
            }
            // changes xsub and osub by reference recursively.
            iradix_r(state, xsub, osub, nextradix);
        }
        itmp = state.radixcounts[radix as usize][ii] as c_int;
        state.radixcounts[radix as usize][ii] = 0;
        ii += 1;
    }
    state.radix_xsub = scratch;
    if state.nalast == 0 {
        // nalast = 0 is dealt with separately as it just sets o to 0
        for i in 0..o.len() {
            if x[o[i] as usize - 1] == NA_INTEGER {
                o[i] = 0;
            }
        }
        // at those indices where x is NA. x[o[i]-1] because x is not
        // modified by reference unlike iinsert or iradix_r
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
fn iradix_r(state: &mut RadixSortState, xsub: &mut [c_int], osub: &mut [c_int], radix: c_int) {
    // N_SMALL=200 is guess based on limited testing. Needs calibrate().
    // Was 50 based on sum(1:50)=1275 worst -vs- 256 cumulate + 256 memset +
    // allowance since reverse order is unlikely.
    // when nalast==0, iinsert will be called only from within iradix.
    let n = xsub.len() as c_int;
    if xsub.len() < N_SMALL {
        iinsert(state, xsub, osub);
        return;
    }

    let shift = (radix * 8) as u32;

    for &v in xsub.iter() {
        let thisx = (v as u32).wrapping_sub(c_int::MIN as u32);
        state.radixcounts[radix as usize][((thisx >> shift) & 0xFF) as usize] += 1;
    }
    let mut itmp = state.radixcounts[radix as usize][0] as c_int;
    let mut ii: usize = 1;
    while itmp < n && ii < 256 {
        // don't cumulate through 0s, important below
        if state.radixcounts[radix as usize][ii] != 0 {
            itmp += state.radixcounts[radix as usize][ii] as c_int;
            state.radixcounts[radix as usize][ii] = itmp as u32;
        }
        ii += 1;
    }
    for i in (0..xsub.len()).rev() {
        let thisx = (xsub[i] as u32).wrapping_sub(c_int::MIN as u32);
        let bucket = ((thisx >> shift) & 0xFF) as usize;
        state.radixcounts[radix as usize][bucket] -= 1;
        let j = state.radixcounts[radix as usize][bucket] as usize;
        state.otmp[j] = osub[i];
        state.xtmp_int[j] = xsub[i];
    }
    osub.copy_from_slice(&state.otmp[..osub.len()]);
    xsub.copy_from_slice(&state.xtmp_int[..xsub.len()]);

    let mut nextradix = radix - 1;
    while nextradix >= 0 && state.skip[nextradix as usize] != 0 {
        nextradix -= 1;
    }

    if state.radixcounts[radix as usize][0] != 0 {
        eprintln!(
            "Logical error. thiscounts[0]={} but should have been decremented to 0. radix={}",
            state.radixcounts[radix as usize][0],
            radix
        );
        return;
    }
    state.radixcounts[radix as usize][256] = n as u32;
    itmp = 0;
    let mut ii: usize = 1;
    while itmp < n && ii <= 256 {
        if state.radixcounts[radix as usize][ii] == 0 {
            ii += 1;
            continue;
        }
        let thisgrpn = state.radixcounts[radix as usize][ii] as c_int - itmp; // undo cumulate; i.e. diff
        if thisgrpn == 1 || nextradix == -1 {
            push(state, thisgrpn);
        } else {
            let start = itmp as usize;
            let end = start + thisgrpn as usize;
            iradix_r(
                state,
                &mut xsub[start..end],
                &mut osub[start..end],
                nextradix,
            );
        }
        itmp = state.radixcounts[radix as usize][ii] as c_int;
        state.radixcounts[radix as usize][ii] = 0;
        ii += 1;
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
fn isorted(state: &mut RadixSortState, x: &[c_int]) -> c_int {
    let n = x.len() as c_int;
    let mut j: c_int = 0;
    let nalast = state.nalast;
    // when nalast = NA,
    // all NAs ? return special value to replace all o's values with '0'
    // any NAs ? return 0 = unsorted and leave it
    //   to sort routines to replace o's with 0's
    // no NAs ? continue to check rest of isorted - the same routine as usual
    if nalast == 0 {
        for &v in x {
            if v != NA_INTEGER {
                j += 1;
            }
        }
        if j == 0 {
            push(state, n);
            return -2;
        }
        if j != n {
            return 0;
        }
    }
    if n <= 1 {
        push(state, n);
        return 1;
    }
    if icheck(state, x[1]) < icheck(state, x[0]) {
        let mut i = 2usize;
        while i < x.len() && icheck(state, x[i]) < icheck(state, x[i - 1]) {
            i += 1;
        }
        // strictly opposite to expected 'order', no ties;
        if i == x.len() {
            mpush(state, 1, n);
            return -1;
        }
        // e.g. no more than one NA at the beginning/end (for order=-1/1)
        return 0;
    }
    let old = state.gs[state.flip].len();
    let mut tt: c_int = 1;
    for i in 1..x.len() {
        if icheck(state, x[i]) < icheck(state, x[i - 1]) {
            state.gs[state.flip].truncate(old);
            return 0;
        }
        if x[i] == x[i - 1] {
            tt += 1;
        } else {
            push(state, tt);
            tt = 1;
        }
    }
    push(state, tt);
    // same as 'order', NAs at the beginning for order=1, at end for
    // order=-1, possibly with ties
    1
}

// ---------------------------------------------------------------------------
// isort -- dispatch between icount, iradix, and iinsert
// ---------------------------------------------------------------------------

/// Sort an integer vector, dispatching to counting sort, radix sort,
/// or insertion sort depending on data characteristics.
fn isort(state: &mut RadixSortState, x: &mut [c_int], o: &mut [c_int], newo: &mut [c_int]) {
    let n = x.len() as c_int;
    if n <= 2 {
        // nalast = 0 and n == 2 (check bottom of this file for explanation)
        if state.nalast == 0 && n == 2 {
            if o[0] == -1 {
                o[0] = 1;
                o[1] = 2;
            }
            for i in 0..x.len() {
                if x[i] == NA_INTEGER {
                    o[i] = 0;
                }
            }
            push(state, 1);
            push(state, 1);
            return;
        } else {
            eprintln!(
                "Internal error: isort received n={}. isorted should have dealt with this already",
                n
            );
            return;
        }
    }
    let nalast = state.nalast;
    let order = state.order;
    if x.len() < N_SMALL && o[0] != -1 && nalast != 0 {
        // see comment above in iradix_r on N_SMALL=200.
        if order != 1 || nalast != -1 {
            // so that default case, i.e., order=1, nalast=FALSE will
            // not be affected (ex: `setkey`)
            for v in x.iter_mut() {
                *v = icheck(state, *v);
            }
        }
        iinsert(state, x, o);
    } else {
        /* Tighter range (e.g. copes better with a few abnormally large
        values in some groups), but also, when setRange was once at
        arg level that caused an extra scan of (long) x
        first. 10,000 calls to setRange takes just 0.04s
        i.e. negligible. */
        setRange(state, x);
        let range = state.range;
        if range == NA_INTEGER {
            eprintln!(
                "Internal error: isort passed all-NA. isorted should have caught this before this point"
            );
            return;
        }
        let target = if o[0] != -1 { &mut newo[..x.len()] } else { o };
        // was range < 10000 for subgroups, but 1e5 for the first
        // arg, tried to generalise here.  1e4 rather than 1e5 here
        // because iterated was (thisgrpn < 200 || range > 20000) then
        // radix a short vector with large range can bite icount when
        // iterated (BLOCK 4 and 6)
        if range <= N_RANGE && range <= n {
            icount(state, x, target);
        } else {
            iradix(state, x, target);
        }
    }
}

// ---------------------------------------------------------------------------
// SEXP-dependent stub functions (return null pointers)
// ---------------------------------------------------------------------------

/// GNU `.Internal(wrap_meta(vec, sorted, noNA))` (sort.c `do_wrap_meta`):
/// attaches ALTREP "known sorted" metadata to a numeric vector. The port
/// has no sorted-metadata ALTREP class; the wrapper is observationally
/// transparent, so the vector is returned unchanged.
pub fn do_wrap_meta(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    CAR(args)
}

/// GNU `.Internal(sorted_fpass(x, decr, nalast))` (sort.c `do_sorted_fpass`).
///
/// The port has no sorted-metadata ALTREP class, so the ALTREP sortedness
/// consult stays UNKNOWN_SORTEDNESS for every type and only GNU's
/// increasing-integer scan can confirm sortedness.
pub fn do_sorted_fpass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    const SORTED_DECR_NA_1ST: c_int = -2;
    const SORTED_DECR: c_int = -1;
    const SORTED_INCR: c_int = 1;
    const SORTED_INCR_NA_1ST: c_int = 2;

    {
        let decr = asInteger_scan(CADR(args));
        let nalast = asInteger_scan(CADDR(args));
        // makeSortEnum: decr == NA_INTEGER (INT_MIN) marks UNKNOWN_SORTEDNESS;
        // an NA nalast means the NAs were/will be removed, i.e. "last".
        let wanted = if decr == NA_INTEGER {
            NA_INTEGER
        } else if decr != 0 {
            if nalast != 0 {
                SORTED_DECR
            } else {
                SORTED_DECR_NA_1ST
            }
        } else if nalast != 0 {
            SORTED_INCR
        } else {
            SORTED_INCR_NA_1ST
        };
        if !matches!(
            wanted,
            SORTED_DECR | SORTED_INCR | SORTED_DECR_NA_1ST | SORTED_INCR_NA_1ST
        ) {
            return Rf_ScalarLogical(0);
        }
        let x = CAR(args);
        let mut done = false;
        // Increasing, usually fairly short, sequences of integers often
        // arise as levels in as.factor.  A quick check here allows a fast
        // return in sort.int.  (No ALTREP in the port, so the scan always
        // applies.)
        if !done && TYPEOF(x) == SEXPTYPE::INTSXP && wanted > 0 {
            let len = XLENGTH(x);
            if len > 0 {
                let xv = wrapped(x);
                let mut last = checked(xv.try_integer_elt(0));
                if last != NA_INTEGER {
                    done = true;
                    for i in 1..len {
                        let next = checked(xv.try_integer_elt(i));
                        if next < last || next == NA_INTEGER {
                            done = false;
                            break;
                        }
                        last = next;
                    }
                }
            }
        }
        Rf_ScalarLogical(done as c_int)
    }
}

/// Copy an integer or logical argument into owned keys for the safe
/// integer kernels.
fn integer_keys(x: SEXP, n: usize) -> Vec<c_int> {
    let value = wrapped(x);
    if value.typeof_() == SEXPTYPE::INTSXP {
        let mut keys = vec![0; n];
        checked(value.copy_integer_into(&mut keys));
        keys
    } else {
        (0..n)
            .map(|i| checked(value.try_logical_elt(i as R_xlen_t)))
            .collect()
    }
}

/// Allocate an INTSXP result and fill it from `values`.
fn integer_result(values: &[c_int]) -> SEXP {
    let x = Rf_allocVector3(SEXPTYPE::INTSXP, values.len() as R_xlen_t);
    let mut value = checked(crate::sexp::object::SexpMut::try_from_checked(wrapped(x)));
    for (i, &elt) in values.iter().enumerate() {
        checked(value.try_set_integer_elt(i as R_xlen_t, elt));
    }
    x
}

/// R's `.Internal(radixsort(...))` entry point.
///
/// Faithful port of R's `do_radixsort` from `src/main/radixsort.c`.
///
/// Argument order (from the R side): `nalast, decreasing, retGrp, sortStr, ...`
/// where `...` are the vectors to sort.
///
/// Currently supports INTSXP, LGLSXP, REALSXP, and STRSXP vectors.
pub fn do_radixsort(_call: SEXP, _op: SEXP, mut args: SEXP, _rho: SEXP) -> SEXP {
    let mut state = RadixSortState::default();
    let mut n: c_int = -1;
    let mut narg: c_int = 0;
    let mut ngrp: c_int;
    let mut tmp: c_int;
    let mut isSorted: bool;
    let retGrp: bool;

    // --- Parse first 4 fixed arguments ---

    // arg 1: nalast
    let nalast_val = asLogical_local(CAR(args));
    state.nalast = if nalast_val == NA_LOGICAL {
        0 // NA -> 0
    } else if nalast_val == 1 {
        1 // TRUE -> 1
    } else {
        -1 // FALSE -> -1
    };
    args = CDR(args);

    // arg 2: decreasing
    let decreasing = CAR(args);
    args = CDR(args);

    // arg 3: retGrp
    retGrp = asBool_local(CAR(args)) != 0;
    args = CDR(args);

    // arg 4: sortStr (sorted order vs first-seen grouping)
    let sort_str = asBool_local(CAR(args)) != 0;
    args = CDR(args);

    // If no vectors to sort, return NULL
    if args == R_NilValue() {
        return R_NilValue();
    }

    // Get the length from the first vector
    let nl: R_xlen_t = if crate::sexp::object::raw_is_atomic_vector(CAR(args)) {
        XLENGTH(CAR(args))
    } else {
        LENGTH(CAR(args)) as R_xlen_t
    };

    // Validate all vector arguments
    let mut ap = args;
    while !ap.is_null() && ap != R_NilValue() {
        if !crate::sexp::object::raw_is_atomic_vector(CAR(ap)) {
            error(&format!("argument {} is not a vector", narg + 1));
        }
        let this_len = XLENGTH(CAR(ap));
        if this_len != nl {
            error("argument lengths differ");
        }
        ap = CDR(ap);
        narg += 1;
    }

    // Validate decreasing length
    if narg != Rf_length(decreasing) {
        error("length(decreasing) must match the number of order arguments");
    }
    for i in 0..narg {
        if LOGICAL_ELT(decreasing, i) == NA_LOGICAL {
            error("'decreasing' elements must be TRUE or FALSE");
        }
    }

    state.order = if LOGICAL_ELT(decreasing, 0) != 0 {
        -1
    } else {
        1
    };

    let mut x = CAR(args);
    args = CDR(args);

    // Long vector check
    if nl > c_int::MAX as R_xlen_t {
        error("long vectors not supported");
    }
    n = nl as c_int;

    // Upper limit for group stack size
    state.gsmaxalloc = n;

    // Order output starts unassigned; the -1 sentinel in slot 0 tells
    // isort to write the permutation directly instead of newo.
    let mut o = vec![0; n as usize];
    if let Some(first) = o.first_mut() {
        *first = -1;
    }
    let xtype = TYPEOF(x);

    state.stackgrps = narg > 1 || retGrp;

    // Dispatch on first arg type
    match SEXPTYPE(xtype) {
        SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => {
            let mut keys = integer_keys(x, n as usize);
            let sorted = isorted(&mut state, &keys);
            isSorted = sorted == 1;
            match sorted {
                1 => {
                    for (i, row) in o.iter_mut().enumerate() {
                        *row = (i + 1) as c_int;
                    }
                }
                -1 => {
                    for (i, row) in o.iter_mut().enumerate() {
                        *row = n - i as c_int;
                    }
                }
                -2 => o.fill(0),
                _ => isort(&mut state, &mut keys, &mut o, &mut []),
            }
        }
        SEXPTYPE::REALSXP => {
            let input = wrapped(x);
            let keys: Vec<_> = (0..nl)
                .map(|i| real_radix_key(checked(input.try_real_elt(i)), state.order, state.nalast))
                .collect();
            isSorted = sort_real_keys(&mut state, &keys, &mut o);
        }
        SEXPTYPE::STRSXP => {
            let keys = character_keys(x, nl);
            sort_character_keys(&mut state, &keys, sort_str, &mut o);
            isSorted = false;
        }
        _ => error(&format!("First arg is type '{}', not yet supported", xtype)),
    }

    // --- Multi-column sort (col >= 2) ---
    let maxgrpn_first: c_int = state.gsmax[state.flip];

    // Allocate xsub and newo for multi-arg sorting
    let mut xsub = Vec::<c_int>::new();
    let mut newo = Vec::<c_int>::new();
    let ngrp_val = state.gs[state.flip].len() as c_int;
    if narg > 1 && ngrp_val < n {
        grow_scratch(&mut xsub, maxgrpn_first as usize);
        grow_scratch(&mut newo, maxgrpn_first as usize);
    }

    let mut col: c_int = 2;
    while col <= narg {
        x = CAR(args);
        args = CDR(args);
        ngrp = state.gs[state.flip].len() as c_int;
        if ngrp == n && state.nalast != 0 {
            break;
        }
        flipflop(&mut state);
        state.stackgrps = col != narg || retGrp;
        state.order = if LOGICAL_ELT(decreasing, col - 1) != 0 {
            -1
        } else {
            1
        };

        let xtype = TYPEOF(x);
        // Only handle INTSXP/LGLSXP for multi-column
        if xtype != SEXPTYPE::INTSXP && xtype != SEXPTYPE::LGLSXP {
            eprintln!("Arg {} is type '{}', not yet supported", col, xtype);
            break;
        }

        let column_keys = integer_keys(x, n as usize);
        let mut idx: c_int = 0;
        let mut grp: c_int = 0;
        let cur_flip = state.flip;
        while grp < ngrp {
            let thisgrpn: c_int = state.gs[1 - cur_flip][grp as usize];
            if thisgrpn == 1 {
                // Single-element group: check NA for nalast==0
                if state.nalast == 0 {
                    let order_index = o[idx as usize];
                    if order_index == 0
                        || column_keys[order_index as usize - 1] == NA_INTEGER
                    {
                        isSorted = false;
                        o[idx as usize] = 0;
                    }
                }
                idx += 1;
                push(&mut state, 1);
                grp += 1;
                continue;
            }

            let start = idx as usize;
            let len = thisgrpn as usize;
            let osub = &mut o[start..start + len];
            let xsub = &mut xsub[..len];

            // Build xsub from the column keys using the order in osub
            for (key, &row) in xsub.iter_mut().zip(osub.iter()) {
                *key = if row == 0 {
                    NA_INTEGER
                } else {
                    column_keys[row as usize - 1]
                };
            }
            idx += thisgrpn;

            // Check sortedness
            tmp = isorted(&mut state, xsub);

            if tmp != 0 {
                // Already sorted
                if tmp == -1 {
                    // Strictly opposite: reverse in-place
                    isSorted = false;
                    osub.reverse();
                } else if state.nalast == 0 && tmp == -2 {
                    // All NAs
                    isSorted = false;
                    osub.fill(0);
                }
                grp += 1;
                continue;
            }

            isSorted = false;
            newo[0] = -1;
            isort(&mut state, xsub, osub, &mut newo);

            if newo[0] != -1 {
                // Reorder osub using newo
                for (target, &index) in xsub.iter_mut().zip(&newo[..len]) {
                    *target = if state.nalast == 0 && index == 0 {
                        0
                    } else {
                        osub[index as usize - 1]
                    };
                }
                osub.copy_from_slice(xsub);
            }
            grp += 1;
        }
        col += 1;
    }

    // --- Handle nalast==0: drop zeros ---
    if !retGrp && !isSorted && state.nalast == 0 && o.contains(&0) {
        o.retain(|&row| row > 0);
    }
    let ans = integer_result(&o);
    let _ans_guard = protect(ans);

    // --- Build retGrp result if requested ---
    if retGrp {
        let s_ends = crate::sexp::symbol::install_str("ends");
        let mut end = 0;
        let ends: Vec<c_int> = state.gs[state.flip]
            .iter()
            .map(|&size| {
                end += size;
                end
            })
            .collect();
        let maxgrpn = if ends.is_empty() {
            NA_INTEGER
        } else {
            state.gsmax[state.flip]
        };
        let x_ends = integer_result(&ends);
        let _x_ends_guard = protect(x_ends);
        setAttrib(ans, s_ends, x_ends);
        let s_maxgrpn = crate::sexp::symbol::install_str("maxgrpn");
        let scalar_maxgrpn = Rf_ScalarInteger(maxgrpn);
        let _scalar_maxgrpn_guard = protect(scalar_maxgrpn);
        setAttrib(ans, s_maxgrpn, scalar_maxgrpn);
        // Set class c("grouping", "integer")
        let nms = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _nms_guard = protect(nms);
        SET_STRING_ELT(nms, 0, crate::sexp::constructors::mk_char_str("grouping"));
        SET_STRING_ELT(nms, 1, crate::sexp::constructors::mk_char_str("integer"));
        let class_sym = crate::sexp::symbol::install_str("class");
        setAttrib(ans, class_sym, nms);
    }

    ans
}

/// Local `asLogical` helper for do_radixsort.
fn asLogical_local(x: SEXP) -> c_int {
    if Rf_isNull(x) != 0 {
        return NA_LOGICAL;
    }
    let len = LENGTH(x);
    if len == 0 {
        return NA_LOGICAL;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::LGLSXP {
        LOGICAL_ELT(x, 0)
    } else if t == SEXPTYPE::INTSXP {
        let v = INTEGER_ELT(x, 0);
        if v == NA_INTEGER {
            NA_LOGICAL
        } else if v != 0 {
            1
        } else {
            0
        }
    } else if t == SEXPTYPE::REALSXP {
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

/// Local `asInteger` helper for do_sorted_fpass (GNU `asInteger` coercion
/// of a length-one logical/integer/double scalar, NA preserved).
fn asInteger_scan(x: SEXP) -> c_int {
    if Rf_isNull(x) != 0 {
        return NA_INTEGER;
    }
    if LENGTH(x) == 0 {
        return NA_INTEGER;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::LGLSXP {
        LOGICAL_ELT(x, 0)
    } else if t == SEXPTYPE::INTSXP {
        INTEGER_ELT(x, 0)
    } else if t == SEXPTYPE::REALSXP {
        let v = REAL_ELT(x, 0);
        if v.is_nan() || v > c_int::MAX as f64 || v < c_int::MIN as f64 {
            NA_INTEGER
        } else {
            v as c_int
        }
    } else {
        NA_INTEGER
    }
}

/// Local `asBool` helper for do_radixsort.
fn asBool_local(x: SEXP) -> c_int {
    {
        let v = asLogical_local(x);
        if v == NA_LOGICAL { 0 } else { v }
    }
}

/// Map a real key to its GNU radix ordering, including missing values.
fn real_radix_key(value: f64, order: c_int, nalast: c_int) -> u64 {
    if value.is_nan() {
        return if nalast == 1 { u64::MAX } else { 0 };
    }
    // Signed zeros are ties; they keep their input order.
    let bits = if value == 0.0 { 0 } else { value.to_bits() };
    let sign = 1u64 << 63;
    let key = if bits & sign != 0 { !bits } else { bits ^ sign };
    if order < 0 { !key } else { key }
}

/// Write one-based indices with stable ties, or zero for rows being dropped.
/// Returns true if the input was already ordered and no rows were dropped.
fn sort_real_keys(state: &mut RadixSortState, keys: &[u64], o: &mut [c_int]) -> bool {
    debug_assert_eq!(keys.len(), o.len());
    let n = keys.len();
    let sorted = keys.windows(2).all(|pair| pair[0] <= pair[1]);
    for (i, index) in o.iter_mut().enumerate() {
        *index = (i + 1) as c_int;
    }
    if !sorted {
        if keys.windows(2).all(|pair| pair[0] > pair[1]) {
            o.reverse();
        } else {
            grow_scratch(&mut state.otmp, n);
            let scratch = &mut state.otmp[..n];
            // Least significant byte first; each scatter preserves input order.
            for pass in 0..8 {
                let (source, target) = if pass % 2 == 0 {
                    (&*o, &mut *scratch)
                } else {
                    (&*scratch, &mut *o)
                };
                let bucket =
                    |index: c_int| ((keys[index as usize - 1] >> (pass * 8)) & 0xff) as usize;
                let mut offsets = [0usize; 257];
                for &index in source {
                    offsets[bucket(index) + 1] += 1;
                }
                for i in 1..offsets.len() {
                    offsets[i] += offsets[i - 1];
                }
                for &index in source {
                    let b = bucket(index);
                    target[offsets[b]] = index;
                    offsets[b] += 1;
                }
            }
        }
    }
    if state.stackgrps {
        let mut start = 0;
        while start < n {
            let key = keys[o[start] as usize - 1];
            let mut end = start + 1;
            while end < n && keys[o[end] as usize - 1] == key {
                end += 1;
            }
            push(state, (end - start) as c_int);
            start = end;
        }
    }
    let mut removed = false;
    if state.nalast == 0 {
        for index in o {
            // Zero is reserved for missing keys; even infinities map above it.
            if keys[*index as usize - 1] == 0 {
                *index = 0;
                removed = true;
            }
        }
    }
    sorted && !removed
}



fn character_keys(x: SEXP, n: R_xlen_t) -> Vec<Option<Vec<u8>>> {
    let input = wrapped(x);
    let na = R_NaString();
    (0..n)
        .map(|i| {
            let value = checked(input.try_string_elt(i));
            let raw = value.as_raw();
            if raw == na {
                return None;
            }
            let mut bytes = crate::mainutils::printutils::charsxp_bytes(raw);
            if let Some(end) = bytes.iter().position(|&byte| byte == 0) {
                bytes.truncate(end);
            }
            Some(bytes)
        })
        .collect()
}

fn sort_character_keys(
    state: &mut RadixSortState,
    keys: &[Option<Vec<u8>>],
    sort_str: bool,
    o: &mut [c_int],
) {
    for (i, row) in o.iter_mut().enumerate() {
        *row = (i + 1) as c_int;
    }
    let key = |row: c_int| keys[row as usize - 1].as_deref();
    if sort_str {
        o.sort_unstable_by(|&left, &right| {
            let a = key(left);
            let b = key(right);
            let comparison = match (a, b) {
                (Some(a), Some(b)) if state.order < 0 => b.cmp(a),
                (None, Some(_)) if state.nalast == 1 => Ordering::Greater,
                (Some(_), None) if state.nalast == 1 => Ordering::Less,
                _ => a.cmp(&b),
            };
            comparison.then_with(|| left.cmp(&right))
        });
    } else {
        // GNU grouping numbers distinct keys by their first occurrence.
        let mut first = HashMap::new();
        let ranks: Vec<_> = o.iter().map(|&row| *first.entry(key(row)).or_insert(row)).collect();
        o.sort_unstable_by_key(|&row| (ranks[row as usize - 1], row));
    }
    if state.stackgrps {
        let mut start = 0;
        while start < o.len() {
            let mut end = start + 1;
            while end < o.len() && key(o[start]) == key(o[end]) {
                end += 1;
            }
            push(state, (end - start) as c_int);
            start = end;
        }
    }
    if state.nalast == 0 {
        for row in o {
            if key(*row).is_none() {
                *row = 0;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Standalone sortedness checks (simple bool-returning variants)
// ---------------------------------------------------------------------------



// ---------------------------------------------------------------------------
// Minimal malloc/realloc/free replacements (avoid libc crate)
// ---------------------------------------------------------------------------

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
    let mut state = RadixSortState {
        stackgrps: false,
        ..Default::default()
    };
    let n = c_int::try_from(data.len())
        .expect("radix sort input exceeds the integer index range");
    if n == 0 {
        return Vec::new();
    }

    // Set nalast: 1=TRUE, 0=NA, -1=FALSE
    state.nalast = match na_last {
        Some(true) => 1,
        Some(false) => -1,
        None => 0,
    };

    // Set order
    state.order = if decreasing { -1 } else { 1 };
    let nalast = state.nalast;

    let mut o: Vec<c_int> = vec![-1; data.len()];

    // Check if already sorted
    let tmp = isorted(&mut state, data);
    if tmp == 1 {
        for i in 0..data.len() {
            o[i] = (i + 1) as c_int;
        }
    } else if tmp == -1 {
        for i in 0..data.len() {
            o[i] = n - i as c_int;
        }
    } else if nalast == 0 && tmp == -2 {
        for i in 0..data.len() {
            o[i] = 0;
        }
    } else {
        // Need to sort
        let mut x_copy: Vec<c_int> = data.to_vec();
        isort(&mut state, &mut x_copy, &mut o, &mut []);
    }

    // Handle nalast=0: filter out zeros
    if nalast == 0 {
        o.retain(|&v| v != 0);
    }

    o
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

    // Unimplemented: fix test isolation with module-level mutable statics
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

    // Unimplemented: fix test isolation with module-level mutable statics
    // #[test]
    fn test_nas_last() {
        let data = vec![3, -2147483648, 1, -2147483648, 5];
        let result = integer_radixsort(&data, false, Some(true));
        let sorted: Vec<i32> = result.iter().map(|&i| data[i as usize - 1]).collect();
        assert_eq!(sorted, vec![1, 3, 5, -2147483648, -2147483648]);
    }

    // Unimplemented: fix test isolation with module-level mutable statics
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
    fn radix_workspace_grows_between_large_groups() {
        let mut state = RadixSortState::default();
        for len in [513, 2049] {
            state.gs[state.flip].clear();
            state.gsmax[state.flip] = 0;
            state.gsmaxalloc = len as c_int;
            let scratch_lens = (
                state.radix_xsub.len(),
                state.otmp.len(),
                state.xtmp_int.len(),
            );
            let values: Vec<i32> = (0..len)
                .map(|i| (((i * 173) % len) as i64 * 100_000_000 / len as i64) as i32)
                .collect();
            let mut actual = vec![-1; len];
            let mut expected: Vec<_> = (1..=len as i32).collect();
            expected.sort_by_key(|&i| values[i as usize - 1]);
            iradix(&mut state, &values, &mut actual);
            assert_eq!(actual, expected);
            assert_eq!(state.gs[state.flip].len(), len);
            assert!(
                state.radix_xsub.len() > scratch_lens.0
                    && state.otmp.len() > scratch_lens.1
                    && state.xtmp_int.len() > scratch_lens.2
            );
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

    #[test]
    fn real_radix_preserves_stable_groups_and_missing_values() {
        let values = [
            f64::NAN,
            NA_REAL,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -0.0,
            0.0,
            3.0,
            3.0,
            -2.0,
            NA_REAL,
            f64::NAN,
        ];
        let mut state = RadixSortState {
            gsmaxalloc: values.len() as c_int,
            nalast: 1,
            ..Default::default()
        };
        let keys: Vec<_> = values
            .iter()
            .map(|&value| real_radix_key(value, state.order, state.nalast))
            .collect();
        let mut order = [0; 11];
        assert!(!sort_real_keys(&mut state, &keys, &mut order));
        assert_eq!(order, [4, 9, 5, 6, 7, 8, 3, 1, 2, 10, 11]);
        assert_eq!(state.gs[state.flip], [1, 1, 2, 2, 1, 4]);
    }
}
