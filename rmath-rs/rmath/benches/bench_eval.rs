#![feature(test)]
#![allow(non_snake_case, non_upper_case_globals)]

extern crate test;

use std::os::raw::c_int;
use test::Bencher;

use rmath::sexp::constructors::*;
use rmath::sexp::ffi::{SEXP, SEXPTYPE};
use rmath::sexp::globals::R_NilValue;
use rmath::sexp::safe::Sexp;

fn make_env() -> SEXP {
    unsafe {
        let env = rmath::sexp::memory_ext::allocSExp(SEXPTYPE::ENVSXP);
        if env.is_null() {
            return R_NilValue();
        }
        (*env).data.envsxp.frame = R_NilValue();
        (*env).data.envsxp.enclos = R_NilValue();
        (*env).data.envsxp.hashtab = R_NilValue();
        env
    }
}

#[bench]
fn bench_alloc_integer_vector(b: &mut Bencher) {
    b.iter(|| unsafe {
        rmath::sexp::memory::reset_arena();
        let v = Rf_allocVector(SEXPTYPE::INTSXP.0, 1000);
        assert!(!v.is_null());
        v
    });
}

#[bench]
fn bench_alloc_real_vector(b: &mut Bencher) {
    b.iter(|| unsafe {
        rmath::sexp::memory::reset_arena();
        let v = Rf_allocVector(SEXPTYPE::REALSXP.0, 1000);
        assert!(!v.is_null());
        v
    });
}

#[bench]
fn bench_alloc_string_vector(b: &mut Bencher) {
    b.iter(|| unsafe {
        rmath::sexp::memory::reset_arena();
        let v = Rf_allocVector(SEXPTYPE::STRSXP.0, 100);
        assert!(!v.is_null());
        v
    });
}

#[bench]
fn bench_eval_self_integer(b: &mut Bencher) {
    b.iter(|| unsafe {
        rmath::sexp::memory::reset_arena();
        let env = make_env();
        let val = Rf_ScalarInteger(42);
        rmath::eval::eval::Rf_eval(val, env)
    });
}

#[bench]
fn bench_eval_self_real(b: &mut Bencher) {
    b.iter(|| unsafe {
        rmath::sexp::memory::reset_arena();
        let env = make_env();
        let val = Rf_ScalarReal(3.14);
        rmath::eval::eval::Rf_eval(val, env)
    });
}

#[bench]
fn bench_eval_null(b: &mut Bencher) {
    b.iter(|| unsafe {
        rmath::sexp::memory::reset_arena();
        let env = make_env();
        rmath::eval::eval::Rf_eval(R_NilValue(), env)
    });
}

#[bench]
fn bench_cons_pairlist_100(b: &mut Bencher) {
    b.iter(|| unsafe {
        rmath::sexp::memory::reset_arena();
        let mut list = R_NilValue();
        for _ in 0..100 {
            let val = Rf_ScalarInteger(1);
            list = Rf_cons(val, list);
        }
        list
    });
}

#[bench]
fn bench_altrep_intseq_create(b: &mut Bencher) {
    b.iter(|| unsafe {
        rmath::sexp::memory::reset_arena();
        rmath::mainutils::altrep::R_compact_intseq(1, 1000)
    });
}

#[bench]
fn bench_altrep_intseq_access(b: &mut Bencher) {
    let seq = unsafe { rmath::mainutils::altrep::R_compact_intseq(1, 1000) };
    b.iter(|| unsafe {
        for i in 0..1000 {
            test::black_box(rmath::mainutils::altrep::ALTINTEGER_ELT(seq, i));
        }
    });
}

#[bench]
fn bench_altrep_realseq_create(b: &mut Bencher) {
    b.iter(|| unsafe {
        rmath::sexp::memory::reset_arena();
        rmath::mainutils::altrep::R_compact_realseq(0.0, 0.001, 1000)
    });
}

#[bench]
fn bench_altrep_realseq_access(b: &mut Bencher) {
    let seq = unsafe { rmath::mainutils::altrep::R_compact_realseq(0.0, 1.0, 1000) };
    b.iter(|| unsafe {
        for i in 0..1000 {
            test::black_box(rmath::mainutils::altrep::ALTREAL_ELT(seq, i));
        }
    });
}

#[bench]
fn bench_output_capture(b: &mut Bencher) {
    b.iter(|| {
        rmath::sexp::memory::reset_arena();
        rmath::sexp::output::start_capture();
        unsafe {
            let val = Rf_ScalarInteger(42);
            rmath::sexp::output::Rf_PrintValue(val);
        }
        let output = rmath::sexp::output::stop_capture();
        assert!(!output.stdout.is_empty());
    });
}

#[bench]
fn bench_math_dnorm(b: &mut Bencher) {
    b.iter(|| unsafe { rmath::dist::normal::Rf_dnorm(0.0, 0.0, 1.0, 0) });
}

#[bench]
fn bench_math_pnorm(b: &mut Bencher) {
    b.iter(|| unsafe { rmath::dist::normal::Rf_pnorm(1.96, 0.0, 1.0, 1, 0) });
}

#[bench]
fn bench_math_qnorm(b: &mut Bencher) {
    b.iter(|| unsafe { rmath::dist::normal::Rf_qnorm(0.975, 0.0, 1.0, 1, 0) });
}

#[bench]
fn bench_math_rnorm(b: &mut Bencher) {
    b.iter(|| unsafe { rmath::dist::normal::Rf_rnorm(0.0, 1.0) });
}

#[bench]
fn bench_unif_rand(b: &mut Bencher) {
    b.iter(|| rmath::rng::unif_rand());
}
