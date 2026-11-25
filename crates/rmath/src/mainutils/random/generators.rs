#![allow(unreachable_code)]
#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]
use super::*;
use crate::mainutils::sort::revsort_with_perm;
use crate::sexp::object::{SessionNodeFactory, Sexp, SexpMut, SexpResult};

fn checked<T>(result: SexpResult<T>) -> T {
    result.unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Checked handle lease for this translated boundary: the current runtime is
/// active inside every do_* entry point below.
fn sexp_factory() -> SessionNodeFactory<'static> {
    SessionNodeFactory::new(
        crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
    )
}

fn wrap_arg(x: SEXP) -> Sexp<'static> {
    checked(sexp_factory().wrap(x))
}






















pub fn R_unif_index(dn: c_double) -> c_double {
    r_R_unif_index(dn)
}


pub fn R_binom_kind() -> c_int {
    r_binom_kind() as c_int
}

// ---------------------------------------------------------------------------
// Internal random generation helpers (used by do_random1/2/3)
// ---------------------------------------------------------------------------

/// One-parameter random generation using a function pointer wrapper.
fn random1_call(f: fn(f64) -> f64, a: &Sexp, na: R_xlen_t, x: &mut SexpMut, n: R_xlen_t) -> bool {
    let mut naflag = false;
    let mut ia: R_xlen_t = 0;
    for i in 0..n {
        let ai = checked(a.try_real_elt(ia));
        let val = f(ai);
        checked(x.try_set_real_elt(i, val));
        if val.is_nan() {
            naflag = true;
        }
        ia += 1;
        if ia >= na {
            ia = 0;
        }
    }
    naflag
}

/// Two-parameter random generation using a function pointer wrapper.
fn random2_call(
    f: fn(f64, f64) -> f64,
    a: &Sexp,
    na: R_xlen_t,
    b: &Sexp,
    nb: R_xlen_t,
    x: &mut SexpMut,
    n: R_xlen_t,
) -> bool {
    let mut naflag = false;
    let mut ia: R_xlen_t = 0;
    let mut ib: R_xlen_t = 0;
    for i in 0..n {
        let ai = checked(a.try_real_elt(ia));
        let bi = checked(b.try_real_elt(ib));
        let val = f(ai, bi);
        checked(x.try_set_real_elt(i, val));
        if val.is_nan() {
            naflag = true;
        }
        ia += 1;
        if ia >= na {
            ia = 0;
        }
        ib += 1;
        if ib >= nb {
            ib = 0;
        }
    }
    naflag
}

/// Three-parameter random generation using a function pointer wrapper.
fn random3_call(
    f: fn(f64, f64, f64) -> f64,
    a: &Sexp,
    na: R_xlen_t,
    b: &Sexp,
    nb: R_xlen_t,
    c: &Sexp,
    nc: R_xlen_t,
    x: &mut SexpMut,
    n: R_xlen_t,
) -> bool {
    let mut naflag = false;
    let mut ia: R_xlen_t = 0;
    let mut ib: R_xlen_t = 0;
    let mut ic: R_xlen_t = 0;
    for i in 0..n {
        let ai = checked(a.try_real_elt(ia));
        let bi = checked(b.try_real_elt(ib));
        let ci = checked(c.try_real_elt(ic));
        let val = f(ai, bi, ci);
        checked(x.try_set_real_elt(i, val));
        if val.is_nan() {
            naflag = true;
        }
        ia += 1;
        if ia >= na {
            ia = 0;
        }
        ib += 1;
        if ib >= nb {
            ib = 0;
        }
        ic += 1;
        if ic >= nc {
            ic = 0;
        }
    }
    naflag
}

// ---------------------------------------------------------------------------
// Helper functions for SEXP argument handling
// ---------------------------------------------------------------------------

fn isVector(x: SEXP) -> bool {
    {
        let t = TYPEOF(x);
        t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::LGLSXP
            || t == SEXPTYPE::CPLXSXP
            || t == SEXPTYPE::STRSXP
            || t == SEXPTYPE::VECSXP
            || t == SEXPTYPE::RAWSXP
    }
}

fn isNumeric(x: SEXP) -> bool {
    {
        let t = TYPEOF(x);
        t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP
    }
}

fn asInteger_local(x: SEXP) -> c_int {
    if x.is_null() {
        return NA_INTEGER;
    }
    let t = TYPEOF(x);
    let Ok(x) = sexp_factory().wrap(x) else {
        return NA_INTEGER;
    };
    if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
        x.try_integer_elt(0).unwrap_or(NA_INTEGER)
    } else if t == SEXPTYPE::REALSXP {
        let v = x.try_real_elt(0).unwrap_or(f64::NAN);
        if !v.is_finite() || v > i32::MAX as f64 || v < i32::MIN as f64 {
            NA_INTEGER
        } else {
            v as c_int
        }
    } else {
        NA_INTEGER
    }
}

fn asReal_local(x: SEXP) -> c_double {
    if x.is_null() {
        return f64::NAN;
    }
    let t = TYPEOF(x);
    let Ok(x) = sexp_factory().wrap(x) else {
        return f64::NAN;
    };
    if t == SEXPTYPE::REALSXP {
        x.try_real_elt(0).unwrap_or(f64::NAN)
    } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
        let v = x.try_integer_elt(0).unwrap_or(NA_INTEGER);
        if v == NA_INTEGER { f64::NAN } else { v as f64 }
    } else {
        f64::NAN
    }
}

#[allow(clippy::if_same_then_else)]
fn asLogical_local(x: SEXP) -> c_int {
    if x.is_null() {
        return NA_INTEGER;
    }
    let t = TYPEOF(x);
    let Ok(x) = sexp_factory().wrap(x) else {
        return NA_INTEGER;
    };
    if t == SEXPTYPE::LGLSXP {
        x.try_logical_elt(0).unwrap_or(NA_INTEGER)
    } else if t == SEXPTYPE::INTSXP {
        x.try_integer_elt(0).unwrap_or(NA_INTEGER)
    } else {
        NA_INTEGER
    }
}

fn isNull(x: SEXP) -> bool {
    x.is_null() || x == R_NilValue()
}

fn checkArity(op: SEXP, args: SEXP) {
    // stub
}

fn PRIMVAL_local(op: SEXP) -> c_int {
    crate::mainutils::relop::PRIMVAL(op)
}

// ---------------------------------------------------------------------------
// do_random1 -- random sampling from 1 parameter families
// ---------------------------------------------------------------------------

/// Allocate an owned REALSXP handle at this audited boundary. The length is
/// truncated through `c_int`, matching the original `Rf_allocVector` call.
fn alloc_real_vector(n: R_xlen_t) -> Sexp<'static> {
    let len = R_xlen_t::from(n as c_int);
    checked(sexp_factory().allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::REALSXP, len))))
}

/// Allocate an owned INTSXP handle at this audited boundary. The length is
/// truncated through `c_int`, matching the original `Rf_allocVector` call.
fn alloc_int_vector(n: R_xlen_t) -> Sexp<'static> {
    let len = R_xlen_t::from(n as c_int);
    checked(sexp_factory().allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, len))))
}

/// Coerce to a checked REALSXP handle at this audited boundary.
fn coerce_real(x: SEXP) -> Sexp<'static> {
    checked(sexp_factory().wrap(coerceVector(x, SEXPTYPE::REALSXP.as_c_int())))
}

/// R's .Internal interface for 1-parameter distributions:
/// rchisq, rexp, rgeom, rpois, rt, rsignrank
pub fn do_random1(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    checkArity(op, args);

    if !isVector(CAR(args)) || !isNumeric(CADR(args)) {
        error("invalid arguments");
    }

    let n: R_xlen_t;
    if XLENGTH(CAR(args)) == 1 {
        let dn = asReal_local(CAR(args));
        if dn.is_nan() || dn < 0.0 {
            error("invalid arguments");
        }
        n = dn as R_xlen_t;
    } else {
        n = XLENGTH(CAR(args));
    }

    let x = alloc_real_vector(n);
    if n == 0 {
        return x.as_raw();
    }
    let mut x_mut = checked(SexpMut::try_from_checked(x.clone()));

    let na = XLENGTH(CADR(args));
    if na < 1 {
        for i in 0..n {
            checked(x_mut.try_set_real_elt(i, f64::NAN));
        }
        return x.as_raw();
    }

    let a = coerce_real(CADR(args));
    GetRNGstate();

    let primval = PRIMVAL_local(op);
    let _naflag: bool;

    match primval {
        0 => {
            let _ = random1_call(crate::nmath::dist::chisq::rchisq, &a, na, &mut x_mut, n);
        }
        1 => {
            let _ = random1_call(crate::nmath::dist::exponential::rexp, &a, na, &mut x_mut, n);
        }
        2 => {
            let _ = random1_call(crate::nmath::dist::geometric::rgeom, &a, na, &mut x_mut, n);
        }
        3 => {
            let _ = random1_call(crate::nmath::dist::poisson::rpois, &a, na, &mut x_mut, n);
        }
        4 => {
            let _ = random1_call(crate::nmath::dist::t_dist::rt, &a, na, &mut x_mut, n);
        }
        5 => {
            let _ = random1_call(crate::nmath::dist::signrank::rsignrank, &a, na, &mut x_mut, n);
        }
        _ => {} // intentionally unhandled: unknown distribution type
    }

    PutRNGstate();
    x.as_raw()
}

// ---------------------------------------------------------------------------
// do_random2 -- random sampling from 2 parameter families
// ---------------------------------------------------------------------------

/// R's .Internal interface for 2-parameter distributions:
/// rbeta, rbinom, rcauchy, rf, rgamma, rlnorm, rlogis, rnbinom, rnorm, runif,
/// rweibull, rwilcox, rnchisq, rnbinom_mu
pub fn do_random2(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    checkArity(op, args);

    if !isVector(CAR(args)) || !isNumeric(CADR(args)) || !isNumeric(CADDR(args)) {
        error("invalid arguments");
    }

    let n: R_xlen_t;
    if XLENGTH(CAR(args)) == 1 {
        let dn = asReal_local(CAR(args));
        if dn.is_nan() || dn < 0.0 {
            error("invalid arguments");
        }
        n = dn as R_xlen_t;
    } else {
        n = XLENGTH(CAR(args));
    }

    let x = alloc_real_vector(n);
    if n == 0 {
        return x.as_raw();
    }
    let mut x_mut = checked(SexpMut::try_from_checked(x.clone()));

    let na = XLENGTH(CADR(args));
    let nb = XLENGTH(CADDR(args));
    if na < 1 || nb < 1 {
        for i in 0..n {
            checked(x_mut.try_set_real_elt(i, f64::NAN));
        }
        return x.as_raw();
    }

    let a = coerce_real(CADR(args));
    let b = coerce_real(CADDR(args));
    GetRNGstate();

    let primval = PRIMVAL_local(op);
    let dist: fn(f64, f64) -> f64 = match primval {
        0 => crate::nmath::dist::beta::rbeta,
        1 => crate::nmath::dist::binomial::rbinom,
        2 => crate::nmath::dist::cauchy::rcauchy,
        3 => crate::nmath::dist::f_dist::rf,
        4 => crate::nmath::dist::gamma::rgamma,
        5 => crate::nmath::dist::lnorm::rlnorm,
        6 => crate::nmath::dist::logistic::rlogis,
        7 => crate::nmath::dist::nbinom::rnbinom,
        8 => crate::nmath::dist::normal::rnorm,
        9 => crate::nmath::dist::uniform::runif,
        10 => crate::nmath::dist::weibull::rweibull,
        11 => crate::nmath::dist::wilcox::rwilcox,
        12 => crate::nmath::dist::nchisq::rnchisq,
        13 => crate::nmath::dist::nbinom::rnbinom_mu,
        _ => crate::nmath::dist::beta::rbeta, // intentionally unhandled: unknown distribution type
    };
    if (0..=13).contains(&primval) {
        let _ = random2_call(dist, &a, na, &b, nb, &mut x_mut, n);
    }

    PutRNGstate();
    x.as_raw()
}

// ---------------------------------------------------------------------------
// do_random3 -- random sampling from 3 parameter families
// ---------------------------------------------------------------------------

/// R's .Internal interface for 3-parameter distributions:
/// rhyper
pub fn do_random3(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    checkArity(op, args);

    if !isVector(CAR(args)) {
        error("invalid arguments");
    }

    let n: R_xlen_t;
    if XLENGTH(CAR(args)) == 1 {
        let dn = asReal_local(CAR(args));
        if dn.is_nan() || dn < 0.0 {
            error("invalid arguments");
        }
        n = dn as R_xlen_t;
    } else {
        n = XLENGTH(CAR(args));
    }

    let x = alloc_real_vector(n);
    if n == 0 {
        return x.as_raw();
    }
    let mut x_mut = checked(SexpMut::try_from_checked(x.clone()));

    let mut args_rest = CDR(args);
    let a = CAR(args_rest);
    args_rest = CDR(args_rest);
    let b = CAR(args_rest);
    args_rest = CDR(args_rest);
    let c = CAR(args_rest);
    if !isNumeric(a) || !isNumeric(b) || !isNumeric(c) {
        return x.as_raw();
    }

    let na = XLENGTH(a);
    let nb = XLENGTH(b);
    let nc = XLENGTH(c);
    if na < 1 || nb < 1 || nc < 1 {
        for i in 0..n {
            checked(x_mut.try_set_real_elt(i, f64::NAN));
        }
        return x.as_raw();
    }

    let a = coerce_real(a);
    let b = coerce_real(b);
    let c = coerce_real(c);
    GetRNGstate();

    let primval = PRIMVAL_local(op);

    if primval == 0 {
        let _ = random3_call(
            crate::nmath::dist::hypergeometric::rhyper,
            &a,
            na,
            &b,
            nb,
            &c,
            nc,
            &mut x_mut,
            n,
        );
    }

    PutRNGstate();
    x.as_raw()
}

// ---------------------------------------------------------------------------
// do_RNGkind / do_setseed -- R's RNGkind() and set.seed()
//
// Stock R wraps the .Internal layer in base-package closures that validate
// string arguments (pmatch against the kind tables) and convert the integer
// result back to names. This port has no R-level closures, so the same
// validation and name conversion lives here.
// ---------------------------------------------------------------------------

const RNG_KIND_NAMES: [&str; 9] = [
    "Wichmann-Hill",
    "Marsaglia-Multicarry",
    "Super-Duper",
    "Mersenne-Twister",
    "Knuth-TAOCP",
    "user-supplied",
    "Knuth-TAOCP-2002",
    "L'Ecuyer-CMRG",
    "default",
];

const N01_KIND_NAMES: [&str; 7] = [
    "Buggy Kinderman-Ramage",
    "Ahrens-Dieter",
    "Box-Muller",
    "user-supplied",
    "Inversion",
    "Kinderman-Ramage",
    "default",
];

const SAMPLE_KIND_NAMES: [&str; 3] = ["Rounding", "Rejection", "default"];

const BINOM_KIND_NAMES: [&str; 3] = ["Buggy BTPE", "BTPE", "default"];

/// pmatch(x, table) for one string: exact match wins, then a unique prefix
/// match; ambiguity or no match is None (stock pmatch).
fn pmatch_one(x: &str, table: &[&str]) -> Option<usize> {
    if let Some(i) = table.iter().position(|t| *t == x) {
        return Some(i);
    }
    let mut matches = table.iter().enumerate().filter(|(_, t)| t.starts_with(x));
    match (matches.next(), matches.next()) {
        (Some((i, _)), None) => Some(i),
        _ => None,
    }
}

fn arg_is_absent(x: SEXP) -> bool {
    x.is_null() || { x == R_NilValue() || x == R_MissingArg() }
}

/// True when the slot carries the given parameter tag. The evaluator keeps
/// call order, so `RNGkind(normal.kind = "Inversion")` delivers the value in
/// the first slot; tag inspection restores the stock parameter mapping.
fn kind_tag_is(x: SEXP, name: &str) -> bool {
    if x.is_null() {
        return false;
    }
    let tag = TAG(x);
    if tag.is_null() || tag == R_NilValue() {
        return false;
    }
    let pname = PRINTNAME(tag);
    if pname.is_null() {
        return false;
    }
    sexp_factory()
        .wrap(pname)
        .ok()
        .and_then(|pname| pname.try_char_eq(name.as_bytes()).ok())
        .unwrap_or(false)
}

/// Map positional slots to kind parameters by tag when any tag is present.
///
/// `cells` are the pairlist cells holding the kind/normal.kind/sample.kind/
/// binom.kind arguments in call order; TAG is read from the cell, not the value.
fn resolve_kind_args(cells: [SEXP; 4]) -> (SEXP, SEXP, SEXP, SEXP) {
    let nil = R_NilValue();
    let mut kind = nil;
    let mut norm = nil;
    let mut sample = nil;
    let mut binom = nil;
    for cell in cells {
        if cell.is_null() || cell == nil {
            continue;
        }
        let x = CAR(cell);
        if arg_is_absent(x) {
            continue;
        }
        if kind_tag_is(cell, "normal.kind") {
            norm = x;
        } else if kind_tag_is(cell, "sample.kind") {
            sample = x;
        } else if kind_tag_is(cell, "binom.kind") {
            binom = x;
        } else {
            // Untagged slots fill kind, then normal.kind, then
            // sample.kind, then binom.kind (stock positional order).
            if kind == nil {
                kind = x;
            } else if norm == nil {
                norm = x;
            } else if sample == nil {
                sample = x;
            } else {
                binom = x;
            }
        }
    }
    (kind, norm, sample, binom)
}

/// First string element as lossy UTF-8; `NA_character_` reads as "NA", like
/// `CHAR(STRING_ELT(arg, 0))` in the original translation.
fn kind_string(arg: SEXP) -> String {
    sexp_factory()
        .wrap(arg)
        .ok()
        .and_then(|arg| arg.try_string_value_elt(0).ok().flatten())
        .unwrap_or_else(|| "NA".to_string())
}

fn kind_error(msg: &str) -> ! {
    let call = crate::mainutils::errors::R_getCurrentCall();
    let nil = R_NilValue();
    if !call.is_null() && call != nil {
        crate::mainutils::errors::errorcall_str(call, msg);
    }
    crate::mainutils::errors::errorcall_str(nil, msg);
}

/// `kind = NULL` absent | code 0..=7 | -1 for "default".
fn parse_rng_kind_arg(arg: SEXP) -> Option<i32> {
    if arg_is_absent(arg) || (TYPEOF(arg) == SEXPTYPE::STRSXP && XLENGTH(arg) == 0) {
        return None;
    }
    if TYPEOF(arg) != SEXPTYPE::STRSXP || XLENGTH(arg) > 1 {
        kind_error("'kind' must be a character string (RNG to be used).");
    }
    let s = kind_string(arg);
    match pmatch_one(&s, &RNG_KIND_NAMES) {
        Some(i) if i == RNG_KIND_NAMES.len() - 1 => Some(-1),
        Some(i) => Some(i as i32),
        None => kind_error(&format!("'{s}' is not a valid abbreviation of an RNG")),
    }
}

/// `normal.kind = NULL` absent | code 0..=5 | -1 for "default".
fn parse_normal_kind_arg(arg: SEXP, from_setseed: bool) -> Option<i32> {
    if arg_is_absent(arg) {
        return None;
    }
    if TYPEOF(arg) != SEXPTYPE::STRSXP || XLENGTH(arg) != 1 {
        kind_error("'normal.kind' must be a character string");
    }
    let s = kind_string(arg);
    match pmatch_one(&s, &N01_KIND_NAMES) {
        None => kind_error(&format!("'{s}' is not valid for 'normal.kind'")),
        Some(0) => {
            // Buggy Kinderman-Ramage
            if from_setseed {
                kind_error("buggy version of Kinderman-Ramage generator is not allowed");
            }
            warn("buggy version of Kinderman-Ramage generator used");
            Some(0)
        }
        Some(i) if i == N01_KIND_NAMES.len() - 1 => Some(-1),
        Some(i) => Some(i as i32),
    }
}

/// `sample.kind = NULL` absent | code 0..=1 | -1 for "default".
fn parse_sample_kind_arg(arg: SEXP) -> Option<i32> {
    if arg_is_absent(arg) {
        return None;
    }
    if TYPEOF(arg) != SEXPTYPE::STRSXP || XLENGTH(arg) != 1 {
        kind_error("'sample.kind' must be a character string");
    }
    let s = kind_string(arg);
    match pmatch_one(&s, &SAMPLE_KIND_NAMES) {
        None => kind_error(&format!("'{s}' is not valid for 'sample.kind'")),
        Some(0) => {
            warn("non-uniform 'Rounding' sampler used");
            Some(0)
        }
        Some(i) if i == SAMPLE_KIND_NAMES.len() - 1 => Some(-1),
        Some(i) => Some(i as i32),
    }
}

/// `binom.kind = NULL` absent | code 0..=1 | -1 for "default".
fn parse_binom_kind_arg(arg: SEXP) -> Option<i32> {
    if arg_is_absent(arg) {
        return None;
    }
    if TYPEOF(arg) != SEXPTYPE::STRSXP || XLENGTH(arg) != 1 {
        kind_error("'binom.kind' must be a character string");
    }
    let s = kind_string(arg);
    match pmatch_one(&s, &BINOM_KIND_NAMES) {
        None => kind_error(&format!("'{s}' is not valid for 'binom.kind'")),
        Some(0) => {
            warn("Buggy BTPE algorithm used for rbinom()");
            Some(0)
        }
        Some(i) if i == BINOM_KIND_NAMES.len() - 1 => Some(-1),
        Some(i) => Some(i as i32),
    }
}

fn rng_kind_from_code(code: i32) -> RNGtype {
    match code {
        0 => RNGtype::WICHMANN_HILL,
        1 => RNGtype::MARSAGLIA_MULTICARRY,
        2 => RNGtype::SUPER_DUPER,
        3 => RNGtype::MERSENNE_TWISTER,
        4 => RNGtype::KNUTH_TAOCP,
        5 => RNGtype::USER_UNIF,
        6 => RNGtype::KNUTH_TAOCP2,
        7 => RNGtype::LECUYER_CMRG,
        -1 => RNG_DEFAULT,
        _ => RNG_DEFAULT,
    }
}

fn n01_kind_from_code(code: i32) -> N01type {
    match code {
        0 => N01type::BUGGY_KINDERMAN_RAMAGE,
        1 => N01type::AHRENS_DIETER,
        2 => N01type::BOX_MULLER,
        3 => N01type::USER_NORM,
        4 => N01type::INVERSION,
        5 => N01type::KINDERMAN_RAMAGE,
        -1 => N01_DEFAULT,
        _ => N01_DEFAULT,
    }
}

fn sample_kind_from_code(code: i32) -> Sampletype {
    match code {
        0 => Sampletype::ROUNDING,
        -1 => Sample_DEFAULT,
        _ => Sampletype::REJECTION,
    }
}

fn binom_kind_from_code(code: i32) -> Binomtype {
    match code {
        0 => Binomtype::BUGGY_BTPE,
        -1 => Binom_DEFAULT,
        _ => Binomtype::BTPE,
    }
}

/// R's `RNGkind(kind = NULL, normal.kind = NULL, sample.kind = NULL,
/// binom.kind = NULL)`.
///
/// Returns the *old* kind names as a character vector; the value is visible
/// when nothing was set and invisible otherwise (stock closure behavior).
pub fn do_RNGkind(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    checkArity(op, args);
    GetRNGstate(); /* might not be initialized */

    let (rng_code, norm_code, sample_code, binom_code) =
        resolve_kind_args([args, CDR(args), CDR(CDR(args)), CDR(CDR(CDR(args)))]);
    let rng_parsed = parse_rng_kind_arg(rng_code);
    let norm_parsed = parse_normal_kind_arg(norm_code, false);
    let sample_parsed = parse_sample_kind_arg(sample_code);
    let binom_parsed = parse_binom_kind_arg(binom_code);

    // Snapshot the old kinds before applying anything.
    let (old_rng, old_n01, old_sample, old_binom) = with_rng_state(|rng| {
        (
            rng.rng_kind.get() as c_int,
            rng.n01_kind.get() as c_int,
            rng.sample_kind.get() as c_int,
            rng.binom_kind.get() as c_int,
        )
    });

    // Stock pulls the kind codes from .Random.seed (if present) before
    // switching; reloading the full state from the same source matches.
    GetRNGstate();

    if let Some(code) = rng_parsed {
        r_RNGkind(rng_kind_from_code(code));
    }
    if let Some(code) = norm_parsed {
        r_Norm_kind(n01_kind_from_code(code));
    }
    if let Some(code) = sample_parsed {
        r_Samp_kind(sample_kind_from_code(code));
    }
    if let Some(code) = binom_parsed {
        r_Bin_kind(binom_kind_from_code(code));
    }

    let ans = checked(sexp_factory().strings(&[
        RNG_KIND_NAMES[old_rng as usize],
        N01_KIND_NAMES[old_n01 as usize],
        SAMPLE_KIND_NAMES[old_sample as usize],
        BINOM_KIND_NAMES[old_binom as usize],
    ]));

    crate::sexp::globals::set_R_Visible(i32::from(
        rng_parsed.is_none()
            && norm_parsed.is_none()
            && sample_parsed.is_none()
            && binom_parsed.is_none(),
    ));
    ans.as_raw()
}

/// GNU `RNGversion(vstr)` — select RNGkind for a historical R version.
pub fn do_RNGversion(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let v = CAR(args);
    if v.is_null() || v == R_NilValue() || TYPEOF(v) != SEXPTYPE::STRSXP || XLENGTH(v) < 1 {
        error("malformed version string");
    }
    let raw = kind_string(v);
    let parts: Vec<i32> = raw
        .split('.')
        .filter_map(|p| p.parse::<i32>().ok())
        .collect();
    if parts.len() < 2 {
        error("malformed version string");
    }
    let major = parts[0];
    let minor = parts[1];
    let (rng, norm, sample, binom) = if major == 0 && minor < 99 {
        (
            RNGtype::WICHMANN_HILL,
            N01type::BUGGY_KINDERMAN_RAMAGE,
            Sampletype::ROUNDING,
            Binomtype::BUGGY_BTPE,
        )
    } else if major == 0 || (major == 1 && minor <= 6) {
        (
            RNGtype::MARSAGLIA_MULTICARRY,
            N01type::BUGGY_KINDERMAN_RAMAGE,
            Sampletype::ROUNDING,
            Binomtype::BUGGY_BTPE,
        )
    } else if major <= 2 || (major == 3 && minor <= 5) {
        (
            RNGtype::MERSENNE_TWISTER,
            N01type::INVERSION,
            Sampletype::ROUNDING,
            Binomtype::BUGGY_BTPE,
        )
    } else if major <= 3 || (major == 4 && minor <= 6) {
        (
            RNGtype::MERSENNE_TWISTER,
            N01type::INVERSION,
            Sampletype::REJECTION,
            Binomtype::BUGGY_BTPE,
        )
    } else {
        (
            RNGtype::MERSENNE_TWISTER,
            N01type::INVERSION,
            Sampletype::REJECTION,
            Binomtype::BTPE,
        )
    };
    r_RNGkind(rng);
    r_Norm_kind(norm);
    r_Samp_kind(sample);
    r_Bin_kind(binom);
    if matches!(binom, Binomtype::BUGGY_BTPE) {
        // RNGkind() warns when "Buggy BTPE" is (re)selected; under
        // options(warn = 2) this becomes the error RNGversion() raises.
        warn("Buggy BTPE algorithm used for rbinom()");
    }
    crate::sexp::globals::set_R_Visible(0);
    R_NilValue()
}

/// R's `set.seed(seed, kind = NULL, normal.kind = NULL, sample.kind = NULL,
/// binom.kind = NULL)`.
///
/// Returns invisible NULL.
pub fn do_setseed(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    checkArity(op, args);

    // The stock closure validates kind arguments before any state change.
    let tail = CDR(args);
    let (skind, nkind, sampkind, binomkind) =
        resolve_kind_args([tail, CDR(tail), CDR(CDR(tail)), CDR(CDR(CDR(tail)))]);
    let kind_parsed = parse_rng_kind_arg(skind);
    let norm_parsed = parse_normal_kind_arg(nkind, true);
    let sample_parsed = parse_sample_kind_arg(sampkind);
    let binom_parsed = parse_binom_kind_arg(binomkind);

    let seed: i64;
    if !isNull(CAR(args)) && CAR(args) != R_MissingArg() {
        let arg = CAR(args);
        if TYPEOF(arg) == SEXPTYPE::STRSXP {
            // asInteger() coerces strings via NAs
            warn("NAs introduced by coercion");
            kind_error("supplied seed is not a valid integer");
        }
        let v = asInteger_local(arg);
        if v == NA_INTEGER {
            kind_error("supplied seed is not a valid integer");
        }
        seed = v as i64;
    } else {
        seed = TimeToSeed() as i64;
    }

    // Pull RNG_kind/N01_kind from .Random.seed if present (stock).
    GetRNGstate();

    if let Some(code) = kind_parsed {
        r_RNGkind(rng_kind_from_code(code));
    }
    if let Some(code) = norm_parsed {
        r_Norm_kind(n01_kind_from_code(code));
    }
    if let Some(code) = sample_parsed {
        r_Samp_kind(sample_kind_from_code(code));
    }
    if let Some(code) = binom_parsed {
        r_Bin_kind(binom_kind_from_code(code));
    }

    // Initialize the RNG with the seed (zaps Box-Muller history)
    with_rng_state(|rng| {
        let kind = rng.rng_kind.get();
        RNG_Init(rng, kind, seed);
    });
    PutRNGstate();

    crate::sexp::globals::set_R_Visible(0);
    R_NilValue()
}

/// Seed the session's R-level RNG from a 64-bit host seed and write
/// `.Random.seed` (host-embedding analogue of `set.seed`).
pub fn set_session_seed64(seed: i64) {
    with_rng_state(|rng| {
        let kind = rng.rng_kind.get();
        RNG_Init(rng, kind, seed);
    });
    PutRNGstate();
}

/// Hook installed into the nmath crate so its samplers draw from the
/// session's R-level RNG dispatch (all RNG kinds, `.Random.seed` state)
/// instead of the standalone MultiCarry stream.
pub fn nmath_unif_hook() -> f64 {
    if crate::sexp::instance::has_current_instance() {
        // Stock R randomizes lazily when the RNG is first used without a
        // .Random.seed; a virgin per-instance state gets the same treatment
        // instead of drawing from an all-zero Mersenne Twister state.
        let virgin = with_rng_state(|rng| !rng.initialized.get());
        if virgin {
            with_rng_state(|rng| {
                let kind = rng.rng_kind.get();
                RNG_Init(rng, kind, crate::mainutils::times::TimeToSeed() as i64);
            });
        }
        r_unif_rand()
    } else {
        crate::nmath::rng::multicarry_unif_rand()
    }
}

/// Hook installed into the nmath crate so `rbinom()`'s BTPE algorithm
/// selection follows the session's `binom.kind` — the port-side equivalent
/// of stock RNG.c's `Bin_kind` writing nmath's shared `Binom_kind` global.
pub fn nmath_binom_kind_hook() -> crate::nmath::rng::Binomtype {
    if crate::sexp::instance::has_current_instance() {
        match r_binom_kind() {
            Binomtype::BUGGY_BTPE => crate::nmath::rng::Binomtype::BUGGY_BTPE,
            Binomtype::BTPE => crate::nmath::rng::Binomtype::BTPE,
        }
    } else {
        // No active session: the standalone default (ML_Binom_kind role).
        crate::nmath::rng::Binomtype::BTPE
    }
}

// ---------------------------------------------------------------------------
// do_sample -- R's sample() function
// ---------------------------------------------------------------------------

pub fn do_sample(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    checkArity(op, args);

    let sn = CAR(args);
    let args2 = CDR(args);
    let sk = CAR(args2);
    let args3 = CDR(args2);
    let sreplace = CAR(args3);
    let args4 = CDR(args3);
    let prob = CAR(args4);

    if LENGTH(sk) != 1 {
        error("invalid 'size' argument");
    }
    if LENGTH(sreplace) != 1 {
        error("invalid 'replace' argument");
    }
    let replace = asLogical_local(sreplace);
    if replace == NA_INTEGER {
        error("invalid 'replace' argument");
    }

    GetRNGstate();

    if !isNull(prob) {
        let n = asInteger_local(sn);
        let k = asInteger_local(sk);
        if n == NA_INTEGER || n < 0 || (k > 0 && n == 0) {
            PutRNGstate();
            error("invalid first argument");
        }
        if k == NA_INTEGER || k < 0 {
            PutRNGstate();
            error("invalid 'size' argument");
        }
        if replace == 0 && k > n {
            PutRNGstate();
            error("cannot take a sample larger than the population when 'replace = FALSE'");
        }

        let y = alloc_int_vector(R_xlen_t::from(k));
        let mut y_mut = checked(SexpMut::try_from_checked(y.clone()));
        for i in 0..k {
            checked(y_mut.try_set_integer_elt(R_xlen_t::from(i), 0));
        }
        PutRNGstate();
        return y.as_raw();
    }

    // Uniform sampling
    let dn = asReal_local(sn);
    let k = if asReal_local(sk).is_nan() {
        0
    } else {
        asReal_local(sk) as R_xlen_t
    };
    if !dn.is_finite() || dn < 0.0 || dn > 4.5e15 {
        PutRNGstate();
        error("invalid first argument");
    }
    if k < 0 {
        PutRNGstate();
        error("invalid 'size' argument");
    }
    if replace == 0 && k > dn as R_xlen_t {
        PutRNGstate();
        error("cannot take a sample larger than the population when 'replace = FALSE'");
    }

    if dn > i32::MAX as f64 || k > i32::MAX as R_xlen_t {
        // Long vector support
        let y = alloc_real_vector(k);
        let mut y_mut = checked(SexpMut::try_from_checked(y.clone()));
        if replace != 0 {
            for i in 0..k {
                checked(y_mut.try_set_real_elt(i, r_R_unif_index(dn) + 1.0));
            }
        }
        PutRNGstate();
        return y.as_raw();
    }

    let n = dn as i32;
    let kk = k as i32;
    let y = alloc_int_vector(R_xlen_t::from(kk));
    let mut y_mut = checked(SexpMut::try_from_checked(y.clone()));

    if replace != 0 || kk < 2 {
        for i in 0..kk {
            checked(
                y_mut.try_set_integer_elt(R_xlen_t::from(i), r_R_unif_index(n as f64) as i32 + 1),
            );
        }
    } else {
        // Without replacement: Fisher-Yates shuffle
        let mut x: Vec<i32> = (0..n).collect();
        for i in 0..kk as usize {
            let remaining = (n - i as i32) as usize;
            let j = r_R_unif_index(remaining as f64) as usize;
            checked(y_mut.try_set_integer_elt(i as R_xlen_t, x[j] + 1));
            x[j] = x[remaining - 1];
        }
    }

    PutRNGstate();
    y.as_raw()
}

// ---------------------------------------------------------------------------
// Probability vector normalization (pure Rust, no SEXP)
// ---------------------------------------------------------------------------

/// Normalize a probability vector and validate it.
///
/// Ensures all probabilities are non-negative, finite, and sum to 1.
///
/// # Errors
/// Returns an error string if validation fails, or `None` on success.
pub fn FixupProb(p: &mut [f64], require_k: usize, replace: bool) -> Option<&'static str> {
    let n = p.len();
    let mut sum = 0.0;
    let mut npos = 0usize;

    for i in 0..n {
        if !p[i].is_finite() {
            return Some("NA in probability vector");
        }
        if p[i] < 0.0 {
            return Some("negative probability");
        }
        if p[i] > 0.0 {
            npos += 1;
            sum += p[i];
        }
    }

    if npos == 0 || (!replace && require_k > npos) {
        return Some("too few positive probabilities");
    }

    for i in 0..n {
        p[i] /= sum;
    }

    None
}

// ---------------------------------------------------------------------------
// Sampling with replacement (pure Rust)
// ---------------------------------------------------------------------------

/// Unequal probability sampling with replacement using the R RNG.
pub fn ProbSampleReplace_r(n: usize, p: &mut [f64], nans: usize) -> Vec<c_int> {
    let mut perm: Vec<c_int> = (1..=n as c_int).collect();
    revsort_with_perm(p, &mut perm, n);
    for i in 1..n {
        p[i] += p[i - 1];
    }
    let mut ans = vec![0 as c_int; nans];
    let nm1 = n - 1;
    for i in 0..nans {
        let r_u = r_unif_rand();
        let mut j = nm1;
        for jj in 0..nm1 {
            if r_u <= p[jj] {
                j = jj;
                break;
            }
        }
        ans[i] = perm[j];
    }
    ans
}

/// A version using Walker's alias method, based on Alg 3.13B in
/// Ripley (1987). Port of walker_ProbSampleReplace
/// (r-source/src/main/random.c), which stock `do_sample` picks over
/// ProbSampleReplace when more than 200 categories have `n * p[i] > 0.1`.
///
/// `p` must already be FixupProb-normalized; its contents are left
/// untouched (stock works on a duplicated/coerced `prob` vector).
pub fn walker_ProbSampleReplace_r(n: usize, p: &mut [f64], nans: usize) -> Vec<c_int> {
    // Alias array `a` and the HL working vector. Stock passes INTEGER(x) of a
    // fresh allocVector(INTSXP, n) as `a`; entries whose alias is never built
    // are only ever consulted when their q >= 1, where the alias is unused.
    let mut a = vec![0 as c_int; n];
    // HL[0..h) labels the entries with q < 1 and HL[l..n) those >= 1.
    let mut hl = vec![0 as c_int; n];
    let mut q = vec![0.0_f64; n];

    /* Create the alias tables.
    By rounding error we could have q[i] < 1. or > 1. for all entries. */
    let mut h = 0usize; // H write cursor (grows up)
    let mut l = n; // L write cursor (grows down)
    for i in 0..n {
        q[i] = p[i] * n as f64;
        if q[i] < 1.0 {
            hl[h] = i as c_int;
            h += 1;
        } else {
            l -= 1;
            hl[l] = i as c_int;
        }
    }
    if h > 0 && l < n {
        /* So some q[i] are >= 1 and some < 1 */
        for k in 0..n - 1 {
            let i = hl[k] as usize;
            let j = hl[l] as usize; // j = *L
            a[i] = j as c_int;
            q[j] += q[i] - 1.0;
            if q[j] < 1.0 {
                l += 1;
            }
            if l >= n {
                break; /* now all are >= 1 */
            }
        }
    }
    for (i, qi) in q.iter_mut().enumerate() {
        *qi += i as f64;
    }

    /* generate sample */
    let sample_kind = r_sample_kind();
    let mut ans = vec![0 as c_int; nans];
    for i in 0..nans {
        let k: usize;
        let r_u: f64;
        if sample_kind == Sampletype::ROUNDING {
            r_u = r_unif_rand() * n as f64;
            k = r_u as usize;
        } else {
            k = r_R_unif_index(n as f64) as usize;
            r_u = k as f64 + r_unif_rand();
        }
        ans[i] = if r_u < q[k] { k as c_int + 1 } else { a[k] + 1 };
    }
    ans
}

/// Unequal probability sampling without replacement using the R RNG.
pub fn ProbSampleNoReplace_r(n: usize, p: &mut [f64], nans: usize) -> Vec<c_int> {
    let mut perm: Vec<c_int> = (1..=n as c_int).collect();
    revsort_with_perm(p, &mut perm, n);
    let mut ans = vec![0 as c_int; nans];
    let mut totalmass = 1.0_f64;
    for i in 0..nans {
        let r_t = totalmass * r_unif_rand();
        let mut mass = 0.0;
        let mut j = n - 1 - i;
        for jj in 0..j {
            mass += p[jj];
            if r_t <= mass {
                j = jj;
                break;
            }
        }
        ans[i] = perm[j];
        totalmass -= p[j];
        for k in j..(n - 1 - i) {
            p[k] = p[k + 1];
            perm[k] = perm[k + 1];
        }
    }
    ans
}

/// Equal probability sampling with replacement using the R RNG.
pub fn SampleReplace_r(n: usize, nans: usize) -> Vec<c_int> {
    let mut ans = vec![0 as c_int; nans];
    for i in 0..nans {
        ans[i] = r_R_unif_index(n as f64) as c_int + 1;
    }
    ans
}


// ---------------------------------------------------------------------------
// S compatibility stubs
// ---------------------------------------------------------------------------



// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
