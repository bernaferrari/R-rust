#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/match.c — string matching utilities.
//!
//! This module ports the partial and exact string matching functions
//! used by R for argument matching, `pmatch()`, `match()`, `charmatch()`, etc.
//!
//! Ported functions from match.c:
//!   psmatch, NonNullStringMatch, pmatch (SEXP version), charFromSexp,
//!   matchPar_int, matchPar, matchArg, matchArgExact,
//!   matchArgs_NR, matchArgs_RC, patchArgsByActuals
//!
//! Ported functions from unique.c:
//!   do_match, do_pmatch, do_charmatch

use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::{FALSE, NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::*;
use crate::sexp::memory_ext::{CONS_NR, allocList, mkPROMISE};
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::R_DotsSymbol;

// ---------------------------------------------------------------------------
// Local helper macros and functions
// ---------------------------------------------------------------------------

/// ARGUSED(x) == LEVELS(x) — uses gp[0..1] bits.
#[inline(always)]
unsafe fn ARGUSED(x: SEXP) -> c_int {
    unsafe { LEVELS(x) }
}

/// SET_ARGUSED(x, v) == SETLEVELS(x, v).
#[inline(always)]
unsafe fn SET_ARGUSED(x: SEXP, v: c_int) {
    unsafe {
        SETLEVELS(x, v);
    }
}

/// SET_TAG equivalent (R macro). Uses existing SETTAG.
#[inline(always)]
unsafe fn SET_TAG(x: SEXP, y: SEXP) {
    unsafe {
        SETTAG(x, y);
    }
}

/// SET_TYPEOF equivalent (R macro).
#[inline(always)]
unsafe fn SET_TYPEOF(x: SEXP, v: SEXPTYPE) {
    unsafe {
        (*x).sxpinfo.set_type(v);
    }
}

/// SET_MISSING equivalent (R macro). Sets gp[2] bit.
/// In R, MISSING(x) is gp[2], and SET_MISSING(x,v) sets it.
/// gp bits 0-1 = LEVELS/ARGUSED, bit 2 = MISSING.
#[inline(always)]
unsafe fn SET_MISSING(x: SEXP, v: c_int) {
    unsafe {
        if x.is_null() {
            return;
        }
        let gp = if v != 0 {
            (*x).sxpinfo.gp() | 0x04
        } else {
            (*x).sxpinfo.gp() & !0x04
        };
        (*x).sxpinfo.set_gp(gp);
    }
}

/// String equality check (streql).
#[inline(always)]
unsafe fn streql(s1: *const c_char, s2: *const c_char) -> c_int {
    unsafe {
        if s1.is_null() || s2.is_null() {
            return if s1.is_null() && s2.is_null() { 1 } else { 0 };
        }
        if libc::strcmp(s1, s2) == 0 { 1 } else { 0 }
    }
}

#[inline(always)]
unsafe fn NA_STRING() -> SEXP { unsafe {
    crate::mainutils::relop::NA_STRING()
}}

#[inline(always)]
unsafe fn isNA_STRING(s: SEXP) -> bool {
    if s.is_null() {
        return true;
    }
    let gp = unsafe { (*s).sxpinfo.gp() };
    gp & 1 != 0
}

/// isString check — STRSXP type.
#[inline(always)]
unsafe fn isString(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::STRSXP.0 }
}

/// isVector check.
#[inline(always)]
unsafe fn isVector(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        matches!(t, 10 | 13 | 14 | 15 | 16 | 19 | 20 | 24)
    }
}

/// isNull check.
#[inline(always)]
unsafe fn isNull(x: SEXP) -> bool {
    unsafe { x.is_null() || TYPEOF(x) == SEXPTYPE::NILSXP.0 }
}

/// IS_BYTES check — delegates to sexp::accessors.
#[inline(always)]
unsafe fn IS_BYTES(s: SEXP) -> c_int { unsafe {
    crate::sexp::accessors::IS_BYTES(s)
}}

/// ENC_KNOWN check — delegates to sexp::accessors.
#[inline(always)]
unsafe fn ENC_KNOWN(s: SEXP) -> c_int { unsafe {
    crate::sexp::accessors::ENC_KNOWN(s)
}}

/// IS_CACHED check — stub, always returns true.
#[inline(always)]
unsafe fn IS_CACHED(_s: SEXP) -> c_int {
    1
}

#[inline(always)]
unsafe fn translateChar(s: SEXP) -> *const c_char { unsafe {
    crate::sexp::accessors::translateChar(s)
}}

#[inline(always)]
unsafe fn translateCharUTF8(s: SEXP) -> *const c_char { unsafe {
    crate::sexp::accessors::translateCharUTF8(s)
}}

#[inline(always)]
unsafe fn getCharCE(s: SEXP) -> c_int { unsafe {
    crate::sexp::accessors::getCharCE(s)
}}

/// checkArity — stub, no-op.
#[inline(always)]
unsafe fn checkArity(op: SEXP, args: SEXP) { unsafe { crate::mainutils::relop::checkArity(op, args) }}

/// asInteger — extract integer value from scalar.
#[inline(always)]
unsafe fn asInteger(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return NA_INTEGER;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::INTSXP.0 {
            let p = INTEGER(x);
            if p.is_null() {
                return NA_INTEGER;
            }
            *p
        } else if t == SEXPTYPE::LGLSXP.0 {
            let p = LOGICAL(x);
            if p.is_null() {
                return NA_INTEGER;
            }
            *p
        } else if t == SEXPTYPE::REALSXP.0 {
            let p = REAL(x);
            if p.is_null() {
                return NA_INTEGER;
            }
            let v = *p;
            if v.is_nan() {
                return NA_INTEGER;
            }
            v as c_int
        } else {
            NA_INTEGER
        }
    }
}

/// asLogical — extract logical value from scalar.
#[inline(always)]
unsafe fn asLogical(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return NA_INTEGER;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::LGLSXP.0 {
            let p = LOGICAL(x);
            if p.is_null() {
                return NA_INTEGER;
            }
            *p
        } else if t == SEXPTYPE::INTSXP.0 {
            let p = INTEGER(x);
            if p.is_null() {
                return NA_INTEGER;
            }
            *p
        } else {
            NA_INTEGER
        }
    }
}

/// isObject check.
#[inline(always)]
unsafe fn isObject(x: SEXP) -> bool {
    unsafe { OBJECT(x) != 0 }
}

/// ScalarInteger — allocate a scalar integer.
#[inline(always)]
unsafe fn ScalarInteger(x: c_int) -> SEXP {
    unsafe { Rf_ScalarInteger(x) }
}

/// length of a pairlist or vector.
#[inline(always)]
unsafe fn length(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::NILSXP.0 {
            return 0;
        }
        if t == SEXPTYPE::LISTSXP.0 || t == SEXPTYPE::LANGSXP.0 || t == SEXPTYPE::DOTSXP.0 {
            let mut n: c_int = 0;
            let mut y = x;
            while !y.is_null() {
                n += 1;
                y = CDR(y);
            }
            n
        } else {
            LENGTH(x)
        }
    }
}

/// R_warn_partial_match_args — stub, no-op.
/// In full R, this signals a condition when warnPartialMatchArgs is TRUE.
#[inline(always)]
unsafe fn R_warn_partial_match_dots(_call: SEXP, _btag: SEXP, _ftag: SEXP) {
    // No action needed stub: warning conditions not yet implemented
}

/// Seql — check if two CHARSXP are string-equal.
/// In the full R, this checks encoding-aware equality.
/// For interned strings, pointer equality suffices.
#[inline(always)]
unsafe fn Seql_local(a: SEXP, b: SEXP) -> c_int {
    unsafe {
        if a == b {
            return 1;
        }
        // Fall back to strcmp for non-interned strings
        let ca = CHAR(a);
        let cb = CHAR(b);
        if ca.is_null() || cb.is_null() {
            return 0;
        }
        streql(ca, cb)
    }
}

// ---------------------------------------------------------------------------
// Partial string matching (C string versions)
// ---------------------------------------------------------------------------

/// Partial string match on C strings.
///
/// Matches formal name `f` against tag name `t`.
/// If `exact` is non-zero, requires an exact match.
/// Otherwise, checks if `t` is a prefix of `f`.
///
/// This is used by R for argument matching in function calls.
///
/// # Safety
/// `f` and `t` must be valid null-terminated C strings.
pub unsafe fn psmatch(f: *const c_char, t: *const c_char, exact: c_int) -> c_int {
    unsafe {
        if f.is_null() || t.is_null() {
            return 0;
        }

        if exact != 0 {
            return streql(f, t);
        }

        // Partial match: check if t is a prefix of f
        let mut ff = f;
        let mut tt = t;
        while *tt != 0 {
            if *tt != *ff {
                return 0;
            }
            tt = tt.add(1);
            ff = ff.add(1);
        }
        1
    }
}

/// Case-insensitive partial string match.
///
/// Like `psmatch` but ignores ASCII case differences.
///
/// # Safety
/// `f` and `t` must be valid null-terminated C strings.
pub unsafe fn psmatch_case_insensitive(f: *const c_char, t: *const c_char, exact: c_int) -> c_int {
    unsafe {
        if f.is_null() || t.is_null() {
            return 0;
        }

        let f_bytes = CStr::from_ptr(f).to_bytes();
        let t_bytes = CStr::from_ptr(t).to_bytes();

        if exact != 0 {
            if f_bytes.len() != t_bytes.len() {
                return 0;
            }
            for i in 0..f_bytes.len() {
                if !f_bytes[i].eq_ignore_ascii_case(&t_bytes[i]) {
                    return 0;
                }
            }
            return 1;
        }

        if t_bytes.len() > f_bytes.len() {
            return 0;
        }
        for i in 0..t_bytes.len() {
            if !t_bytes[i].eq_ignore_ascii_case(&f_bytes[i]) {
                return 0;
            }
        }
        1
    }
}

/// R's `pmatch()` algorithm for matching C strings against targets.
///
/// Given a candidate string `x` and a set of target strings `table`,
/// find the best (partial or exact) match.
///
/// Returns the 1-based index of the match, or 0 if no match.
/// If `dup` is non-null, it is set to 1 if there are multiple matches.
///
/// # Safety
/// `x` must be a valid null-terminated C string.
/// `table` must be a valid pointer to an array of `n` valid null-terminated C strings.
pub unsafe fn R_pmatch(
    x: *const c_char,
    table: *const *const c_char,
    n: c_int,
    dup: *mut c_int,
) -> c_int {
    unsafe {
        if x.is_null() || table.is_null() || n <= 0 {
            return 0;
        }

        let x_bytes = CStr::from_ptr(x).to_bytes();
        let x_len = x_bytes.len();

        // Try exact match first
        for i in 0..n as usize {
            let entry = *table.add(i);
            if entry.is_null() {
                continue;
            }
            let entry_bytes = CStr::from_ptr(entry).to_bytes();
            if entry_bytes == x_bytes {
                return (i + 1) as c_int;
            }
        }

        // Try partial match — find unique prefix match
        let mut match_idx: Option<usize> = None;
        for i in 0..n as usize {
            let entry = *table.add(i);
            if entry.is_null() {
                continue;
            }
            let entry_bytes = CStr::from_ptr(entry).to_bytes();
            if entry_bytes.len() >= x_len && &entry_bytes[..x_len] == x_bytes {
                if match_idx.is_some() {
                    // Duplicate partial match
                    if !dup.is_null() {
                        *dup = 1;
                    }
                    return 0;
                }
                match_idx = Some(i);
            }
        }

        match match_idx {
            Some(idx) => {
                if !dup.is_null() {
                    *dup = 0;
                }
                (idx + 1) as c_int
            }
            None => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// SEXP-based matching functions from match.c
// ---------------------------------------------------------------------------

/// NonNullStringMatch — check if two CHARSXP are equal and non-empty, non-NA.
///
/// Returns TRUE (1) if both strings are non-empty, non-NA, and equal.
/// Returns FALSE (0) otherwise.
///
/// Used in subscript.c and subassign.c.
pub unsafe fn NonNullStringMatch(s: SEXP, t: SEXP) -> c_int {
    unsafe {
        // "" or NA string matches nothing
        if isNA_STRING(s) || isNA_STRING(t) {
            return FALSE;
        }
        let cs = CHAR(s);
        let ct = CHAR(t);
        if cs.is_null() || ct.is_null() {
            return FALSE;
        }
        if *cs == 0 || *ct == 0 {
            return FALSE;
        }
        Seql_local(s, t)
    }
}

/// Extract a CHARSXP from various SEXP types.
///
/// - SYMSXP: returns PRINTNAME
/// - CHARSXP: returns itself
/// - STRSXP of length 1: returns STRING_ELT(x, 0)
/// - Otherwise: error
unsafe fn charFromSexp(s: SEXP) -> SEXP {
    unsafe {
        let t = TYPEOF(s);
        if t == SEXPTYPE::SYMSXP.0 {
            PRINTNAME(s)
        } else if t == SEXPTYPE::CHARSXP.0 {
            s
        } else if t == SEXPTYPE::STRSXP.0 {
            if LENGTH(s) == 1 {
                STRING_ELT(s, 0)
            } else {
                std::panic::panic_any(RError {
                    message: "invalid partial string match".to_string(),
                });
            }
        } else {
            std::panic::panic_any(RError {
                message: "invalid partial string match".to_string(),
            });
        }
    }
}

/// pmatch — SEXP-based partial matching for argument matching.
///
/// Matches `formal` against `tag`. Both can be SYMSXP, CHARSXP, or STRSXP.
/// If `exact` is non-zero, requires an exact match.
/// Otherwise, allows partial matching (tag is a prefix of formal).
///
/// # Safety
/// `formal` and `tag` must be valid SEXP pointers.
pub unsafe fn pmatch(formal: SEXP, tag: SEXP, exact: c_int) -> c_int {
    unsafe {
        let f = charFromSexp(formal);
        let t = charFromSexp(tag);
        let fenc = getCharCE(f);
        let tenc = getCharCE(t);

        if fenc == tenc {
            psmatch(CHAR(f), CHAR(t), exact)
        } else {
            // Different encodings — translate to UTF-8 for comparison
            psmatch(translateCharUTF8(f), translateCharUTF8(t), exact)
        }
    }
}

// ---------------------------------------------------------------------------
// Destructive list element extraction
// ---------------------------------------------------------------------------

/// Destructively extract a named list element matching `tag`.
///
/// Searches for the first element whose TAG matches `tag` via psmatch.
/// If found, removes it from the list and returns its CAR.
/// Pattern is a C string.
unsafe fn matchPar_int(tag: *const c_char, list: *mut SEXP, exact: c_int) -> SEXP {
    unsafe {
        if *list == R_NilValue() {
            return R_MissingArg();
        } else if !TAG(*list).is_null()
            && TAG(*list) != R_NilValue()
            && psmatch(tag, CHAR(PRINTNAME(TAG(*list))), exact) != 0
        {
            let s = *list;
            *list = CDR(*list);
            return CAR(s);
        } else {
            let mut last = *list;
            let mut next = CDR(*list);
            while next != R_NilValue() {
                if !TAG(next).is_null()
                    && TAG(next) != R_NilValue()
                    && psmatch(tag, CHAR(PRINTNAME(TAG(next))), exact) != 0
                {
                    SETCDR(last, CDR(next));
                    return CAR(next);
                } else {
                    last = next;
                    next = CDR(next);
                }
            }
            R_MissingArg()
        }
    }
}

/// matchPar — partial matching version of matchPar_int.
pub unsafe fn matchPar(tag: *const c_char, list: *mut SEXP) -> SEXP {
    unsafe { matchPar_int(tag, list, FALSE) }
}

/// matchArg — destructively extract a named list element matching tag (a symbol).
/// Uses partial matching.
pub unsafe fn matchArg(tag: SEXP, list: *mut SEXP) -> SEXP {
    unsafe { matchPar(CHAR(PRINTNAME(tag)), list) }
}

/// matchArgExact — destructively extract a named list element matching tag (a symbol).
/// Uses exact matching.
pub unsafe fn matchArgExact(tag: SEXP, list: *mut SEXP) -> SEXP {
    unsafe { matchPar_int(CHAR(PRINTNAME(tag)), list, TRUE) }
}

// ---------------------------------------------------------------------------
// matchArgs_NR — Match supplied arguments with formals
// ---------------------------------------------------------------------------

/// matchArgs_NR — Match supplied arguments with the formals and return
/// the matched arguments in actuals.
///
/// "NR" means non-reference-tracking — uses CONS_NR.
/// This is the core of R's argument matching for function calls.
/// Note: canonical version lives in sexp/envir.rs; this is a
/// local implementation used internally by match_mod.
pub(crate) unsafe fn matchArgs_NR_local(formals: SEXP, supplied: SEXP, call: SEXP) -> SEXP {
    unsafe {
        let mut seendots: bool;
        let mut i: c_int;
        let mut arg_i: c_int = 0;
        let mut f: SEXP;
        let mut a: SEXP;
        let mut b: SEXP;
        let mut dots: SEXP;
        let mut actuals: SEXP;

        actuals = R_NilValue();
        f = formals;
        while f != R_NilValue() {
            // CONS_NR is used since argument lists created here are only
            // used internally and so should not increment reference counts
            actuals = CONS_NR(R_MissingArg(), actuals);
            SET_MISSING(actuals, 1);
            f = CDR(f);
            arg_i += 1;
        }

        // fargused: track which formals have been used, to avoid modifying
        // the formals SEXPs themselves (which can cause issues with gc/finalizers)
        let nfarg = if arg_i > 0 { arg_i as usize } else { 1 };
        let mut fargused = vec![0i32; nfarg];

        // Reset ARGUSED on all supplied args
        b = supplied;
        while b != R_NilValue() {
            SET_ARGUSED(b, 0);
            b = CDR(b);
        }

        Rf_protect(actuals);

        // ---- First pass: exact matches by tag ----
        f = formals;
        a = actuals;
        arg_i = 0;
        while f != R_NilValue() {
            let ftag = TAG(f);
            let ftag_name = CHAR(PRINTNAME(ftag));
            if ftag != R_DotsSymbol() && ftag != R_NilValue() {
                b = supplied;
                i = 1;
                while b != R_NilValue() {
                    let btag = TAG(b);
                    if btag != R_NilValue() {
                        let btag_name = CHAR(PRINTNAME(btag));
                        if streql(ftag_name, btag_name) != 0 {
                            if fargused[arg_i as usize] == 2 {
                                std::panic::panic_any(RError {
                                    message: format!(
                                        "formal argument \"{}\" matched by multiple actual arguments",
                                        CStr::from_ptr(ftag_name).to_string_lossy()
                                    ),
                                });
                            }
                            if ARGUSED(b) == 2 {
                                std::panic::panic_any(RError {
                                    message: format!(
                                        "argument {} matches multiple formal arguments",
                                        i
                                    ),
                                });
                            }
                            SETCAR(a, CAR(b));
                            if CAR(b) != R_MissingArg() {
                                SET_MISSING(a, 0);
                            }
                            SET_ARGUSED(b, 2);
                            fargused[arg_i as usize] = 2;
                        }
                    }
                    b = CDR(b);
                    i += 1;
                }
            }
            f = CDR(f);
            a = CDR(a);
            arg_i += 1;
        }

        // ---- Second pass: partial matches based on tags ----
        // An exact match is required after first ...
        // The location of the first ... is saved in "dots"
        dots = R_NilValue();
        seendots = false;
        f = formals;
        a = actuals;
        arg_i = 0;
        while f != R_NilValue() {
            if fargused[arg_i as usize] == 0 {
                if TAG(f) == R_DotsSymbol() && !seendots {
                    // Record where ... value goes
                    dots = a;
                    seendots = true;
                } else {
                    b = supplied;
                    i = 1;
                    while b != R_NilValue() {
                        if ARGUSED(b) != 2
                            && TAG(b) != R_NilValue()
                            && pmatch(TAG(f), TAG(b), if seendots { TRUE } else { FALSE }) != 0
                        {
                            if ARGUSED(b) != 0 {
                                std::panic::panic_any(RError {
                                    message: format!(
                                        "argument {} matches multiple formal arguments",
                                        i
                                    ),
                                });
                            }
                            if fargused[arg_i as usize] == 1 {
                                std::panic::panic_any(RError {
                                    message: format!(
                                        "formal argument \"{}\" matched by multiple actual arguments",
                                        CStr::from_ptr(CHAR(PRINTNAME(TAG(f)))).to_string_lossy()
                                    ),
                                });
                            }
                            R_warn_partial_match_dots(call, TAG(b), TAG(f));
                            SETCAR(a, CAR(b));
                            if CAR(b) != R_MissingArg() {
                                SET_MISSING(a, 0);
                            }
                            SET_ARGUSED(b, 1);
                            fargused[arg_i as usize] = 1;
                        }
                        b = CDR(b);
                        i += 1;
                    }
                }
            }
            f = CDR(f);
            a = CDR(a);
            arg_i += 1;
        }

        // ---- Third pass: matches based on order ----
        // All args specified in tag=value form have now been matched.
        // If we find ... we gobble up all the remaining args.
        // Otherwise we bind untagged values in order to any unmatched formals.
        f = formals;
        a = actuals;
        b = supplied;
        seendots = false;

        while f != R_NilValue() && b != R_NilValue() && !seendots {
            if TAG(f) == R_DotsSymbol() {
                // Skip ... matching until all tags done
                seendots = true;
                f = CDR(f);
                a = CDR(a);
            } else if CAR(a) != R_MissingArg() {
                // Already matched by tag — skip to next formal
                f = CDR(f);
                a = CDR(a);
            } else if ARGUSED(b) != 0 || TAG(b) != R_NilValue() {
                // This value used or tagged, skip to next value
                // The second test ensures we don't consider tagged values
                // for positional matches.
                b = CDR(b);
            } else {
                // We have a positional match
                SETCAR(a, CAR(b));
                if CAR(b) != R_MissingArg() {
                    SET_MISSING(a, 0);
                }
                SET_ARGUSED(b, 1);
                b = CDR(b);
                f = CDR(f);
                a = CDR(a);
            }
        }

        if dots != R_NilValue() {
            // Gobble up all unused actuals
            SET_MISSING(dots, 0);
            i = 0;
            a = supplied;
            while a != R_NilValue() {
                if ARGUSED(a) == 0 {
                    i += 1;
                }
                a = CDR(a);
            }

            if i != 0 {
                a = allocList(i);
                SET_TYPEOF(a, SEXPTYPE::DOTSXP);
                f = a;
                b = supplied;
                while b != R_NilValue() {
                    if ARGUSED(b) == 0 {
                        SETCAR(f, CAR(b));
                        SET_TAG(f, TAG(b));
                        f = CDR(f);
                    }
                    b = CDR(b);
                }
                SETCAR(dots, a);
            }
        } else {
            // Check that all arguments are used
            b = supplied;
            while b != R_NilValue() && ARGUSED(b) != 0 {
                b = CDR(b);
            }

            if b != R_NilValue() {
                // Show bad arguments in call without evaluating them
                std::panic::panic_any(RError {
                    message: "unused argument(s)".to_string(),
                });
            }
        }

        Rf_unprotect(1);
        actuals
    }
}

/// matchArgs_RC — wrapper around matchArgs_NR that enables reference counting.
///
/// Use this if the result might escape into R.
pub unsafe fn matchArgs_RC(formals: SEXP, supplied: SEXP, call: SEXP) -> SEXP {
    unsafe {
        let args = matchArgs_NR_local(formals, supplied, call);
        // In full R, this would enable reference counting on the arglist.
        // just return the result as-is.
        args
    }
}

// ---------------------------------------------------------------------------
// patchArgsByActuals — patch promises for NextMethod
// ---------------------------------------------------------------------------

/// Patch states for formals in patchArgsByActuals.
#[repr(i32)]
enum Fstype {
    Unmatched = 0,
    MatchedPresent = 1,
    MatchedMissing = 2,
    MatchedLocal = 3,
}

/// Patch a single argument slot to be a promise.
///
/// If the value is R_MissingArg, look up the name in cloenv.
/// If found, patch to mkPROMISE; otherwise mark as MATCHED_MISSING.
unsafe fn patchArgument(supplied_slot: SEXP, name: SEXP, farg: *mut i32, cloenv: SEXP) {
    unsafe {
        use crate::sexp::envir::R_findVarInFrame;

        let value = CAR(supplied_slot);
        if value == R_MissingArg() {
            let found = R_findVarInFrame(cloenv, name);
            if found == R_MissingArg() {
                if !farg.is_null() {
                    *farg = Fstype::MatchedMissing as i32;
                }
                return;
            }
            if !farg.is_null() {
                *farg = Fstype::MatchedLocal as i32;
            }
        } else if !farg.is_null() {
            *farg = Fstype::MatchedPresent as i32;
        }

        SETCAR(supplied_slot, mkPROMISE(name, cloenv));
    }
}

/// patchArgsByActuals — patch promargs to be promises for the respective actuals
/// in the given environment. Used by NextMethod.
pub unsafe fn patchArgsByActuals(formals: SEXP, supplied: SEXP, cloenv: SEXP) -> SEXP {
    unsafe {
        let mut farg_i: c_int;
        let mut f: SEXP;
        let mut a: SEXP;
        let mut b: SEXP;
        let prsupplied: SEXP;

        let nfarg = length(formals);
        let nfarg_usize = if nfarg > 0 { nfarg as usize } else { 1 };
        let mut farg = vec![Fstype::Unmatched as i32; nfarg_usize];

        // Shallow-duplicate supplied arguments
        prsupplied = Rf_protect(allocList(length(supplied)));
        b = supplied;
        a = prsupplied;
        while b != R_NilValue() {
            SETCAR(a, CAR(b));
            SET_ARGUSED(a, 0);
            SET_TAG(a, TAG(b));
            b = CDR(b);
            a = CDR(a);
        }

        // ---- First pass: exact matches by tag ----
        f = formals;
        farg_i = 0;
        while f != R_NilValue() {
            if TAG(f) != R_DotsSymbol() {
                b = prsupplied;
                while b != R_NilValue() {
                    if TAG(b) != R_NilValue() && pmatch(TAG(f), TAG(b), TRUE) != 0 {
                        patchArgument(b, TAG(f), farg.as_mut_ptr().add(farg_i as usize), cloenv);
                        SET_ARGUSED(b, 2);
                        break; // Previous invocation ensured unique matches
                    }
                    b = CDR(b);
                }
            }
            f = CDR(f);
            farg_i += 1;
        }

        // ---- Second pass: partial matches based on tags ----
        // An exact match is required after first ...
        let mut seendots: bool = false;
        f = formals;
        farg_i = 0;
        while f != R_NilValue() {
            if farg[farg_i as usize] == Fstype::Unmatched as i32 {
                if TAG(f) == R_DotsSymbol() && !seendots {
                    seendots = true;
                } else {
                    b = prsupplied;
                    while b != R_NilValue() {
                        if ARGUSED(b) == 0
                            && TAG(b) != R_NilValue()
                            && pmatch(TAG(f), TAG(b), if seendots { TRUE } else { FALSE }) != 0
                        {
                            patchArgument(
                                b,
                                TAG(f),
                                farg.as_mut_ptr().add(farg_i as usize),
                                cloenv,
                            );
                            SET_ARGUSED(b, 1);
                            break; // Previous invocation ensured unique matches
                        }
                        b = CDR(b);
                    }
                }
            }
            f = CDR(f);
            farg_i += 1;
        }

        // ---- Third pass: matches based on order ----
        f = formals;
        b = prsupplied;
        farg_i = 0;
        while f != R_NilValue() && b != R_NilValue() {
            if TAG(f) == R_DotsSymbol() {
                // Done, ... and following args cannot be patched
                break;
            } else if farg[farg_i as usize] == Fstype::MatchedPresent as i32 {
                // Already matched by tag — skip to next formal
                f = CDR(f);
                farg_i += 1;
            } else if ARGUSED(b) != 0 || TAG(b) != R_NilValue() {
                // This value is used or tagged, skip to next value
                b = CDR(b);
            } else {
                // We have a positional match
                if farg[farg_i as usize] == Fstype::MatchedLocal as i32 {
                    // A missing with a tag has been patched to a promise reading
                    // this formal. Turn this supplied argument into missing to
                    // avoid supplying a value twice.
                    SETCAR(b, R_MissingArg());
                } else {
                    patchArgument(b, TAG(f), ptr::null_mut(), cloenv);
                }
                SET_ARGUSED(b, 1);
                b = CDR(b);
                f = CDR(f);
                farg_i += 1;
            }
        }

        // Previous invocation of matchArgs_NR ensured all args are used
        Rf_unprotect(1);
        prsupplied
    }
}

// ---------------------------------------------------------------------------
// do_match — R's match() function
// ---------------------------------------------------------------------------
//
// This is a simplified port of R's do_match from unique.c.
// The full implementation uses hashing (HashData, HashTableSetup, DoHashing,
// HashLookup) which are not yet available. We implement a linear scan version
// that handles STRSXP, INTSXP, LGLSXP, REALSXP types.
//
// .Internal(match(x, table, nomatch, incomparables))

pub unsafe fn do_match(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let x = CAR(args);
        let table = CADR(args);
        let nomatch_val = asInteger(CADDR(args));
        // CADDDR(args) = incomparables — not supported in stub

        let n = XLENGTH(x);
        if n == 0 {
            return Rf_allocVector(SEXPTYPE::INTSXP.0, 0);
        }

        let ntable = LENGTH(table);
        if ntable == 0 {
            let ans = Rf_allocVector(SEXPTYPE::INTSXP.0, n as c_int);
            let pa = INTEGER(ans);
            for i in 0..n as i64 {
                *pa.add(i as usize) = nomatch_val;
            }
            return ans;
        }

        let xtype = TYPEOF(x);
        let ttype = TYPEOF(table);

        // Determine common type
        let common_type = if xtype >= SEXPTYPE::STRSXP.0 || ttype >= SEXPTYPE::STRSXP.0 {
            SEXPTYPE::STRSXP
        } else if xtype < ttype {
            SEXPTYPE(ttype)
        } else {
            SEXPTYPE(xtype)
        };

        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, n as c_int));
        let ians = INTEGER(ans);

        // Initialize all to nomatch
        for i in 0..n as i64 {
            *ians.add(i as usize) = nomatch_val;
        }

        match common_type {
            SEXPTYPE::STRSXP => {
                for i in 0..n as i64 {
                    let x_elt = STRING_ELT(x, i);
                    for j in 0..ntable as i64 {
                        if Seql_local(STRING_ELT(table, j), x_elt) != 0 {
                            *ians.add(i as usize) = (j + 1) as c_int;
                            break;
                        }
                    }
                }
            }
            SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => {
                let x_ints = INTEGER(x);
                let t_ints = INTEGER(table);
                for i in 0..n as i64 {
                    let xval = *x_ints.add(i as usize);
                    for j in 0..ntable as i64 {
                        if *t_ints.add(j as usize) == xval {
                            *ians.add(i as usize) = (j + 1) as c_int;
                            break;
                        }
                    }
                }
            }
            SEXPTYPE::REALSXP => {
                let x_reals = REAL(x);
                let t_reals = REAL(table);
                for i in 0..n as i64 {
                    let xval = *x_reals.add(i as usize);
                    for j in 0..ntable as i64 {
                        let tval = *t_reals.add(j as usize);
                        // NaN and NA matching
                        if xval.is_nan() && tval.is_nan() {
                            *ians.add(i as usize) = (j + 1) as c_int;
                            break;
                        }
                        if xval == tval {
                            *ians.add(i as usize) = (j + 1) as c_int;
                            break;
                        }
                    }
                }
            }
            _ => {
                // Unsupported type — return nomatch for all
            }
        }

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_pmatch — R's pmatch() function
// ---------------------------------------------------------------------------
//
// .Internal(pmatch(x, table, nomatch, duplicates.ok))

pub unsafe fn do_pmatch(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let input = CAR(args);
        let target = CADR(args);
        let n_input = XLENGTH(input);
        let n_target = LENGTH(target);
        let no_match = asInteger(CADDR(args));
        let dups_ok = asLogical(CADDDR(args));

        if dups_ok == NA_INTEGER {
            std::panic::panic_any(RError {
                message: "invalid 'duplicates.ok' argument".to_string(),
            });
        }
        let no_dups = dups_ok == 0;

        if !isString(input) || !isString(target) {
            std::panic::panic_any(RError {
                message: "argument is not of mode character".to_string(),
            });
        }

        // Track which target entries have been used (for no_dups mode)
        let mut used: Vec<c_int> = if no_dups {
            vec![0; n_target as usize]
        } else {
            Vec::new()
        };

        // Determine encoding mode
        let mut use_bytes = false;
        let mut use_utf8 = false;

        for i in 0..n_input {
            if IS_BYTES(STRING_ELT(input, i)) != 0 {
                use_bytes = true;
                use_utf8 = false;
                break;
            } else if ENC_KNOWN(STRING_ELT(input, i)) != 0 {
                use_utf8 = true;
            }
        }
        if !use_bytes {
            for i in 0..n_target {
                if IS_BYTES(STRING_ELT(target, i as R_xlen_t)) != 0 {
                    use_bytes = true;
                    use_utf8 = false;
                    break;
                } else if ENC_KNOWN(STRING_ELT(target, i as R_xlen_t)) != 0 {
                    use_utf8 = true;
                }
            }
        }

        // Build string arrays
        let mut in_strs: Vec<*const c_char> = Vec::with_capacity(n_input as usize);
        let mut tar_strs: Vec<*const c_char> = Vec::with_capacity(n_target as usize);

        for i in 0..n_input {
            let s = STRING_ELT(input, i);
            let cs = if use_bytes {
                CHAR(s)
            } else if use_utf8 {
                translateCharUTF8(s)
            } else {
                translateChar(s)
            };
            in_strs.push(cs);
        }
        for j in 0..n_target {
            let s = STRING_ELT(target, j as R_xlen_t);
            let cs = if use_bytes {
                CHAR(s)
            } else if use_utf8 {
                translateCharUTF8(s)
            } else {
                translateChar(s)
            };
            tar_strs.push(cs);
        }

        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, n_input as c_int));
        let ians = INTEGER(ans);

        // Initialize
        for i in 0..n_input as usize {
            *ians.add(i) = 0;
        }

        // ---- First pass: exact matching ----
        let mut nexact: R_xlen_t = 0;
        for i in 0..n_input as usize {
            let ss = in_strs[i];
            let slen = libc::strlen(ss);
            if slen == 0 {
                continue;
            }
            for j in 0..n_target as usize {
                if no_dups && used[j] != 0 {
                    continue;
                }
                if libc::strcmp(ss, tar_strs[j]) == 0 {
                    *ians.add(i) = (j + 1) as c_int;
                    if no_dups {
                        used[j] = 1;
                    }
                    nexact += 1;
                    break;
                }
            }
        }

        if (nexact as R_xlen_t) < n_input {
            // ---- Second pass: partial matching ----
            for i in 0..n_input as usize {
                if *ians.add(i) != 0 {
                    continue;
                }
                let ss = in_strs[i];
                let temp = libc::strlen(ss);
                if temp == 0 {
                    continue;
                }
                let mut mtch: c_int = 0;
                let mut mtch_count: c_int = 0;
                for j in 0..n_target as usize {
                    if no_dups && used[j] != 0 {
                        continue;
                    }
                    if libc::strncmp(ss, tar_strs[j], temp) == 0 {
                        mtch = (j + 1) as c_int;
                        mtch_count += 1;
                    }
                }
                if mtch > 0 && mtch_count == 1 {
                    if no_dups {
                        used[(mtch - 1) as usize] = 1;
                    }
                    *ians.add(i) = mtch;
                }
            }
            // ---- Third pass: set no matches ----
            for i in 0..n_input as usize {
                if *ians.add(i) == 0 {
                    *ians.add(i) = no_match;
                }
            }
        }

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_charmatch — R's charmatch() function
// ---------------------------------------------------------------------------
//
// .Internal(charmatch(x, table, nomatch))
//
// Based on Therneau's charmatch.
// Returns integer vector:
//   0 = no match or multiple matches
//   j = unique exact match at position j
//   j = unique partial match at position j (if no exact match)

pub unsafe fn do_charmatch(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let input = CAR(args);
        let target = CADR(args);
        let n_input = LENGTH(input);
        let n_target = LENGTH(target);
        let no_match = asInteger(CADDR(args));

        if !isString(input) || !isString(target) {
            std::panic::panic_any(RError {
                message: "argument is not of mode character".to_string(),
            });
        }

        // Determine encoding mode
        let mut use_bytes = false;
        let mut use_utf8 = false;

        for i in 0..n_input {
            if IS_BYTES(STRING_ELT(input, i as R_xlen_t)) != 0 {
                use_bytes = true;
                use_utf8 = false;
                break;
            } else if ENC_KNOWN(STRING_ELT(input, i as R_xlen_t)) != 0 {
                use_utf8 = true;
            }
        }
        if !use_bytes {
            for i in 0..n_target {
                if IS_BYTES(STRING_ELT(target, i as R_xlen_t)) != 0 {
                    use_bytes = true;
                    use_utf8 = false;
                    break;
                } else if ENC_KNOWN(STRING_ELT(target, i as R_xlen_t)) != 0 {
                    use_utf8 = true;
                }
            }
        }

        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, n_input));
        let ians = INTEGER(ans);

        for i in 0..n_input as usize {
            let s_elt = STRING_ELT(input, i as R_xlen_t);
            let ss = if use_bytes {
                CHAR(s_elt)
            } else if use_utf8 {
                translateCharUTF8(s_elt)
            } else {
                translateChar(s_elt)
            };
            let temp = libc::strlen(ss);
            let mut imatch: c_int = NA_INTEGER;
            let mut perfect = false;

            for j in 0..n_target as usize {
                let t_elt = STRING_ELT(target, j as R_xlen_t);
                let st = if use_bytes {
                    CHAR(t_elt)
                } else if use_utf8 {
                    translateCharUTF8(t_elt)
                } else {
                    translateChar(t_elt)
                };

                let k = libc::strncmp(ss, st, temp);
                if k == 0 {
                    if libc::strlen(st) == temp {
                        // Exact match
                        if perfect {
                            imatch = 0;
                        } else {
                            perfect = true;
                            imatch = (j + 1) as c_int;
                        }
                    } else if !perfect {
                        // Partial match
                        if imatch == NA_INTEGER {
                            imatch = (j + 1) as c_int;
                        } else {
                            imatch = 0;
                        }
                    }
                }
            }

            *ians.add(i) = if imatch == NA_INTEGER {
                no_match
            } else {
                imatch
            };
        }

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn test_ok<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
        match result {
            Ok(value) => value,
            Err(err) => panic!("test setup failed: {err}"),
        }
    }

    #[test]
    fn test_psmatch_exact() {
        let f = test_ok(CString::new("abc"));
        let t = test_ok(CString::new("abc"));
        assert_eq!(unsafe { psmatch(f.as_ptr(), t.as_ptr(), 1) }, 1);

        let t2 = test_ok(CString::new("ab"));
        assert_eq!(unsafe { psmatch(f.as_ptr(), t2.as_ptr(), 1) }, 0);
    }

    #[test]
    fn test_psmatch_partial() {
        let f = test_ok(CString::new("abc"));
        let t = test_ok(CString::new("ab"));
        assert_eq!(unsafe { psmatch(f.as_ptr(), t.as_ptr(), 0) }, 1);

        let t2 = test_ok(CString::new("bc"));
        assert_eq!(unsafe { psmatch(f.as_ptr(), t2.as_ptr(), 0) }, 0);

        let t3 = test_ok(CString::new("abcd"));
        assert_eq!(unsafe { psmatch(f.as_ptr(), t3.as_ptr(), 0) }, 0);
    }

    #[test]
    fn test_psmatch_null() {
        assert_eq!(unsafe { psmatch(std::ptr::null(), std::ptr::null(), 0) }, 0);
    }

    #[test]
    fn test_psmatch_case_insensitive() {
        let f = test_ok(CString::new("ABC"));
        let t = test_ok(CString::new("ab"));
        assert_eq!(
            unsafe { psmatch_case_insensitive(f.as_ptr(), t.as_ptr(), 0) },
            1
        );

        let f2 = test_ok(CString::new("abc"));
        let t2 = test_ok(CString::new("ABC"));
        assert_eq!(
            unsafe { psmatch_case_insensitive(f2.as_ptr(), t2.as_ptr(), 1) },
            1
        );
    }

    #[test]
    fn test_R_pmatch_exact() {
        let x = test_ok(CString::new("foo"));
        let t1 = test_ok(CString::new("bar"));
        let t2 = test_ok(CString::new("foo"));
        let t3 = test_ok(CString::new("baz"));
        let table = [t1.as_ptr(), t2.as_ptr(), t3.as_ptr()];
        let mut dup: c_int = 0;
        let result = unsafe { R_pmatch(x.as_ptr(), table.as_ptr(), 3, &mut dup) };
        assert_eq!(result, 2); // 1-based index
    }

    #[test]
    fn test_R_pmatch_partial_unique() {
        let x = test_ok(CString::new("fo"));
        let t1 = test_ok(CString::new("bar"));
        let t2 = test_ok(CString::new("foo"));
        let t3 = test_ok(CString::new("baz"));
        let table = [t1.as_ptr(), t2.as_ptr(), t3.as_ptr()];
        let mut dup: c_int = 0;
        let result = unsafe { R_pmatch(x.as_ptr(), table.as_ptr(), 3, &mut dup) };
        assert_eq!(result, 2);
        assert_eq!(dup, 0);
    }

    #[test]
    fn test_R_pmatch_partial_duplicate() {
        let x = test_ok(CString::new("ba"));
        let t1 = test_ok(CString::new("bar"));
        let t2 = test_ok(CString::new("baz"));
        let table = [t1.as_ptr(), t2.as_ptr()];
        let mut dup: c_int = 0;
        let result = unsafe { R_pmatch(x.as_ptr(), table.as_ptr(), 2, &mut dup) };
        assert_eq!(result, 0);
        assert_eq!(dup, 1); // duplicate match
    }

    #[test]
    fn test_R_pmatch_no_match() {
        let x = test_ok(CString::new("xyz"));
        let t1 = test_ok(CString::new("bar"));
        let t2 = test_ok(CString::new("foo"));
        let table = [t1.as_ptr(), t2.as_ptr()];
        let result = unsafe { R_pmatch(x.as_ptr(), table.as_ptr(), 2, std::ptr::null_mut()) };
        assert_eq!(result, 0);
    }

    #[test]
    fn test_streql() {
        let a = test_ok(CString::new("hello"));
        let b = test_ok(CString::new("hello"));
        let c = test_ok(CString::new("world"));
        assert_eq!(unsafe { streql(a.as_ptr(), b.as_ptr()) }, 1);
        assert_eq!(unsafe { streql(a.as_ptr(), c.as_ptr()) }, 0);
        assert_eq!(unsafe { streql(std::ptr::null(), std::ptr::null()) }, 1);
    }

    #[test]
    fn test_NonNullStringMatch_null() {
        // With null NA_STRING, both should be null = NA
        assert_eq!(
            unsafe { NonNullStringMatch(ptr::null_mut(), ptr::null_mut()) },
            FALSE
        );
    }
}
