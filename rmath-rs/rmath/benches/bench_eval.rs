#![allow(non_snake_case, non_upper_case_globals)]

use std::hint::black_box;
use std::time::Instant;

use rmath::sexp::builder::{PairlistBuilder, scalar_integer_in};
use rmath::sexp::output::{print_value, start_capture, stop_capture};
use rmath::sexp::{RSession, SEXPTYPE, Sexp};

fn run<T>(name: &str, iterations: usize, mut f: impl FnMut() -> T) {
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(f());
    }
    eprintln!("{name}: {:?} for {iterations} iterations", start.elapsed());
}

fn main() {
    const DEFAULT_ITERS: usize = 1_000;

    run("alloc_integer_vector", DEFAULT_ITERS, || {
        let mut session = RSession::new();
        session
            .with_arena(|arena| {
                arena
                    .alloc_vector_sexp(SEXPTYPE::INTSXP, 1000)
                    .unwrap()
                    .len()
            })
            .unwrap()
    });

    run("alloc_real_vector", DEFAULT_ITERS, || {
        let mut session = RSession::new();
        session
            .with_arena(|arena| {
                arena
                    .alloc_vector_sexp(SEXPTYPE::REALSXP, 1000)
                    .unwrap()
                    .len()
            })
            .unwrap()
    });

    run("alloc_string_vector", DEFAULT_ITERS, || {
        let mut session = RSession::new();
        session
            .with_arena(|arena| {
                arena
                    .alloc_vector_sexp(SEXPTYPE::STRSXP, 100)
                    .unwrap()
                    .len()
            })
            .unwrap()
    });

    run("eval_self_integer", DEFAULT_ITERS, || {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_code_with_output_capture("42L");
        result.unwrap().try_integer_elt(0).unwrap()
    });

    run("eval_self_real", DEFAULT_ITERS, || {
        let mut session = RSession::new();
        let (result, _, _) =
            session.eval_code_with_output_capture(&std::f64::consts::PI.to_string());
        result.unwrap().try_as_f64().unwrap()
    });

    run("eval_null", DEFAULT_ITERS, || {
        let mut session = RSession::new();
        let (result, _, _) = session.eval_code_with_output_capture("NULL");
        result.unwrap().is_nil()
    });

    run("cons_pairlist_100", DEFAULT_ITERS, || {
        let mut session = RSession::new();
        session
            .with_arena(|arena| {
                let mut builder = PairlistBuilder::new();
                for _ in 0..100 {
                    builder = builder.push_untagged_value(Sexp::nil());
                }
                builder.build_in(arena).unwrap().len()
            })
            .unwrap()
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

    run("output_capture", DEFAULT_ITERS, || {
        let mut session = RSession::new();
        session
            .with_arena(|arena| {
                start_capture();
                let val = scalar_integer_in(arena, 42).unwrap();
                print_value(val);
                let output = stop_capture();
                assert!(!output.stdout.is_empty());
                output.stdout.len()
            })
            .unwrap()
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
