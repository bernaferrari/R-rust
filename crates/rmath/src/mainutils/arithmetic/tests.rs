use super::*;

use crate::sexp::{
    altrep::{self, AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    object::{PairlistBuilder, SessionNodeFactory, Sexp, SexpError, SexpMut, SexpResult},
    session::RSession,
};
use std::{cell::Cell, rc::Rc};

struct LogicalCoercionProvider {
    mode: Rc<Cell<u8>>,
    reads: Rc<Cell<usize>>,
}

impl AltrepClass for LogicalCoercionProvider {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::LGLSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(4)
    }
    fn element<'s>(
        &self,
        context: &AltrepContext<'s>,
        index: i64,
    ) -> SexpResult<AltrepElement<'s>> {
        self.reads.set(self.reads.get() + 1);
        context.gc()?;
        if index == 1 {
            match self.mode.get() {
                1 => {
                    return Err(SexpError::Altrep {
                        reason: "logical coercion read failure",
                    });
                }
                2 => panic!("logical coercion callback unwind"),
                3 => {
                    context.object().try_logical_elt(index)?;
                }
                _ => {}
            }
        }
        Ok(AltrepElement::Logical(
            [1, 0, NA_INTEGER, 1][index as usize],
        ))
    }
}

fn logical_coercion_source<'s>(
    session: &'s RSession,
    mode: Rc<Cell<u8>>,
    reads: Rc<Cell<usize>>,
) -> Sexp<'s> {
    let class = session
        .register_altrep_class("logical-coercion", LogicalCoercionProvider { mode, reads })
        .unwrap();
    AltrepBuilder::new(class).build().unwrap()
}

#[test]
fn owned_scalar_altrep_arithmetic_retains_detached_operands() {
    struct ScalarProvider {
        kind: SEXPTYPE,
        integer: i32,
        real: f64,
        detach: Rc<Cell<SEXP>>,
        nil: SEXP,
        reads: Rc<Cell<usize>>,
    }
    impl AltrepClass for ScalarProvider {
        fn vector_type(&self) -> SEXPTYPE {
            self.kind
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
            Ok(1)
        }
        fn element<'s>(
            &self,
            context: &AltrepContext<'s>,
            index: i64,
        ) -> SexpResult<AltrepElement<'s>> {
            assert_eq!(index, 0);
            self.reads.set(self.reads.get() + 1);
            let head = self.detach.replace(std::ptr::null_mut());
            if !head.is_null() {
                unsafe { crate::sexp::accessors::SETCDR(head, self.nil) };
            }
            context.gc()?;
            Ok(if self.kind == SEXPTYPE::INTSXP {
                AltrepElement::Integer(self.integer)
            } else {
                AltrepElement::Real(self.real)
            })
        }
    }
    for kind in [SEXPTYPE::INTSXP, SEXPTYPE::REALSXP] {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let detach = Rc::new(Cell::new(std::ptr::null_mut()));
        let left_reads = Rc::new(Cell::new(0));
        let right_reads = Rc::new(Cell::new(0));
        let left_class = session
            .register_altrep_class(
                "scalar-left",
                ScalarProvider {
                    kind,
                    integer: 7,
                    real: 2.5,
                    detach: detach.clone(),
                    nil: factory.nil().as_raw(),
                    reads: left_reads.clone(),
                },
            )
            .unwrap();
        let right_class = session
            .register_altrep_class(
                "scalar-right",
                ScalarProvider {
                    kind,
                    integer: 5,
                    real: 4.25,
                    detach: Rc::new(Cell::new(std::ptr::null_mut())),
                    nil: factory.nil().as_raw(),
                    reads: right_reads.clone(),
                },
            )
            .unwrap();
        let left = AltrepBuilder::new(left_class).build().unwrap();
        let original_class = altrep::altrep_class(&left).unwrap();
        let right = AltrepBuilder::new(right_class).build().unwrap();
        let mut arguments = PairlistBuilder::from_factory(factory.clone());
        arguments.push(left.clone(), None).unwrap();
        // Move away the right operand's only independent root. Initially
        // the argument chain alone retains it for this actual handler call.
        arguments.push(right, None).unwrap();
        let arguments = arguments.finish().unwrap();
        let operation = factory
            .wrap(unsafe { crate::mainutils::names::R_Primitive(c"+".as_ptr()) })
            .unwrap();
        let collections = Rc::new(Cell::new(0));
        let observed = collections.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(observed.get() + 1);
            crate::sexp::gengc::full_gc();
        }));
        let before = crate::sexp::protect::R_ProtectCount();
        detach.set(arguments.as_raw());
        let result = factory
            .wrap(unsafe {
                do_arith(
                    factory.nil().as_raw(),
                    operation.as_raw(),
                    arguments.as_raw(),
                    factory.nil().as_raw(),
                )
            })
            .unwrap();
        assert!(arguments.try_cdr().unwrap().is_nil());
        assert_eq!(left_reads.get(), 1);
        assert_eq!(right_reads.get(), 1);
        assert!(collections.get() >= 2);
        drop(arguments);
        crate::sexp::gengc::full_gc();
        assert_eq!(result.typeof_(), kind);
        if kind == SEXPTYPE::INTSXP {
            assert_eq!(result.try_integer_elt(0).unwrap(), 12);
        } else {
            assert_eq!(result.try_real_elt(0).unwrap(), 6.75);
        }
        assert_eq!(left.typeof_(), kind);
        assert!(altrep::is_altrep(&left));
        assert!(!altrep::is_materialized(&left));
        assert_eq!(altrep::altrep_class(&left).unwrap(), original_class);
        assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
    }
}

#[test]
fn owned_scalar_arithmetic_preserves_na_power_cases_during_result_gc() {
    let session = RSession::new_for_gc_tests();
    let factory = SessionNodeFactory::new(session.owner_token().unwrap());
    let power = factory
        .wrap(unsafe { crate::mainutils::names::R_Primitive(c"^".as_ptr()) })
        .unwrap();
    let collections = Rc::new(Cell::new(0));
    let observed = collections.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        observed.set(observed.get() + 1);
        crate::sexp::gengc::full_gc();
    }));
    for kind in [SEXPTYPE::INTSXP, SEXPTYPE::REALSXP] {
        let scalar = |value: f64| {
            let value_node = factory
                .allocate(|arena| Some(arena.alloc_vector(kind, 1)))
                .unwrap();
            let mut value_node = SexpMut::try_from_checked(value_node).unwrap();
            if kind == SEXPTYPE::INTSXP {
                let integer = if crate::sexp::ffi::is_na_real(value) {
                    NA_INTEGER
                } else {
                    value as i32
                };
                value_node.try_set_integer_elt(0, integer).unwrap();
            } else {
                value_node.try_set_real_elt(0, value).unwrap();
            }
            value_node.freeze()
        };
        for (left, right, expected) in [
            (1.0, NA_REAL, 1.0),
            (NA_REAL, 0.0, 1.0),
            (NA_REAL, 2.0, NA_REAL),
        ] {
            let mut arguments = PairlistBuilder::from_factory(factory.clone());
            arguments.push(scalar(left), None).unwrap();
            arguments.push(scalar(right), None).unwrap();
            let arguments = arguments.finish().unwrap();
            let before = crate::sexp::protect::R_ProtectCount();
            let collections_before = collections.get();
            session.with_active_in(|instance| unsafe {
                (*instance).memory_state.gc_force_gap = 1;
                (*instance).memory_state.gc_force_wait = 1;
            });
            let result = factory
                .wrap(unsafe {
                    do_arith(
                        factory.nil().as_raw(),
                        power.as_raw(),
                        arguments.as_raw(),
                        factory.nil().as_raw(),
                    )
                })
                .unwrap();
            session.with_active_in(|instance| unsafe {
                (*instance).memory_state.gc_force_gap = 0;
            });
            assert!(collections.get() > collections_before);
            drop(arguments);
            crate::sexp::gengc::full_gc();
            assert_eq!(result.typeof_(), SEXPTYPE::REALSXP);
            let actual = result.try_real_elt(0).unwrap();
            if crate::sexp::ffi::is_na_real(expected) {
                assert!(crate::sexp::ffi::is_na_real(actual));
            } else {
                assert_eq!(actual, expected);
            }
            assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
        }
    }
}

#[test]
fn owned_integer_vector_division_reads_collecting_altrep_without_output_loans() {
    struct CollectingIntegers(Rc<Cell<usize>>);
    impl AltrepClass for CollectingIntegers {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::INTSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
            Ok(4)
        }
        fn element<'s>(
            &self,
            context: &AltrepContext<'s>,
            index: i64,
        ) -> SexpResult<AltrepElement<'s>> {
            self.0.set(self.0.get() + 1);
            context.gc()?;
            Ok(AltrepElement::Integer(
                [2, 8, NA_INTEGER, 1][index as usize],
            ))
        }
    }
    let session = RSession::new_for_gc_tests();
    let factory = SessionNodeFactory::new(session.owner_token().unwrap());
    let reads = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class("arithmetic-integers", CollectingIntegers(reads.clone()))
        .unwrap();
    let source = AltrepBuilder::new(class).build().unwrap();
    let denominator = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 2)))
        .unwrap();
    let mut denominator = SexpMut::try_from_checked(denominator).unwrap();
    denominator.try_set_integer_elt(0, 2).unwrap();
    denominator.try_set_integer_elt(1, 2).unwrap();
    let mut arguments = PairlistBuilder::from_factory(factory.clone());
    arguments.push(source.clone(), None).unwrap();
    arguments.push(denominator.freeze(), None).unwrap();
    let arguments = arguments.finish().unwrap();
    let operation = factory
        .wrap(unsafe { crate::mainutils::names::R_Primitive(c"/".as_ptr()) })
        .unwrap();
    let collections = Rc::new(Cell::new(0));
    let observed = collections.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        observed.set(observed.get() + 1);
        crate::sexp::gengc::full_gc();
    }));
    let before = crate::sexp::protect::R_ProtectCount();
    let result = factory
        .wrap(unsafe {
            do_arith(
                factory.nil().as_raw(),
                operation.as_raw(),
                arguments.as_raw(),
                factory.nil().as_raw(),
            )
        })
        .unwrap();
    drop(arguments);
    crate::sexp::gengc::full_gc();
    assert_eq!(result.typeof_(), SEXPTYPE::REALSXP);
    for (index, value) in [(0, 1.0), (1, 4.0), (3, 0.5)] {
        assert_eq!(result.try_real_elt(index).unwrap(), value);
    }
    assert!(crate::sexp::ffi::is_na_real(
        result.try_real_elt(2).unwrap()
    ));
    assert_eq!(reads.get(), 4);
    assert!(collections.get() >= 4);
    assert_eq!(source.typeof_(), SEXPTYPE::INTSXP);
    assert!(!altrep::is_materialized(&source));
    assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
}

#[test]
fn owned_integer_vector_division_and_power_use_real_results_through_gc() {
    let session = RSession::new_for_gc_tests();
    let factory = SessionNodeFactory::new(session.owner_token().unwrap());
    let integer_vector = |values: &[i32]| {
        let vector = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, values.len() as _)))
            .unwrap();
        let mut vector = SexpMut::try_from_checked(vector).unwrap();
        for (index, value) in values.iter().enumerate() {
            vector.try_set_integer_elt(index as _, *value).unwrap();
        }
        vector.freeze()
    };
    let collections = Rc::new(Cell::new(0));
    let observed = collections.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        observed.set(observed.get() + 1);
        crate::sexp::gengc::full_gc();
    }));
    let run = |name: &std::ffi::CStr, left: &[i32], right: &[i32]| {
        let left = integer_vector(left);
        let right = integer_vector(right);
        let mut arguments = PairlistBuilder::from_factory(factory.clone());
        arguments.push(left, None).unwrap();
        arguments.push(right, None).unwrap();
        let arguments = arguments.finish().unwrap();
        let operation = factory
            .wrap(unsafe { crate::mainutils::names::R_Primitive(name.as_ptr()) })
            .unwrap();
        let before = crate::sexp::protect::R_ProtectCount();
        let collections_before = collections.get();
        session.with_active_in(|instance| unsafe {
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
        });
        let result = factory
            .wrap(unsafe {
                do_arith(
                    factory.nil().as_raw(),
                    operation.as_raw(),
                    arguments.as_raw(),
                    factory.nil().as_raw(),
                )
            })
            .unwrap();
        session.with_active_in(|instance| unsafe {
            (*instance).memory_state.gc_force_gap = 0;
        });
        assert!(collections.get() > collections_before);
        assert_eq!(result.typeof_(), SEXPTYPE::REALSXP);
        assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
        drop(arguments);
        crate::sexp::gengc::full_gc();
        result
    };
    let divided = run(c"/", &[2, 8, NA_INTEGER, 0, 1], &[2, 0]);
    assert_eq!(divided.len(), 5);
    assert_eq!(divided.try_real_elt(0).unwrap(), 1.0);
    assert_eq!(divided.try_real_elt(1).unwrap(), f64::INFINITY);
    assert!(crate::sexp::ffi::is_na_real(
        divided.try_real_elt(2).unwrap()
    ));
    let zero_divided = divided.try_real_elt(3).unwrap();
    assert!(zero_divided.is_nan());
    assert!(!crate::sexp::ffi::is_na_real(zero_divided));
    assert_eq!(divided.try_real_elt(4).unwrap(), 0.5);

    let power = run(
        c"^",
        &[2, 1, NA_INTEGER, NA_INTEGER, 0, NA_INTEGER],
        &[3, NA_INTEGER, 0, 2, 0, NA_INTEGER],
    );
    assert_eq!(power.len(), 6);
    for (index, value) in [(0, 8.0), (1, 1.0), (2, 1.0), (4, 1.0)] {
        assert_eq!(power.try_real_elt(index).unwrap(), value);
    }
    for index in [3, 5] {
        assert!(crate::sexp::ffi::is_na_real(
            power.try_real_elt(index).unwrap()
        ));
    }
    let recycled_power = run(c"^", &[2, 3, 4, 1, NA_INTEGER, NA_INTEGER], &[0, 2]);
    for (index, value) in [1.0, 9.0, 1.0, 1.0, 1.0].into_iter().enumerate() {
        assert_eq!(recycled_power.try_real_elt(index as _).unwrap(), value);
    }
    assert!(crate::sexp::ffi::is_na_real(
        recycled_power.try_real_elt(5).unwrap()
    ));
    for name in [c"/", c"^"] {
        let empty = run(name, &[], &[2]);
        assert_eq!(empty.len(), 0);
    }
    crate::sexp::gengc::full_gc();
    assert_eq!(power.try_real_elt(0).unwrap(), 8.0);
    assert_eq!(divided.try_real_elt(4).unwrap(), 0.5);
}

#[test]
fn owned_logical_coercion_preserves_lazy_and_materialized_providers_during_gc() {
    for materialized in [false, true] {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let reads = Rc::new(Cell::new(0));
        let source = logical_coercion_source(&session, Rc::new(Cell::new(0)), reads.clone());
        let descriptor = altrep::altrep_class(&source).unwrap();
        if materialized {
            altrep::force_materialization(&source).unwrap();
        }
        let reads_before = reads.get();
        let collections = Rc::new(Cell::new(0));
        let observed = collections.clone();
        let detach_target = Rc::new(Cell::new(std::ptr::null_mut()));
        let detached_head = detach_target.clone();
        let nil = factory.nil().as_raw();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(observed.get() + 1);
            let head: SEXP = detached_head.replace(std::ptr::null_mut());
            if !head.is_null() {
                unsafe { crate::sexp::accessors::SETCDR(head, nil) };
            }
            crate::sexp::gengc::full_gc();
        }));
        session.with_active_in(|instance| unsafe {
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
        });
        let before = crate::sexp::protect::R_ProtectCount();
        let result = coerce_logical_to_int(&factory, source.clone()).unwrap();
        assert_ne!(result.as_raw(), source.as_raw());
        assert_eq!(result.typeof_(), SEXPTYPE::INTSXP);
        assert_eq!(source.typeof_(), SEXPTYPE::LGLSXP);
        assert_eq!(altrep::altrep_class(&source).unwrap(), descriptor);
        assert_eq!(altrep::is_materialized(&source), materialized);
        assert!(!altrep::is_altrep(&result));
        assert_eq!(reads.get() - reads_before, if materialized { 0 } else { 4 });
        for (index, value) in [1, 0, NA_INTEGER, 1].into_iter().enumerate() {
            assert_eq!(result.try_integer_elt(index as _).unwrap(), value);
            assert_eq!(source.try_logical_elt(index as _).unwrap(), value);
        }
        // The actual binary arithmetic consumer must retain its first
        // converted value while converting the second collecting provider.
        let right_class = session
            .register_altrep_class(
                "logical-coercion-right",
                LogicalCoercionProvider {
                    mode: Rc::new(Cell::new(0)),
                    reads: Rc::new(Cell::new(0)),
                },
            )
            .unwrap();
        let right = AltrepBuilder::new(right_class).build().unwrap();
        let mut arguments = PairlistBuilder::from_factory(factory.clone());
        arguments.push(source.clone(), None).unwrap();
        arguments.push(right, None).unwrap();
        let arguments = arguments.finish().unwrap();
        let plus = factory
            .wrap(unsafe { crate::mainutils::names::R_Primitive(c"+".as_ptr()) })
            .unwrap();
        // The right provider has no caller root. The first result-allocation
        // callback detaches it, before its own coercion begins.
        detach_target.set(arguments.as_raw());
        let sum = factory
            .wrap(unsafe {
                do_arith(
                    factory.nil().as_raw(),
                    plus.as_raw(),
                    arguments.as_raw(),
                    factory.nil().as_raw(),
                )
            })
            .unwrap();
        assert!(arguments.try_cdr().unwrap().is_nil());
        crate::sexp::gengc::full_gc();
        for (index, value) in [2, 0, NA_INTEGER, 2].into_iter().enumerate() {
            assert_eq!(sum.try_integer_elt(index as _).unwrap(), value);
        }
        assert_eq!(result.try_integer_elt(2).unwrap(), NA_INTEGER);
        assert_eq!(source.typeof_(), SEXPTYPE::LGLSXP);
        assert_eq!(altrep::altrep_class(&source).unwrap(), descriptor);
        assert!(collections.get() > 0);
        assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
    }
}

#[test]
fn owned_logical_coercion_retypes_only_unshared_dense_storage() {
    let session = RSession::new_for_gc_tests();
    let factory = SessionNodeFactory::new(session.owner_token().unwrap());
    for shared in [false, true] {
        let value = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::LGLSXP, 2)))
            .unwrap();
        let mut value = SexpMut::try_from_checked(value).unwrap();
        value.try_set_logical_elt(0, 1).unwrap();
        value.try_set_logical_elt(1, NA_INTEGER).unwrap();
        let input = value.freeze();
        unsafe { crate::sexp::accessors::SET_NAMED(input.as_raw(), if shared { 2 } else { 0 }) };
        let original_pointer = input.as_raw();
        let output = coerce_logical_to_int(&factory, input.clone()).unwrap();
        assert_eq!(output.as_raw() == original_pointer, !shared);
        assert_eq!(output.typeof_(), SEXPTYPE::INTSXP);
        assert_eq!(output.try_integer_elt(0).unwrap(), 1);
        assert_eq!(output.try_integer_elt(1).unwrap(), NA_INTEGER);
        if shared {
            assert_eq!(input.typeof_(), SEXPTYPE::LGLSXP);
            assert_eq!(input.try_logical_elt(1).unwrap(), NA_INTEGER);
        }
    }
}

#[test]
fn owned_logical_coercion_errors_unwind_and_reentry_leave_source_retryable() {
    for mode in [1, 2, 3] {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let state = Rc::new(Cell::new(mode));
        let source = logical_coercion_source(&session, state.clone(), Rc::new(Cell::new(0)));
        let descriptor = altrep::altrep_class(&source).unwrap();
        let before = crate::sexp::protect::R_ProtectCount();
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            coerce_logical_to_int(&factory, source.clone())
        }));
        if mode == 2 {
            assert!(failure.is_err());
        } else {
            assert!(failure.unwrap().is_err());
        }
        assert_eq!(source.typeof_(), SEXPTYPE::LGLSXP);
        assert_eq!(altrep::altrep_class(&source).unwrap(), descriptor);
        assert!(!altrep::is_materialized(&source));
        assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
        state.set(0);
        crate::sexp::gengc::full_gc();
        let retry = coerce_logical_to_int(&factory, source.clone()).unwrap();
        crate::sexp::gengc::full_gc();
        for (index, value) in [1, 0, NA_INTEGER, 1].into_iter().enumerate() {
            assert_eq!(retry.try_integer_elt(index as _).unwrap(), value);
            assert_eq!(source.try_logical_elt(index as _).unwrap(), value);
        }
        assert_eq!(source.typeof_(), SEXPTYPE::LGLSXP);
        assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
    }
}

unsafe fn int_scalar(value: c_int) -> SEXP {
    unsafe {
        let scalar = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
        *INTEGER(scalar) = value;
        scalar
    }
}

unsafe fn two_arg_call_args(left: SEXP, right: SEXP) -> SEXP {
    unsafe {
        crate::sexp::constructors::Rf_cons(
            left,
            crate::sexp::constructors::Rf_cons(right, crate::sexp::globals::R_NilValue()),
        )
    }
}

#[test]
fn test_do_arith_uses_primitive_operation_code() {
    let _session = crate::sexp::session::RSession::new_for_gc_tests();
    unsafe {
        let plus = crate::mainutils::names::R_Primitive(c"+".as_ptr());
        let plus_args = two_arg_call_args(int_scalar(5), int_scalar(3));
        let plus_result = do_arith(
            std::ptr::null_mut(),
            plus,
            plus_args,
            crate::sexp::globals::R_NilValue(),
        );
        assert_eq!(TYPEOF(plus_result), SEXPTYPE::INTSXP);
        assert_eq!(*INTEGER(plus_result), 8);

        let minus = crate::mainutils::names::R_Primitive(c"-".as_ptr());
        let minus_args = two_arg_call_args(int_scalar(5), int_scalar(3));
        let minus_result = do_arith(
            std::ptr::null_mut(),
            minus,
            minus_args,
            crate::sexp::globals::R_NilValue(),
        );
        assert_eq!(TYPEOF(minus_result), SEXPTYPE::INTSXP);
        assert_eq!(*INTEGER(minus_result), 2);
    }
}

#[test]
fn test_R_integer_plus() {
    let mut naflag = false;
    assert_eq!(R_integer_plus(3, 4, &mut naflag), 7);
    assert!(!naflag);

    // NA propagation
    assert_eq!(R_integer_plus(NA_INTEGER, 4, &mut naflag), NA_INTEGER);
    assert_eq!(R_integer_plus(3, NA_INTEGER, &mut naflag), NA_INTEGER);

    // Overflow
    assert_eq!(R_integer_plus(c_int::MAX, 1, &mut naflag), NA_INTEGER);
    assert!(naflag);
}

#[test]
fn test_R_integer_minus() {
    let mut naflag = false;
    assert_eq!(R_integer_minus(10, 3, &mut naflag), 7);
    assert!(!naflag);

    // Overflow: (MIN+2) - 3 = MIN-1, which overflows
    assert_eq!(R_integer_minus(c_int::MIN + 2, 3, &mut naflag), NA_INTEGER);
    assert!(naflag);
}

#[test]
fn test_R_integer_times() {
    let mut naflag = false;
    assert_eq!(R_integer_times(6, 7, &mut naflag), 42);
    assert!(!naflag);

    // NA propagation
    assert_eq!(R_integer_times(NA_INTEGER, 7, &mut naflag), NA_INTEGER);

    // Overflow
    naflag = false;
    assert_eq!(R_integer_times(c_int::MAX, 2, &mut naflag), NA_INTEGER);
    assert!(naflag);
}

#[test]
fn test_R_integer_divide() {
    assert!((R_integer_divide(10, 3) - 10.0 / 3.0).abs() < 1e-10);
    assert!(R_integer_divide(NA_INTEGER, 3).is_nan());
    assert!(R_integer_divide(3, NA_INTEGER).is_nan());
}

#[test]
fn test_myfmod_basic() {
    assert!((myfmod(10.0, 3.0) - 1.0).abs() < 1e-10);
    // R's %% takes the sign of the divisor (floor-based), unlike C fmod.
    assert!((myfmod(-10.0, 3.0) - 2.0).abs() < 1e-10);
    assert!(myfmod(10.0, 0.0).is_nan());
}

#[test]
fn test_myfmod_exact() {
    // When x1 == x2 in magnitude, result should be 0
    assert_eq!(myfmod(5.0, 5.0), 0.0);
    assert_eq!(myfmod(-5.0, 5.0), 0.0);
}

#[test]
fn test_myfloor_basic() {
    // 10 / 3 = 3.33.. => floor = 3
    assert!((myfloor(10.0, 3.0) - 3.0).abs() < 1e-10);
    // -10 / 3 = -3.33.. => floor = -4
    assert!((myfloor(-10.0, 3.0) - (-4.0)).abs() < 1e-10);
}

#[test]
fn test_Rsqrt() {
    assert_eq!(Rsqrt(4.0), 2.0);
    assert_eq!(Rsqrt(9.0), 3.0);
    assert_eq!(Rsqrt(0.0), 0.0);
    assert!((Rsqrt(2.0) - 2.0_f64.sqrt()).abs() < 1e-15);
}

#[test]
fn test_Rsqrt_negative_zero() {
    let neg_zero = -0.0_f64;
    assert_eq!(Rsqrt(neg_zero).is_sign_negative(), true);
}

#[test]
fn test_Rexp_small() {
    // For very small x, should return 1 + x
    let x = f64::EPSILON * 0.1;
    assert!((Rexp(x) - (1.0 + x)).abs() < 1e-20);
}

#[test]
fn test_Rexp_normal() {
    assert!((Rexp(1.0) - 1.0_f64.exp()).abs() < 1e-15);
}

#[test]
fn test_Rexpm1_small() {
    let x = f64::EPSILON * 0.1;
    assert!((Rexpm1(x) - x).abs() < 1e-20);
}

#[test]
fn test_Rlog1p_small() {
    let x = f64::EPSILON * 0.1;
    assert!((Rlog1p(x) - x).abs() < 1e-20);
}

#[test]
fn test_Rsin_small() {
    let x = 1e-10_f64;
    assert!((Rsin(x) - x).abs() < 1e-20);
}

#[test]
fn test_Rcos_small() {
    let x = 1e-8_f64;
    let expected = 1.0 - x * x * 0.5;
    assert!((Rcos(x) - expected).abs() < 1e-20);
}

#[test]
fn test_Rtan_small() {
    let x = 1e-10_f64;
    assert!((Rtan(x) - x).abs() < 1e-20);
}

#[test]
fn test_Rasin_small() {
    let x = 1e-10_f64;
    assert!((Rasin(x) - x).abs() < 1e-20);
}

#[test]
fn test_Ratan_small() {
    let x = 1e-10_f64;
    assert!((Ratan(x) - x).abs() < 1e-20);
}

#[test]
fn test_R_IsNA() {
    let na = f64::from_bits(crate::sexp::ffi::R_NA_BIT_PATTERN);
    assert_eq!(R_IsNA(na), 1);
    assert_eq!(R_IsNA(f64::NAN), 0);
    assert_eq!(R_IsNA(1.0), 0);
}

#[test]
fn test_R_IsNaN() {
    assert_eq!(R_IsNaN(f64::NAN), 1);
    let na = f64::from_bits(crate::sexp::ffi::R_NA_BIT_PATTERN);
    assert_eq!(R_IsNaN(na), 0);
    assert_eq!(R_IsNaN(1.0), 0);
}
