//! Allocation notifications must observe completed constructor results and may
//! reenter full GC without reclaiming intermediate constructor graphs.
use super::*;
use crate::sexp::{gengc, heap::CheckedNode, instance, protect::protect, session::RSession};
use std::cell::{Cell, RefCell};

#[derive(Clone, Copy)]
enum Expected {
    Scalar(SEXPTYPE),
    String,
    Language(usize),
    Primitive,
    Closure {
        formals: SEXP,
        body: SEXP,
        env: SEXP,
    },
    Symbol {
        name: SEXP,
        value: SEXP,
    },
}

thread_local! {
    static EXPECTED: RefCell<Option<Expected>> = const { RefCell::new(None) };
    static NOTIFICATIONS: Cell<usize> = const { Cell::new(0) };
    static COMPLETED: Cell<bool> = const { Cell::new(false) };
}

fn arm(expected: Expected) {
    EXPECTED.with(|slot| *slot.borrow_mut() = Some(expected));
    NOTIFICATIONS.set(0);
    COMPLETED.set(false);
    gengc::register_gc_callback(Box::new(|_| {
        NOTIFICATIONS.set(NOTIFICATIONS.get() + 1);
        // No arena/instance/header loan crosses the reentrant collection.
        let nodes: Vec<(SEXP, CheckedNode)> =
            instance::with_required_current_instance(|owner| unsafe {
                (*owner)
                    .arena
                    .active_nodes()
                    .map(|raw| (raw, (*owner).arena.node_token(raw).unwrap()))
                    .collect()
            });
        gengc::full_gc();
        let expected = EXPECTED.with(|slot| slot.borrow().unwrap());
        for (raw, allocation) in nodes {
            if !allocation.is_live() {
                continue;
            }
            let Some((raw, current)) = memory::checked_projection(raw) else {
                continue;
            };
            assert_eq!(current, allocation);
            // SAFETY: this callback has no further R reentry while inspecting
            // canonical live projections whose owners survived nested GC.
            let complete = unsafe { matches_expectation(raw, expected) };
            COMPLETED.set(COMPLETED.get() || complete);
        }
    }));
    unsafe { crate::mainutils::memory_main::R_gc_torture(1, 1, 0) };
}

unsafe fn matches_expectation(raw: SEXP, expected: Expected) -> bool {
    use crate::sexp::accessors::*;
    unsafe {
        let kind = (*raw).sxpinfo.type_of();
        match expected {
            Expected::Scalar(wanted) if kind == wanted => match wanted {
                SEXPTYPE::LGLSXP => *LOGICAL(raw) == 1,
                SEXPTYPE::INTSXP => *INTEGER(raw) == 73,
                SEXPTYPE::REALSXP => *REAL(raw) == 12.5,
                SEXPTYPE::CPLXSXP => {
                    let value = *COMPLEX(raw);
                    value.r == 12.5 && value.i == -3.0
                }
                SEXPTYPE::RAWSXP => *RAW(raw) == 0xa5,
                _ => false,
            },
            Expected::String if kind == SEXPTYPE::STRSXP => {
                let child = STRING_ELT(raw, 0);
                memory::checked_projection(child).is_some_and(|(child, _)| {
                    (*child).sxpinfo.type_of() == SEXPTYPE::CHARSXP
                        && std::ffi::CStr::from_ptr(CHAR(child)).to_bytes() == b"complete"
                })
            }
            Expected::Language(length) if kind == SEXPTYPE::LANGSXP => {
                let mut cell = raw;
                for index in 0..length {
                    let Some((canonical, _)) = memory::checked_projection(cell) else {
                        return false;
                    };
                    if (*canonical).sxpinfo.type_of()
                        != if index == 0 {
                            SEXPTYPE::LANGSXP
                        } else {
                            SEXPTYPE::LISTSXP
                        }
                    {
                        return false;
                    }
                    cell = (*canonical).data.listsxp.cdrval;
                }
                cell == R_NilValue()
            }
            Expected::Primitive => kind == SEXPTYPE::BUILTINSXP && PRIMOFFSET(raw) == 7,
            Expected::Closure { formals, body, env } => {
                kind == SEXPTYPE::CLOSXP
                    && FORMALS(raw) == formals
                    && BODY(raw) == body
                    && CLOENV(raw) == env
            }
            Expected::Symbol { name, value } => {
                kind == SEXPTYPE::SYMSXP && PRINTNAME(raw) == name && SYMVALUE(raw) == value
            }
            _ => false,
        }
    }
}

fn assert_completed(raw: SEXP) {
    assert!(
        NOTIFICATIONS.get() > 0,
        "no allocating collection callback ran"
    );
    assert!(
        COMPLETED.get(),
        "callback observed an unfinished constructor"
    );
    assert!(
        memory::checked_projection(raw).is_some(),
        "constructor returned a retired allocation"
    );
    unsafe { crate::mainutils::memory_main::R_gc_torture(0, 0, 0) };
    EXPECTED.with(|slot| *slot.borrow_mut() = None);
}

#[test]
fn scalar_values_are_initialized_before_reentrant_collection() {
    for kind in [
        SEXPTYPE::LGLSXP,
        SEXPTYPE::INTSXP,
        SEXPTYPE::REALSXP,
        SEXPTYPE::CPLXSXP,
        SEXPTYPE::RAWSXP,
    ] {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            arm(Expected::Scalar(kind));
            let result = match kind {
                SEXPTYPE::LGLSXP => Rf_ScalarLogical(1),
                SEXPTYPE::INTSXP => Rf_ScalarInteger(73),
                SEXPTYPE::REALSXP => Rf_ScalarReal(12.5),
                SEXPTYPE::CPLXSXP => {
                    Rf_ScalarComplex(super::super::ffi::Rcomplex { r: 12.5, i: -3.0 })
                }
                SEXPTYPE::RAWSXP => Rf_ScalarRaw(0xa5),
                _ => unreachable!(),
            };
            assert_completed(result);
            assert!(matches_expectation(result, Expected::Scalar(kind)));
        });
    }
}

#[test]
fn strings_publish_their_complete_child_before_reentrant_collection() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        arm(Expected::String);
        let result = Rf_mkString(c"complete".as_ptr());
        assert_completed(result);
        assert!(matches_expectation(result, Expected::String));
    });
}

#[test]
fn language_constructors_publish_the_complete_call_before_collection() {
    for length in 2..=5 {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let values = [
                Rf_ScalarInteger(1),
                Rf_ScalarInteger(2),
                Rf_ScalarInteger(3),
                Rf_ScalarInteger(4),
            ];
            let roots: Vec<_> = values.into_iter().map(|value| protect(value)).collect();
            arm(Expected::Language(length));
            let result = match length {
                2 => Rf_lang2(R_NilValue(), values[0]),
                3 => Rf_lang3(R_NilValue(), values[0], values[1]),
                4 => Rf_lang4(R_NilValue(), values[0], values[1], values[2]),
                5 => Rf_lang5(R_NilValue(), values[0], values[1], values[2], values[3]),
                _ => unreachable!(),
            };
            assert_completed(result);
            assert!(matches_expectation(result, Expected::Language(length)));
            drop(roots);
        });
    }
}

#[test]
fn structure_constructors_initialize_children_and_offsets_before_collection() {
    for variant in 0..3 {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let value = Rf_ScalarInteger(73);
            let value_root = protect(value);
            let name = Rf_mkChar(c"..2".as_ptr());
            let name_root = protect(name);
            let formal = Rf_cons(R_NilValue(), R_NilValue());
            let formal_root = protect(formal);
            let expected = match variant {
                0 => Expected::Primitive,
                1 => Expected::Closure {
                    formals: formal,
                    body: value,
                    env: super::super::globals::R_GlobalEnv(),
                },
                2 => Expected::Symbol { name, value },
                _ => unreachable!(),
            };
            arm(expected);
            let result = match variant {
                0 => crate::mainutils::dstruct::mkPRIMSXP(7, 1),
                1 => crate::mainutils::dstruct::mkCLOSXP(formal, value, R_NilValue()),
                2 => crate::mainutils::dstruct::mkSYMSXP(name, value),
                _ => unreachable!(),
            };
            assert_completed(result);
            assert!(matches_expectation(result, expected));
            drop((formal_root, name_root, value_root));
        });
    }
}

#[test]
fn string_coercion_restores_the_live_original_digits_option_after_collection() {
    use crate::sexp::accessors::*;
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let digits = crate::sexp::symbol::Rf_install(c"digits".as_ptr());
        let original = Rf_ScalarInteger(4);
        let original_allocation = memory::checked_projection(original).unwrap().1;
        crate::mainutils::options::R_SetOption(digits, original);
        // SetOption's translated entrypoint also installs a preserve. Remove
        // that extra lease so the option map/binding owns the test value.
        crate::sexp::protect::R_ReleaseObject(original);
        let input = Rf_allocVector(SEXPTYPE::REALSXP, 3);
        let input_root = protect(input);
        for (index, value) in [1.23456789, 2.34567891, 3.45678912].into_iter().enumerate() {
            REAL(input).add(index).write(value);
        }
        // The option state is the original value's only root. Replacing it for
        // conversion must retain ownership until restoration has finished.
        gengc::register_gc_callback(Box::new(|_| {
            gengc::full_gc();
        }));
        crate::mainutils::memory_main::R_gc_torture(1, 1, 0);
        let result = crate::mainutils::coerce::coerceToString(input);
        crate::mainutils::memory_main::R_gc_torture(0, 0, 0);
        assert!(original_allocation.is_live());
        let restored = crate::mainutils::options::GetOption1(digits);
        assert_eq!(restored, original);
        assert_eq!(
            memory::checked_projection(restored).unwrap().1,
            original_allocation
        );
        assert_eq!(*INTEGER(restored), 4);
        assert_eq!(XLENGTH(result), 3);
        assert_eq!(
            std::ffi::CStr::from_ptr(CHAR(STRING_ELT(result, 0))).to_bytes(),
            b"1.23456789"
        );
        drop(input_root);
    });
}
