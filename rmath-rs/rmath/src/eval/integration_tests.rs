#![allow(non_snake_case, non_upper_case_globals, unused_variables)]

//! Integration tests for the eval pipeline.
//!
//! Tests the full lifecycle: construct SEXP -> evaluate -> read result.

use std::os::raw::c_int;

use crate::sexp::constructors::*;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::output::{start_capture, stop_capture};
use crate::sexp::safe::Sexp;

fn make_test_env() -> SEXP {
    unsafe {
        let env = crate::sexp::memory_ext::allocSExp(SEXPTYPE::ENVSXP);
        if env.is_null() {
            return R_NilValue();
        }
        (*env).data.envsxp.frame = R_NilValue();
        (*env).data.envsxp.enclos = R_NilValue();
        (*env).data.envsxp.hashtab = R_NilValue();
        env
    }
}

#[test]
fn test_self_evaluating_integer() {
    unsafe {
        let val = Rf_ScalarInteger(42);
        assert!(!val.is_null());
        let env = make_test_env();
        let result = crate::eval::eval::Rf_eval(val, env);
        assert!(!result.is_null());
        assert_eq!((*result).sxpinfo.type_of(), SEXPTYPE::INTSXP);
        let data = (*result).gengc_next_node as *const c_int;
        assert_eq!(*data, 42);
    }
}

#[test]
fn test_self_evaluating_real() {
    unsafe {
        let val = Rf_ScalarReal(3.14);
        assert!(!val.is_null());
        let env = make_test_env();
        let result = crate::eval::eval::Rf_eval(val, env);
        assert!(!result.is_null());
        assert_eq!((*result).sxpinfo.type_of(), SEXPTYPE::REALSXP);
        let data = (*result).gengc_next_node as *const f64;
        assert!((*data - 3.14).abs() < 1e-10);
    }
}

#[test]
fn test_self_evaluating_null() {
    unsafe {
        let val = R_NilValue();
        let env = make_test_env();
        let result = crate::eval::eval::Rf_eval(val, env);
        assert_eq!(result, R_NilValue());
    }
}

#[test]
fn test_self_evaluating_logical() {
    unsafe {
        let val = Rf_ScalarLogical(1);
        assert!(!val.is_null());
        let env = make_test_env();
        let result = crate::eval::eval::Rf_eval(val, env);
        assert!(!result.is_null());
        assert_eq!((*result).sxpinfo.type_of(), SEXPTYPE::LGLSXP);
        let data = (*result).gengc_next_node as *const c_int;
        assert_eq!(*data, 1);
    }
}

#[test]
fn test_self_evaluating_string() {
    unsafe {
        let val = Rf_mkString(c"hello".as_ptr());
        assert!(!val.is_null());
        let env = make_test_env();
        let result = crate::eval::eval::Rf_eval(val, env);
        assert!(!result.is_null());
        assert_eq!((*result).sxpinfo.type_of(), SEXPTYPE::STRSXP);
    }
}

#[test]
fn test_eval_integer_vector() {
    unsafe {
        let vec = Rf_allocVector(SEXPTYPE::INTSXP.0, 5);
        assert!(!vec.is_null());
        let data = (*vec).gengc_next_node as *mut c_int;
        for i in 0..5 {
            *data.add(i) = ((i + 1) * 10) as c_int;
        }

        let env = make_test_env();
        let result = crate::eval::eval::Rf_eval(vec, env);
        assert!(!result.is_null());
        assert_eq!((*result).sxpinfo.type_of(), SEXPTYPE::INTSXP);

        let s = Sexp::from_raw(result).unwrap();
        assert_eq!(s.len(), 5);
        assert_eq!(s.integer_elt(0), Some(10));
        assert_eq!(s.integer_elt(4), Some(50));
    }
}

#[test]
fn test_eval_real_vector() {
    unsafe {
        let vec = Rf_allocVector(SEXPTYPE::REALSXP.0, 3);
        assert!(!vec.is_null());
        let data = (*vec).gengc_next_node as *mut f64;
        *data = 1.1;
        *data.add(1) = 2.2;
        *data.add(2) = 3.3;

        let env = make_test_env();
        let result = crate::eval::eval::Rf_eval(vec, env);
        assert!(!result.is_null());

        let s = Sexp::from_raw(result).unwrap();
        assert_eq!(s.len(), 3);
        assert!((s.real_elt(0).unwrap() - 1.1).abs() < 1e-10);
        assert!((s.real_elt(2).unwrap() - 3.3).abs() < 1e-10);
    }
}

#[test]
fn test_eval_safe_wrapper() {
    let env_raw = make_test_env();
    let env = unsafe { Sexp::from_raw_unchecked(env_raw) };
    let val = unsafe {
        let v = Rf_ScalarInteger(99);
        Sexp::from_raw_unchecked(v)
    };

    let result = crate::eval::eval::eval_safe(val, env);
    assert!(result.is_ok());
    let r = result.unwrap();
    assert_eq!(r.integer_elt(0), Some(99));
}

#[test]
fn test_eval_null_via_safe() {
    let env_raw = make_test_env();
    let env = unsafe { Sexp::from_raw_unchecked(env_raw) };
    let null = unsafe { Sexp::from_raw_unchecked(R_NilValue()) };

    let result = crate::eval::eval::eval_safe(null, env);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().typeof_(), SEXPTYPE::NILSXP);
}

#[test]
fn test_altrep_compact_intseq() {
    unsafe {
        let seq = crate::mainutils::altrep::R_compact_intseq(1, 5);
        assert!(!seq.is_null());

        let elt = crate::mainutils::altrep::ALTINTEGER_ELT(seq, 0);
        assert_eq!(elt, 1);

        let elt2 = crate::mainutils::altrep::ALTINTEGER_ELT(seq, 4);
        assert_eq!(elt2, 5);
    }
}

#[test]
fn test_altrep_compact_realseq() {
    unsafe {
        let seq = crate::mainutils::altrep::R_compact_realseq(0.0, 1.0, 5);
        assert!(!seq.is_null());

        let elt = crate::mainutils::altrep::ALTREAL_ELT(seq, 0);
        assert!((elt - 0.0).abs() < 1e-10);

        let elt2 = crate::mainutils::altrep::ALTREAL_ELT(seq, 4);
        assert!((elt2 - 4.0).abs() < 1e-10);
    }
}

#[test]
fn test_altrep_new_altrep_data_roundtrip() {
    unsafe {
        let class_sym = Rf_ScalarInteger(42);
        let data1 = Rf_ScalarInteger(100);
        let data2 = Rf_ScalarReal(3.14);
        let altrep = crate::mainutils::altrep::R_new_altrep(class_sym, data1, data2);
        assert!(!altrep.is_null());

        let d1 = crate::mainutils::altrep::R_altrep_data1(altrep);
        assert!(!d1.is_null());
        let d2 = crate::mainutils::altrep::R_altrep_data2(altrep);
        assert!(!d2.is_null());

        let d1_val = Sexp::from_raw_unchecked(d1);
        assert_eq!(d1_val.integer_elt(0), Some(100));

        let d2_val = Sexp::from_raw_unchecked(d2);
        assert!((d2_val.real_elt(0).unwrap() - 3.14).abs() < 1e-10);
    }
}

#[test]
fn test_output_capture_print_integer() {
    unsafe {
        start_capture();
        let val = Rf_ScalarInteger(42);
        crate::sexp::output::Rf_PrintValue(val);
        let output = stop_capture();
        assert!(
            output.stdout.contains("42"),
            "expected 42 in output, got: {}",
            output.stdout
        );
    }
}

#[test]
fn test_output_capture_print_real() {
    unsafe {
        start_capture();
        let val = Rf_ScalarReal(3.14);
        crate::sexp::output::Rf_PrintValue(val);
        let output = stop_capture();
        assert!(
            output.stdout.contains("3.14"),
            "expected 3.14 in output, got: {}",
            output.stdout
        );
    }
}

#[test]
fn test_output_capture_print_null() {
    unsafe {
        start_capture();
        crate::sexp::output::Rf_PrintValue(R_NilValue());
        let output = stop_capture();
        assert!(
            output.stdout.contains("NULL"),
            "expected NULL in output, got: {}",
            output.stdout
        );
    }
}

#[test]
fn test_output_capture_print_logical() {
    unsafe {
        start_capture();
        let val = Rf_ScalarLogical(1);
        crate::sexp::output::Rf_PrintValue(val);
        let output = stop_capture();
        assert!(
            output.stdout.contains("TRUE"),
            "expected TRUE in output, got: {}",
            output.stdout
        );
    }
}

#[test]
fn test_pairlist_eval() {
    unsafe {
        let a = Rf_ScalarInteger(1);
        let b = Rf_ScalarInteger(2);
        let list = Rf_cons(a, Rf_cons(b, R_NilValue()));

        let env = make_test_env();
        let result = crate::eval::eval::Rf_eval(list, env);
        assert!(!result.is_null());
    }
}

#[test]
fn test_arena_alloc_and_eval() {
    unsafe {
        let vec = Rf_allocVector(SEXPTYPE::REALSXP.0, 4);
        assert!(!vec.is_null());

        let data = (*vec).gengc_next_node as *mut f64;
        for i in 0..4 {
            *data.add(i) = i as f64 * 2.0;
        }

        let env = make_test_env();
        let result = crate::eval::eval::Rf_eval(vec, env);
        assert!(!result.is_null());

        let s = Sexp::from_raw_unchecked(result);
        assert_eq!(s.len(), 4);
        assert!((s.real_elt(0).unwrap() - 0.0).abs() < 1e-10);
        assert!((s.real_elt(3).unwrap() - 6.0).abs() < 1e-10);
    }
}

#[test]
fn test_cons_and_car_cdr() {
    unsafe {
        let a = Rf_ScalarInteger(10);
        let b = Rf_ScalarInteger(20);
        let cell = Rf_cons(a, b);
        assert!(!cell.is_null());

        let s = Sexp::from_raw_unchecked(cell);
        let car = s.car().unwrap();
        let cdr = s.cdr().unwrap();

        assert_eq!(car.integer_elt(0), Some(10));
        assert_eq!(cdr.integer_elt(0), Some(20));
    }
}

#[test]
fn test_gc_after_allocations() {
    unsafe {
        for _ in 0..100 {
            let v = Rf_allocVector(SEXPTYPE::INTSXP.0, 10);
            assert!(!v.is_null());
        }
    }
    crate::sexp::memory::reset_arena();
}

// ---------------------------------------------------------------------------
// Math comparison against known R values
// ---------------------------------------------------------------------------

#[test]
fn test_dnorm_vs_r() {
    let cases = [
        (0.0, 0.0, 1.0, 0.3989422804014327),
        (1.0, 0.0, 1.0, 0.24197072451914337),
        (-1.0, 0.0, 1.0, 0.24197072451914337),
        (2.0, 0.0, 1.0, 0.05399096651318806),
        (0.0, 5.0, 2.0, 0.00876415024678427),
        (5.0, 5.0, 2.0, 0.19947114020071622),
    ];
    for (x, mean, sd, expected) in cases {
        let got = crate::dist::normal::dnorm(x, mean, sd, 0);
        assert!(
            (got - expected).abs() < 1e-10,
            "dnorm({x}, {mean}, {sd}): expected {expected}, got {got}"
        );
    }
}

#[test]
fn test_pnorm_vs_r() {
    let cases = [
        (0.0, 0.0, 1.0, true, 0.5),
        (1.96, 0.0, 1.0, true, 0.9750021048517795),
        (-1.96, 0.0, 1.0, true, 0.0249978951482205),
        (1.0, 0.0, 1.0, true, 0.8413447460685429),
        (0.0, 0.0, 1.0, false, 0.5),
        (2.0, 0.0, 1.0, false, 0.02275013194817921),
    ];
    for (x, mean, sd, lower, expected) in cases {
        let lt = if lower { 1 } else { 0 };
        let got = crate::dist::normal::pnorm(x, mean, sd, lt, 0);
        assert!(
            (got - expected).abs() < 1e-10,
            "pnorm({x}, {mean}, {sd}, lower={lower}): expected {expected}, got {got}"
        );
    }
}

#[test]
fn test_qnorm_vs_r() {
    let cases = [
        (0.5, 0.0, 1.0, true, 0.0),
        (0.975, 0.0, 1.0, true, 1.959963984540054),
        (0.025, 0.0, 1.0, true, -1.959963984540054),
        (0.99, 0.0, 1.0, true, 2.3263478740408408),
        (0.01, 0.0, 1.0, true, -2.3263478740408408),
    ];
    for (p, mean, sd, lower, expected) in cases {
        let lt = if lower { 1 } else { 0 };
        let got = crate::dist::normal::qnorm(p, mean, sd, lt, 0);
        assert!(
            (got - expected).abs() < 1e-8,
            "qnorm({p}, {mean}, {sd}, lower={lower}): expected {expected}, got {got}"
        );
    }
}

#[test]
fn test_bessel_j_vs_r() {
    let cases = [
        (0.0, 0.0, 1.0),
        (1.0, 0.0, 0.7651976865579666),
        (2.0, 0.0, 0.2238907791412357),
        (10.0, 0.0, -0.2459357644513483),
    ];
    for (x, alpha, expected) in cases {
        let got = crate::special::bessel::bessel_j(x, alpha);
        assert!(
            (got - expected).abs() < 1e-8,
            "bessel_j({x}, {alpha}): expected {expected}, got {got}"
        );
    }
}

#[test]
fn test_bessel_i_vs_r() {
    let cases = [
        (0.0, 0.0, 1.0),
        (1.0, 0.0, 1.2660658777520082),
        (2.0, 1.0, 1.590636854637329),
        (5.0, 0.0, 27.23987182360446),
    ];
    for (x, alpha, expected) in cases {
        let got = crate::special::bessel::bessel_i(x, alpha, false);
        assert!(
            (got - expected).abs() < 1e-8,
            "bessel_i({x}, {alpha}): expected {expected}, got {got}"
        );
    }
}

#[test]
fn test_bessel_y_vs_r() {
    let cases = [
        (1.0, 0.0, 0.0882569642156769),
        (2.0, 0.0, 0.5103756726497451),
        (5.0, 0.0, -0.3085176253247014),
        (10.0, 0.0, 0.05567116252151137),
    ];
    for (x, alpha, expected) in cases {
        let got = crate::special::bessel::bessel_y(x, alpha);
        assert!(
            (got - expected).abs() < 1e-8,
            "bessel_y({x}, {alpha}): expected {expected}, got {got}"
        );
    }
}

#[test]
fn test_gamma_fn_vs_r() {
    let cases = [
        (1.0, 1.0),
        (2.0, 1.0),
        (3.0, 2.0),
        (4.0, 6.0),
        (5.0, 24.0),
        (0.5, 1.7724538509055159),
        (1.5, 0.886226925452758),
    ];
    for (x, expected) in cases {
        let got = libm::tgamma(x);
        assert!(
            (got - expected).abs() < 1e-10,
            "gamma({x}): expected {expected}, got {got}"
        );
    }
}

#[test]
fn test_lgamma_fn_vs_r() {
    let cases = [
        (1.0, 0.0),
        (2.0, 0.0),
        (3.0, 0.6931471805599453),
        (4.0, 1.791759469228055),
        (0.5, 0.5723649429247001),
    ];
    for (x, expected) in cases {
        let got = libm::lgamma(x);
        assert!(
            (got - expected).abs() < 1e-10,
            "lgamma({x}): expected {expected}, got {got}"
        );
    }
}

#[test]
fn test_exp_log_roundtrip() {
    for x in [0.5, 1.0, 2.0, 10.0, 100.0] {
        let roundtrip = libm::log(libm::exp(x));
        assert!((roundtrip - x).abs() < 1e-10, "log(exp({x})) = {roundtrip}");
    }
}

#[test]
fn test_trig_identities() {
    for x in [0.0, 0.5, 1.0, 1.5, 3.14159] {
        let sin2 = libm::sin(x) * libm::sin(x);
        let cos2 = libm::cos(x) * libm::cos(x);
        assert!(
            (sin2 + cos2 - 1.0).abs() < 1e-10,
            "sin²({x}) + cos²({x}) = {}",
            sin2 + cos2
        );
    }
}

// ---------------------------------------------------------------------------
// Property-based arena tests
// ---------------------------------------------------------------------------

#[test]
fn test_arena_alloc_dealloc_pattern() {
    let mut arena = crate::sexp::memory::RArena::new();
    assert_eq!(arena.node_count(), 0);

    let p1 = arena.alloc_vector(SEXPTYPE::INTSXP, 10);
    assert!(!p1.is_null());
    assert_eq!(arena.node_count(), 1);

    let p2 = arena.alloc_vector(SEXPTYPE::REALSXP, 5);
    assert!(!p2.is_null());
    assert_eq!(arena.node_count(), 2);

    assert_ne!(p1, p2);
}

#[test]
fn test_arena_large_alloc() {
    let mut arena = crate::sexp::memory::RArena::new();
    let n = 10000;
    let p = arena.alloc_vector(SEXPTYPE::REALSXP, n);
    assert!(!p.is_null());
    assert_eq!(arena.node_count(), 1);

    let s = Sexp::from_raw(p).unwrap();
    assert_eq!(s.len(), n as i64);
}

#[test]
fn test_arena_many_small_allocs() {
    let mut arena = crate::sexp::memory::RArena::new();
    let mut ptrs = Vec::new();
    for _ in 0..500 {
        let p = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
        assert!(!p.is_null());
        ptrs.push(p);
    }
    assert_eq!(arena.node_count(), 500);

    for (i, &p) in ptrs.iter().enumerate() {
        if i > 0 {
            assert_ne!(p, ptrs[i - 1]);
        }
    }
}

#[test]
fn test_arena_node_types() {
    let mut arena = crate::sexp::memory::RArena::new();

    let types = [
        SEXPTYPE::INTSXP,
        SEXPTYPE::REALSXP,
        SEXPTYPE::LGLSXP,
        SEXPTYPE::STRSXP,
        SEXPTYPE::VECSXP,
        SEXPTYPE::LISTSXP,
        SEXPTYPE::ENVSXP,
    ];

    for (i, &ty) in types.iter().enumerate() {
        let p = arena.alloc_node(ty);
        assert!(!p.is_null(), "alloc_node({ty:?}) failed at index {i}");
        unsafe {
            assert_eq!((*p).sxpinfo.type_of(), ty);
        }
    }
    assert_eq!(arena.node_count(), types.len());
}

#[test]
fn test_arena_independent_sessions() {
    let mut arena1 = crate::sexp::memory::RArena::new();
    let mut arena2 = crate::sexp::memory::RArena::new();

    let p1 = arena1.alloc_vector(SEXPTYPE::INTSXP, 5);
    let p2 = arena2.alloc_vector(SEXPTYPE::REALSXP, 5);

    assert_eq!(arena1.node_count(), 1);
    assert_eq!(arena2.node_count(), 1);
    assert_ne!(p1, p2);
}

#[test]
fn test_protect_stack_integrity() {
    unsafe {
        use crate::sexp::protect::{Rf_protect, Rf_unprotect, protect};

        let v1 = Rf_ScalarInteger(1);
        let v2 = Rf_ScalarReal(2.0);
        let v3 = Rf_allocVector(SEXPTYPE::INTSXP.0, 10);

        let _guard = protect(v1);
        Rf_protect(v2);
        Rf_protect(v3);

        Rf_unprotect(2);
    }
}

#[test]
fn test_session_eval_parsed_exprs() {
    let mut session = crate::android::RSession::new();

    let r1 = session.eval("42");
    assert!(r1.output.contains("42"));

    let r2 = session.eval("3.14");
    assert!(r2.output.contains("3.14"));

    let r3 = session.eval("\"hello\"");
    assert!(r3.output.contains("hello"));

    let r4 = session.eval("TRUE");
    assert!(r4.output.contains("TRUE") || r4.output.contains("1"));

    let r5 = session.eval("NULL");
    assert_eq!(r5.output, "NULL");
}

#[test]
fn test_eval_arithmetic_direct() {
    use crate::eval::eval::eval_safe;
    use crate::eval::parser;
    use crate::sexp::init;

    unsafe {
        init::initialize_r();
    }

    let mut arena = crate::sexp::memory::RArena::new();
    let expr = parser::parse("1 + 2", &mut arena).expect("parse failed");

    let global_env = unsafe { crate::sexp::globals::R_GlobalEnv() };
    let env = unsafe { crate::sexp::safe::Sexp::from_raw_unchecked(global_env) };
    let e = unsafe { crate::sexp::safe::Sexp::from_raw_unchecked(expr) };

    let result = eval_safe(e, env);
    assert!(result.is_ok(), "eval failed: {:?}", result);
    let val = result.unwrap();
    let v = val.real_elt(0).unwrap_or(0.0);
    assert!((v - 3.0).abs() < 1e-10, "expected 3.0, got {}", v);

    unsafe {
        init::shutdown_r();
    }
}
