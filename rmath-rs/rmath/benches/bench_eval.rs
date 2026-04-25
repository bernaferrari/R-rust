#![allow(non_snake_case, non_upper_case_globals)]

use std::hint::black_box;
use std::time::Instant;

use rmath::sexp::constructors::{Rf_ScalarInteger, Rf_ScalarReal, Rf_allocVector, Rf_cons};
use rmath::sexp::ffi::{SEXP, SEXPTYPE};
use rmath::sexp::globals::R_NilValue;

const DEFAULT_ITERS: usize = 1_000;

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

fn run<T>(name: &str, iterations: usize, mut f: impl FnMut() -> T) {
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(f());
    }
    eprintln!("{name}: {:?} for {iterations} iterations", start.elapsed());
}

fn main() {
    run("alloc_integer_vector", DEFAULT_ITERS, || unsafe {
        rmath::sexp::memory::reset_arena();
        let v = Rf_allocVector(SEXPTYPE::INTSXP.0, 1000);
        assert!(!v.is_null());
        v
    });

    run("alloc_real_vector", DEFAULT_ITERS, || unsafe {
        rmath::sexp::memory::reset_arena();
        let v = Rf_allocVector(SEXPTYPE::REALSXP.0, 1000);
        assert!(!v.is_null());
        v
    });

    run("alloc_string_vector", DEFAULT_ITERS, || unsafe {
        rmath::sexp::memory::reset_arena();
        let v = Rf_allocVector(SEXPTYPE::STRSXP.0, 100);
        assert!(!v.is_null());
        v
    });

    run("eval_self_integer", DEFAULT_ITERS, || unsafe {
        rmath::sexp::memory::reset_arena();
        let env = make_env();
        let val = Rf_ScalarInteger(42);
        rmath::eval::eval::Rf_eval(val, env)
    });

    run("eval_self_real", DEFAULT_ITERS, || unsafe {
        rmath::sexp::memory::reset_arena();
        let env = make_env();
        let val = Rf_ScalarReal(std::f64::consts::PI);
        rmath::eval::eval::Rf_eval(val, env)
    });

    run("eval_null", DEFAULT_ITERS, || unsafe {
        rmath::sexp::memory::reset_arena();
        let env = make_env();
        rmath::eval::eval::Rf_eval(R_NilValue(), env)
    });

    run("cons_pairlist_100", DEFAULT_ITERS, || unsafe {
        rmath::sexp::memory::reset_arena();
        let mut list = R_NilValue();
        for _ in 0..100 {
            let val = Rf_ScalarInteger(1);
            list = Rf_cons(val, list);
        }
        list
    });

    run("altrep_intseq_create", DEFAULT_ITERS, || unsafe {
        rmath::sexp::memory::reset_arena();
        rmath::mainutils::altrep::R_compact_intseq(1, 1000)
    });

    let int_seq = unsafe { rmath::mainutils::altrep::R_compact_intseq(1, 1000) };
    run("altrep_intseq_access", DEFAULT_ITERS, || unsafe {
        for i in 0..1000 {
            black_box(rmath::mainutils::altrep::ALTINTEGER_ELT(int_seq, i));
        }
    });

    run("altrep_realseq_create", DEFAULT_ITERS, || unsafe {
        rmath::sexp::memory::reset_arena();
        rmath::mainutils::altrep::R_compact_realseq(0.0, 0.001, 1000)
    });

    let real_seq = unsafe { rmath::mainutils::altrep::R_compact_realseq(0.0, 1.0, 1000) };
    run("altrep_realseq_access", DEFAULT_ITERS, || unsafe {
        for i in 0..1000 {
            black_box(rmath::mainutils::altrep::ALTREAL_ELT(real_seq, i));
        }
    });

    run("output_capture", DEFAULT_ITERS, || unsafe {
        rmath::sexp::memory::reset_arena();
        rmath::sexp::output::start_capture();
        let val = Rf_ScalarInteger(42);
        rmath::sexp::output::Rf_PrintValue(val);
        let output = rmath::sexp::output::stop_capture();
        assert!(!output.stdout.is_empty());
    });

    run("math_dnorm", DEFAULT_ITERS, || {
        rmath::dist::normal::Rf_dnorm(0.0, 0.0, 1.0, 0)
    });
    run("math_pnorm", DEFAULT_ITERS, || {
        rmath::dist::normal::Rf_pnorm(1.96, 0.0, 1.0, 1, 0)
    });
    run("math_qnorm", DEFAULT_ITERS, || {
        rmath::dist::normal::Rf_qnorm(0.975, 0.0, 1.0, 1, 0)
    });
    run("math_rnorm", DEFAULT_ITERS, || {
        rmath::dist::normal::Rf_rnorm(0.0, 1.0)
    });
    run("unif_rand", DEFAULT_ITERS, rmath::rng::unif_rand);
}
