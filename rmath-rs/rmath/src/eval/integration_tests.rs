#![allow(non_snake_case, non_upper_case_globals, unused_variables)]

//! Integration tests for the eval pipeline.
//!
//! Tests the full lifecycle: construct SEXP -> evaluate -> read result.

use std::os::raw::c_int;

use crate::sexp::constructors::*;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory::with_arena;
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
