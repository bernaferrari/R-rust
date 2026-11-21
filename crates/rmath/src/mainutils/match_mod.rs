#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/match.c — string matching utilities.
//!
//! This module ports the partial and exact string matching functions
//! used by R for argument matching, `pmatch()`, `match()`, `charmatch()`, etc.
//!
//! Ported functions from match.c:
//!   psmatch, NonNullStringMatch, pmatch (SEXP version), charFromSexp,
//!   matchArgs_NR, matchArgs_RC

use std::ffi::CStr;
use std::os::raw::c_int;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::{FALSE, NA_INTEGER, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::*;
use crate::sexp::memory_ext::{CONS_NR, allocList};
use crate::sexp::protect::protect;
use crate::sexp::symbol::R_DotsSymbol;

// ---------------------------------------------------------------------------
// Local helper macros and functions
// ---------------------------------------------------------------------------

/// ARGUSED(x) == LEVELS(x) — uses gp[0..1] bits.
#[inline(always)]
fn ARGUSED(x: SEXP) -> c_int {
    LEVELS(x)
}

/// SET_ARGUSED(x, v) == SETLEVELS(x, v).
#[inline(always)]
fn SET_ARGUSED(x: SEXP, v: c_int) {
    SETLEVELS(x, v);
}

/// SET_TAG equivalent (R macro). Uses existing SETTAG.
#[inline(always)]
fn SET_TAG(x: SEXP, y: SEXP) {
    SETTAG(x, y);
}

/// SET_TYPEOF equivalent (R macro).
#[inline(always)]
fn SET_TYPEOF(x: SEXP, v: SEXPTYPE) {
    crate::sexp::accessors::SET_TYPEOF(x, v.as_c_int());
}

/// SET_MISSING equivalent (R macro). Sets gp[2] bit.
/// In R, MISSING(x) is gp[2], and SET_MISSING(x,v) sets it.
/// gp bits 0-1 = LEVELS/ARGUSED, bit 2 = MISSING.
#[inline(always)]
fn SET_MISSING(x: SEXP, v: c_int) {
    if x.is_null() {
        return;
    }
    crate::sexp::accessors::SET_MISSING(x, v);
}

/// String equality check (streql); two absent strings compare equal.
#[inline(always)]
fn streql(s1: Option<&CStr>, s2: Option<&CStr>) -> c_int {
    match (s1, s2) {
        (Some(a), Some(b)) => (a == b) as c_int,
        (None, None) => 1,
        _ => 0,
    }
}

#[inline(always)]
fn NA_STRING() -> SEXP {
    crate::mainutils::relop::NA_STRING()
}

#[inline(always)]
fn isNA_STRING(s: SEXP) -> bool {
    if s.is_null() {
        return true;
    }
    LEVELS(s) & 1 != 0
}

/// Raw CHAR bytes of a CHARSXP (GNU `CHAR(x)`, up to its terminator).
#[inline(always)]
fn char_bytes(s: SEXP) -> Vec<u8> {
    crate::mainutils::printutils::charsxp_bytes(s)
}

/// Partial (or exact) match of tag bytes `t` against formal bytes `f`.
fn psmatch_bytes(f: &[u8], t: &[u8], exact: c_int) -> c_int {
    if exact != 0 {
        (f == t) as c_int
    } else {
        f.starts_with(t) as c_int
    }
}

/// isString check — STRSXP type.
#[inline(always)]
fn isString(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::STRSXP
}

/// isVector check.
#[inline(always)]
fn isVector(x: SEXP) -> bool {
    crate::sexp::object::raw_is_vector(x)
}

/// isNull check.
#[inline(always)]
fn isNull(x: SEXP) -> bool {
    x.is_null() || TYPEOF(x) == SEXPTYPE::NILSXP
}

/// IS_BYTES check — delegates to sexp::accessors.
#[inline(always)]
fn IS_BYTES(s: SEXP) -> c_int {
    crate::sexp::accessors::IS_BYTES(s)
}

/// ENC_KNOWN check — delegates to sexp::accessors.
#[inline(always)]
fn ENC_KNOWN(s: SEXP) -> c_int {
    crate::sexp::accessors::ENC_KNOWN(s)
}

/// IS_CACHED check — stub, always returns true.
#[inline(always)]
fn IS_CACHED(_s: SEXP) -> c_int {
    1
}

#[inline(always)]
fn getCharCE(s: SEXP) -> c_int {
    crate::sexp::accessors::getCharCE(s)
}

/// checkArity — shared primitive arity validation.
#[inline(always)]
fn checkArity(op: SEXP, args: SEXP) {
    crate::mainutils::relop::checkArity(op, args)
}

/// asInteger — extract integer value from scalar.
#[inline(always)]
fn asInteger(x: SEXP) -> c_int {
    if x.is_null() || XLENGTH(x) == 0 {
        return NA_INTEGER;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::INTSXP {
        INTEGER_ELT(x, 0)
    } else if t == SEXPTYPE::LGLSXP {
        LOGICAL_ELT(x, 0)
    } else if t == SEXPTYPE::REALSXP {
        let v = REAL_ELT(x, 0);
        if v.is_nan() {
            return NA_INTEGER;
        }
        v as c_int
    } else {
        NA_INTEGER
    }
}

/// asLogical — extract logical value from scalar.
#[inline(always)]
fn asLogical(x: SEXP) -> c_int {
    if x.is_null() || XLENGTH(x) == 0 {
        return NA_INTEGER;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::LGLSXP {
        LOGICAL_ELT(x, 0)
    } else if t == SEXPTYPE::INTSXP {
        INTEGER_ELT(x, 0)
    } else {
        NA_INTEGER
    }
}

/// isObject check.
#[inline(always)]
fn isObject(x: SEXP) -> bool {
    OBJECT(x) != 0
}

/// ScalarInteger — allocate a scalar integer.
#[inline(always)]
fn ScalarInteger(x: c_int) -> SEXP {
    Rf_ScalarInteger(x)
}

/// length of a pairlist or vector.
#[inline(always)]
fn length(x: SEXP) -> c_int {
    {
        if x.is_null() {
            return 0;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::NILSXP {
            return 0;
        }
        if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP || t == SEXPTYPE::DOTSXP {
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

/// R_warn_partial_match_args — signals a partial-argument-match warning
/// condition when `options(warnPartialMatchArgs = TRUE)`.
#[inline(always)]
pub(crate) fn R_warn_partial_match_args(call: SEXP, btag: SEXP, ftag: SEXP) {
    if !crate::mainutils::options::logical_option_enabled(c"warnPartialMatchArgs") {
        return;
    }
    let cond =
        crate::mainutils::errors::R_makePartialArgumentMatchWarningCondition(call, btag, ftag);
    let _cond_guard = protect(cond);
    crate::mainutils::errors::R_signalWarningCondition(cond);
}

/// Seql — check if two CHARSXP are string-equal.
/// In the full R, this checks encoding-aware equality.
/// For interned strings, pointer equality suffices.
#[inline(always)]
fn Seql_local(a: SEXP, b: SEXP) -> c_int {
    if a == b {
        return 1;
    }
    // Fall back to strcmp for non-interned strings
    if a.is_null() || b.is_null() {
        return 0;
    }
    (char_bytes(a) == char_bytes(b)) as c_int
}

// ---------------------------------------------------------------------------
// Partial string matching (C string versions)
// ---------------------------------------------------------------------------

/// Partial string match on C strings.
///
/// Matches formal name `f` against tag name `t`.
/// If `exact` is non-zero, requires an exact match.
/// Otherwise, checks if `t` is a prefix of `f`. Absent strings never match.
///
/// This is used by R for argument matching in function calls.
pub fn psmatch(f: Option<&CStr>, t: Option<&CStr>, exact: c_int) -> c_int {
    match (f, t) {
        (Some(f), Some(t)) => psmatch_bytes(f.to_bytes(), t.to_bytes(), exact),
        _ => 0,
    }
}

/// Case-insensitive partial string match.
///
/// Like `psmatch` but ignores ASCII case differences.
pub fn psmatch_case_insensitive(f: Option<&CStr>, t: Option<&CStr>, exact: c_int) -> c_int {
    let (Some(f), Some(t)) = (f, t) else {
        return 0;
    };
    let f_bytes = f.to_bytes();
    let t_bytes = t.to_bytes();

    if exact != 0 {
        return f_bytes.eq_ignore_ascii_case(t_bytes) as c_int;
    }

    if t_bytes.len() > f_bytes.len() {
        return 0;
    }
    f_bytes[..t_bytes.len()].eq_ignore_ascii_case(t_bytes) as c_int
}

/// R's `pmatch()` algorithm for matching C strings against targets.
///
/// Given a candidate string `x` and a set of target strings `table`,
/// find the best (partial or exact) match. Absent table entries are skipped.
///
/// Returns the 1-based index of the match, or 0 if no match.
/// If `dup` is supplied, it is set to 1 if there are multiple matches.
pub fn R_pmatch(x: Option<&CStr>, table: &[Option<&CStr>], dup: Option<&mut c_int>) -> c_int {
    let Some(x) = x else {
        return 0;
    };
    if table.is_empty() {
        return 0;
    }
    let x_bytes = x.to_bytes();

    // Try exact match first
    if let Some(i) = table
        .iter()
        .position(|entry| entry.is_some_and(|entry| entry.to_bytes() == x_bytes))
    {
        return (i + 1) as c_int;
    }

    // Try partial match — find unique prefix match
    let mut match_idx: Option<usize> = None;
    for (i, entry) in table.iter().enumerate() {
        let Some(entry) = entry else {
            continue;
        };
        if entry.to_bytes().starts_with(x_bytes) {
            if match_idx.is_some() {
                // Duplicate partial match
                if let Some(dup) = dup {
                    *dup = 1;
                }
                return 0;
            }
            match_idx = Some(i);
        }
    }

    match match_idx {
        Some(idx) => {
            if let Some(dup) = dup {
                *dup = 0;
            }
            (idx + 1) as c_int
        }
        None => 0,
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
pub fn NonNullStringMatch(s: SEXP, t: SEXP) -> c_int {
    // "" or NA string matches nothing
    if isNA_STRING(s) || isNA_STRING(t) {
        return FALSE;
    }
    if char_bytes(s).is_empty() || char_bytes(t).is_empty() {
        return FALSE;
    }
    Seql_local(s, t)
}

/// Extract a CHARSXP from various SEXP types.
///
/// - SYMSXP: returns PRINTNAME
/// - CHARSXP: returns itself
/// - STRSXP of length 1: returns STRING_ELT(x, 0)
/// - Otherwise: error
fn charFromSexp(s: SEXP) -> SEXP {
    let t = TYPEOF(s);
    if t == SEXPTYPE::SYMSXP {
        PRINTNAME(s)
    } else if t == SEXPTYPE::CHARSXP {
        s
    } else if t == SEXPTYPE::STRSXP {
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

/// pmatch — SEXP-based partial matching for argument matching.
///
/// Matches `formal` against `tag`. Both can be SYMSXP, CHARSXP, or STRSXP.
/// If `exact` is non-zero, requires an exact match.
/// Otherwise, allows partial matching (tag is a prefix of formal).
///
/// # Safety
/// `formal` and `tag` must be valid SEXP pointers.
pub fn pmatch(formal: SEXP, tag: SEXP, exact: c_int) -> c_int {
    let f = charFromSexp(formal);
    let t = charFromSexp(tag);
    // Mixed encodings would translate to UTF-8 here; this runtime's
    // `translateCharUTF8` exposes the stored CHAR bytes, so both cases compare
    // the CHAR payloads.
    psmatch_bytes(&char_bytes(f), &char_bytes(t), exact)
}

// ---------------------------------------------------------------------------
// matchArgs_NR — Match supplied arguments with formals
// ---------------------------------------------------------------------------

/// matchArgs_NR — Match supplied arguments with the formals and return
/// the matched arguments in actuals.
///
/// "NR" means non-reference-tracking — uses CONS_NR.
/// This is the production three-pass matcher (exact, partial, positional).
/// Closure evaluation uses `eval/closure.rs::match_closure_args`, a second
/// port of the same GNU `match.c` algorithm.
pub(crate) fn matchArgs_NR_local(formals: SEXP, supplied: SEXP, call: SEXP) -> SEXP {
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

    let _actuals_guard = protect(actuals);

    // ---- First pass: exact matches by tag ----
    f = formals;
    a = actuals;
    arg_i = 0;
    while f != R_NilValue() {
        let ftag = TAG(f);
        if ftag != R_DotsSymbol() && ftag != R_NilValue() {
            let ftag_name = char_bytes(PRINTNAME(ftag));
            b = supplied;
            i = 1;
            while b != R_NilValue() {
                let btag = TAG(b);
                if !btag.is_null() && btag != R_NilValue() {
                    let btag_name = char_bytes(PRINTNAME(btag));
                    if ftag_name == btag_name {
                        if fargused[arg_i as usize] == 2 {
                            std::panic::panic_any(RError {
                                message: format!(
                                    "formal argument \"{}\" matched by multiple actual arguments",
                                    String::from_utf8_lossy(&ftag_name)
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
                        && !TAG(b).is_null()
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
                                    String::from_utf8_lossy(&char_bytes(PRINTNAME(TAG(f))))
                                ),
                            });
                        }
                        R_warn_partial_match_args(call, TAG(b), TAG(f));
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
        } else if ARGUSED(b) != 0 || (!TAG(b).is_null() && TAG(b) != R_NilValue()) {
            // Used or genuinely tagged (GNU: TAG != R_NilValue).
            // Lazy-load pairlists store untagged args as a null TAG,
            // which must still take a positional slot.
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
            // Show bad arguments in call without evaluating them.
            // Stock match.c wording: singular/plural with every unused
            // argument listed, e.g. "unused arguments (c = 3, d = 4)".
            let mut items: Vec<String> = Vec::new();
            let mut u = b;
            while u != R_NilValue() {
                if ARGUSED(u) == 0 {
                    items.push(unused_arg_item(CAR(u), TAG(u)));
                }
                u = CDR(u);
            }
            let message = if items.len() == 1 {
                format!("unused argument ({})", items[0])
            } else {
                format!("unused arguments ({})", items.join(", "))
            };
            std::panic::panic_any(RError { message });
        }
    }

    actuals
}

/// One `tag = value` (or bare `value`) item for the stock "unused
/// argument(s)" error, deparsed without evaluating promises.
fn unused_arg_item(car: SEXP, tag: SEXP) -> String {
    let mut car_b = car;
    if TYPEOF(car_b) == SEXPTYPE::PROMSXP {
        car_b = PRCODE(car_b);
    }
    let text = crate::mainutils::deparse::deparse1line(car_b, false);
    let deparsed = if text.is_null() || text == R_NilValue() || XLENGTH(text) == 0 {
        String::new()
    } else {
        String::from_utf8_lossy(&char_bytes(STRING_ELT(text, 0))).into_owned()
    };
    if tag.is_null() || tag == R_NilValue() {
        return deparsed;
    }
    let pname = PRINTNAME(tag);
    if pname.is_null() || pname == R_NilValue() {
        return deparsed;
    }
    format!(
        "{} = {}",
        String::from_utf8_lossy(&char_bytes(pname)),
        deparsed
    )
}

/// matchArgs_RC — wrapper around matchArgs_NR that enables reference counting.
///
/// Use this if the result might escape into R.
/// # Safety
/// No obligations beyond live SEXP arguments; the signature stays `unsafe`
/// for existing library callers.
pub unsafe fn matchArgs_RC(formals: SEXP, supplied: SEXP, call: SEXP) -> SEXP {
    let args = matchArgs_NR_local(formals, supplied, call);
    // In full R, this would enable reference counting on the arglist.
    // just return the result as-is.
    args
}

/// Bind evaluated `args` to `names` via GNU `matchArgs` (exact, partial, positional).
/// # Safety
/// No obligations beyond live SEXP arguments; the signature stays `unsafe`
/// for existing library callers.
pub unsafe fn match_formal_slots(call: SEXP, args: SEXP, names: &[&str]) -> Vec<SEXP> {
    let mut formals = R_NilValue();
    for name in names.iter().rev() {
        let sym = crate::sexp::symbol::install_str(name);
        let cell = Rf_cons(R_MissingArg(), formals);
        SETTAG(cell, sym);
        formals = cell;
    }
    let _formals = protect(formals);
    let matched = matchArgs_NR_local(formals, args, call);
    let _matched = protect(matched);
    let mut out = Vec::with_capacity(names.len());
    let mut cell = matched;
    while !cell.is_null() && cell != R_NilValue() {
        out.push(CAR(cell));
        cell = CDR(cell);
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::ptr;

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
        assert_eq!(psmatch(Some(&f), Some(&t), 1), 1);

        let t2 = test_ok(CString::new("ab"));
        assert_eq!(psmatch(Some(&f), Some(&t2), 1), 0);
    }

    #[test]
    fn test_psmatch_partial() {
        let f = test_ok(CString::new("abc"));
        let t = test_ok(CString::new("ab"));
        assert_eq!(psmatch(Some(&f), Some(&t), 0), 1);

        let t2 = test_ok(CString::new("bc"));
        assert_eq!(psmatch(Some(&f), Some(&t2), 0), 0);

        let t3 = test_ok(CString::new("abcd"));
        assert_eq!(psmatch(Some(&f), Some(&t3), 0), 0);
    }

    #[test]
    fn test_psmatch_null() {
        assert_eq!(psmatch(None, None, 0), 0);
    }

    #[test]
    fn test_psmatch_case_insensitive() {
        let f = test_ok(CString::new("ABC"));
        let t = test_ok(CString::new("ab"));
        assert_eq!(psmatch_case_insensitive(Some(&f), Some(&t), 0), 1);

        let f2 = test_ok(CString::new("abc"));
        let t2 = test_ok(CString::new("ABC"));
        assert_eq!(psmatch_case_insensitive(Some(&f2), Some(&t2), 1), 1);
    }

    #[test]
    fn test_R_pmatch_exact() {
        let x = test_ok(CString::new("foo"));
        let t1 = test_ok(CString::new("bar"));
        let t2 = test_ok(CString::new("foo"));
        let t3 = test_ok(CString::new("baz"));
        let table = [
            Some(t1.as_c_str()),
            Some(t2.as_c_str()),
            Some(t3.as_c_str()),
        ];
        let mut dup: c_int = 0;
        let result = R_pmatch(Some(&x), &table, Some(&mut dup));
        assert_eq!(result, 2); // 1-based index
    }

    #[test]
    fn test_R_pmatch_partial_unique() {
        let x = test_ok(CString::new("fo"));
        let t1 = test_ok(CString::new("bar"));
        let t2 = test_ok(CString::new("foo"));
        let t3 = test_ok(CString::new("baz"));
        let table = [
            Some(t1.as_c_str()),
            Some(t2.as_c_str()),
            Some(t3.as_c_str()),
        ];
        let mut dup: c_int = 0;
        let result = R_pmatch(Some(&x), &table, Some(&mut dup));
        assert_eq!(result, 2);
        assert_eq!(dup, 0);
    }

    #[test]
    fn test_R_pmatch_partial_duplicate() {
        let x = test_ok(CString::new("ba"));
        let t1 = test_ok(CString::new("bar"));
        let t2 = test_ok(CString::new("baz"));
        let table = [Some(t1.as_c_str()), Some(t2.as_c_str())];
        let mut dup: c_int = 0;
        let result = R_pmatch(Some(&x), &table, Some(&mut dup));
        assert_eq!(result, 0);
        assert_eq!(dup, 1); // duplicate match
    }

    #[test]
    fn test_R_pmatch_no_match() {
        let x = test_ok(CString::new("xyz"));
        let t1 = test_ok(CString::new("bar"));
        let t2 = test_ok(CString::new("foo"));
        let table = [Some(t1.as_c_str()), Some(t2.as_c_str())];
        let result = R_pmatch(Some(&x), &table, None);
        assert_eq!(result, 0);
    }

    #[test]
    fn test_streql() {
        let a = test_ok(CString::new("hello"));
        let b = test_ok(CString::new("hello"));
        let c = test_ok(CString::new("world"));
        assert_eq!(streql(Some(&a), Some(&b)), 1);
        assert_eq!(streql(Some(&a), Some(&c)), 0);
        assert_eq!(streql(None, None), 1);
    }

    #[test]
    fn test_NonNullStringMatch_null() {
        // With null NA_STRING, both should be null = NA
        assert_eq!(NonNullStringMatch(ptr::null_mut(), ptr::null_mut()), FALSE);
    }
}
