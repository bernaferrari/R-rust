//! Linear and Step Function Interpolation
//! Port of r-source/src/library/stats/src/approx.c

use crate::main::coerce::R_FINITE;
use crate::main::coerce::{asInteger, asLogical, asReal, coerceVector};
use crate::sexp::accessors::{REAL, TYPEOF, XLENGTH};
use crate::sexp::constructors::Rf_allocVector3;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect as protect_sexp;
use crate::sexp::{Sexp, SexpMut, owner::OwnerToken};
use std::os::raw::{c_double, c_int};

struct appr_meth {
    ylow: c_double,
    yhigh: c_double,
    f1: c_double,
    f2: c_double,
    kind: c_int,
}
fn active(owner: OwnerToken<'_>) {
    owner
        .require_active()
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
}
fn require_real(value: Sexp<'_>) -> Sexp<'_> {
    if value.typeof_() != SEXPTYPE::REALSXP {
        // This is a static type-name table entry, not a numerical payload.
        let name = unsafe {
            std::ffi::CStr::from_ptr(crate::mainutils::util_main::type2char(
                value.typeof_().as_c_int(),
            ))
        };
        crate::mainutils::essentials::base_error(format!(
            "REAL() can only be applied to a 'numeric', not a '{}'",
            name.to_string_lossy()
        ));
    }
    value
}
fn real_input<'s>(owner: OwnerToken<'s>, raw: SEXP) -> Sexp<'s> {
    active(owner);
    require_real(
        owner
            .sexp(raw)
            .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string())),
    )
}
fn x_input<'s>(owner: OwnerToken<'s>, raw: SEXP) -> Sexp<'s> {
    active(owner);
    let value = owner
        .sexp(raw)
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    if value.is_nil() {
        crate::mainutils::essentials::base_error("LENGTH or similar applied to NULL object");
    }
    require_real(value)
}

fn paired(x: &Sexp<'_>, y: &Sexp<'_>) {
    if x.len() != y.len() {
        crate::mainutils::essentials::base_error("'x' and 'y' lengths differ");
    }
}
fn prepare_real(owner: OwnerToken<'_>, value: &Sexp<'_>) {
    active(owner);
    crate::sexp::altrep::force_materialization(value)
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    active(owner);
}

fn read_real(owner: OwnerToken<'_>, value: &Sexp<'_>, index: R_xlen_t) -> c_double {
    active(owner);
    let result = value
        .try_real_elt(index)
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    active(owner);
    result
}
fn real_parameter(owner: OwnerToken<'_>, raw: SEXP) -> c_double {
    active(owner);
    let value = unsafe { asReal(raw) };
    active(owner);
    value
}
fn integer_parameter(owner: OwnerToken<'_>, raw: SEXP) -> c_int {
    active(owner);
    let value = unsafe { asInteger(raw) };
    active(owner);
    value
}
fn logical_parameter(owner: OwnerToken<'_>, raw: SEXP) -> c_int {
    active(owner);
    let value = unsafe { asLogical(raw) };
    active(owner);
    value
}
fn approx1(
    v: c_double,
    x: &Sexp<'_>,
    y: &Sexp<'_>,
    meth: &appr_meth,
    owner: OwnerToken<'_>,
) -> c_double {
    if x.is_empty() {
        return f64::NAN;
    }
    let mut i = 0;
    let mut j = x.len() - 1;
    if v < read_real(owner, x, i) {
        return meth.ylow;
    }
    if v > read_real(owner, x, j) {
        return meth.yhigh;
    }
    while j > i + 1 {
        let ij = i + (j - i) / 2;
        if v < read_real(owner, x, ij) {
            j = ij;
        } else {
            i = ij;
        }
    }
    let xj = read_real(owner, x, j);
    if v == xj {
        return read_real(owner, y, j);
    }
    let xi = read_real(owner, x, i);
    if v == xi {
        return read_real(owner, y, i);
    }
    if meth.kind == 1 {
        let yi = read_real(owner, y, i);
        let yj = read_real(owner, y, j);
        yi + (yj - yi) * ((v - xi) / (xj - xi))
    } else {
        (if meth.f1 != 0.0 {
            read_real(owner, y, i) * meth.f1
        } else {
            0.0
        }) + (if meth.f2 != 0.0 {
            read_real(owner, y, j) * meth.f2
        } else {
            0.0
        })
    }
}
fn R_approxtest(
    x: &Sexp<'_>,
    y: &Sexp<'_>,
    method: c_int,
    f: c_double,
    na_rm: c_int,
    owner: OwnerToken<'_>,
) -> Result<(), &'static std::ffi::CStr> {
    match method {
        1 => {}
        2 => {
            if !R_FINITE(f) || !(0.0..=1.0).contains(&f) {
                return Err(c"approx(): invalid f value");
            }
        }
        _ => return Err(c"approx(): invalid interpolation method"),
    }
    for i in 0..x.len() {
        let xv = read_real(owner, x, i);
        if na_rm != 0 {
            if xv.is_nan() || read_real(owner, y, i).is_nan() {
                return Err(c"approx(): attempted to interpolate NA values");
            }
        } else if xv.is_nan() {
            return Err(c"approx(x,y, .., na.rm=FALSE): NA values in x are not allowed");
        }
    }
    Ok(())
}
pub unsafe fn ApproxTest(x: SEXP, y: SEXP, method: SEXP, f: SEXP, na_rm: SEXP) -> SEXP {
    let owner = unsafe { OwnerToken::current() }
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    let x = x_input(owner, x);
    let y = real_input(owner, y);
    paired(&x, &y);
    let method = integer_parameter(owner, method);
    let f = real_parameter(owner, f);
    let na_rm = logical_parameter(owner, na_rm);
    // Preserve native REAL's eager materialization without retaining pointers
    // from earlier inputs across later providers or reentrant collection.
    prepare_real(owner, &x);
    prepare_real(owner, &y);
    paired(&x, &y);
    if let Err(message) = R_approxtest(&x, &y, method, f, na_rm, owner) {
        unsafe { crate::main::errors::Rf_error(message.as_ptr()) };
    }
    active(owner);
    unsafe { R_NilValue() }
}
pub unsafe fn Approx(
    x: SEXP,
    y: SEXP,
    v: SEXP,
    method: SEXP,
    yleft: SEXP,
    yright: SEXP,
    f: SEXP,
    na_rm: SEXP,
) -> SEXP {
    let owner = unsafe { OwnerToken::current() }
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    let x = x_input(owner, x);
    let y = real_input(owner, y);
    paired(&x, &y);
    let xout = unsafe { coerceVector(v, SEXPTYPE::REALSXP.as_c_int()) };
    active(owner);
    let _xout_guard = unsafe { protect_sexp(xout) };
    let xout = real_input(owner, xout);
    let yout = unsafe { Rf_allocVector3(SEXPTYPE::REALSXP, xout.len()) };
    let _yout_guard = unsafe { protect_sexp(yout) };
    let mut output = SexpMut::try_from_checked(real_input(owner, yout))
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    let kind = integer_parameter(owner, method);
    let ylow = real_parameter(owner, yleft);
    let yhigh = real_parameter(owner, yright);
    let f = real_parameter(owner, f);
    let _na_rm = logical_parameter(owner, na_rm);
    prepare_real(owner, &x);
    prepare_real(owner, &y);
    prepare_real(owner, &xout);
    paired(&x, &y);
    if xout.len() != output.len() {
        crate::mainutils::essentials::base_error(
            "approx(): query length changed during evaluation",
        );
    }
    let meth = appr_meth {
        ylow,
        yhigh,
        f1: 1.0 - f,
        f2: f,
        kind,
    };
    // Checked handles and copied elements cross callbacks, never exported payload pointers.
    for i in 0..xout.len() {
        let v = read_real(owner, &xout, i);
        let value = if v.is_nan() {
            v
        } else {
            approx1(v, &x, &y, &meth, owner)
        };
        active(owner);
        output
            .try_set_real_elt(i, value)
            .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    }
    active(owner);
    yout
}

/// GNU `approx(x, y, xout)`.
pub unsafe fn do_approx(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, INTEGER, SET_VECTOR_ELT};
        use crate::sexp::constructors::{Rf_ScalarInteger, Rf_ScalarReal, Rf_allocVector3};
        use crate::sexp::protect::protect;
        let x = CAR(args);
        let y = CAR(CDR(args));
        let xout = CAR(CDR(CDR(args)));
        let xc = coerceVector(x, SEXPTYPE::REALSXP.as_c_int());
        let _xc = protect(xc);
        let yc = coerceVector(y, SEXPTYPE::REALSXP.as_c_int());
        let _yc = protect(yc);
        let n = XLENGTH(xc);
        let yleft = Rf_ScalarReal(*REAL(yc));
        let _yl = protect(yleft);
        let yright = Rf_ScalarReal(*REAL(yc).add((n - 1) as usize));
        let _yr = protect(yright);
        let method = Rf_ScalarInteger(1);
        let _m = protect(method);
        let f = Rf_ScalarReal(0.0);
        let _f = protect(f);
        let na_rm = Rf_ScalarInteger(1);
        let _na = protect(na_rm);
        let yout = Approx(xc, yc, xout, method, yleft, yright, f, na_rm);
        let _yo = protect(yout);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, xout);
        SET_VECTOR_ELT(result, 1, yout);
        crate::mainutils::essentials::set_string_names(result, &["x".to_string(), "y".to_string()]);
        let _ = INTEGER;
        result
    }
}

/// GNU `approxfun(x, y)` linear interpolator.
pub unsafe fn do_approxfun(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, SETTAG};
        use crate::sexp::constructors::{Rf_cons, Rf_lang2};
        use crate::sexp::protect::protect;
        use crate::sexp::symbol::Rf_install;
        let x = coerceVector(CAR(args), SEXPTYPE::REALSXP.as_c_int());
        let _x = protect(x);
        let y = coerceVector(CAR(CDR(args)), SEXPTYPE::REALSXP.as_c_int());
        let _y = protect(y);
        let env = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), rho, R_NilValue());
        let _env = protect(env);
        crate::sexp::envir::defineVar(Rf_install(c"x".as_ptr()), x, env);
        crate::sexp::envir::defineVar(Rf_install(c"y".as_ptr()), y, env);
        let vsym = Rf_install(c"v".as_ptr());
        let formals = Rf_cons(crate::sexp::globals::R_MissingArg(), R_NilValue());
        SETTAG(formals, vsym);
        let body = Rf_lang2(Rf_install(c".approx_apply".as_ptr()), vsym);
        crate::mainutils::dstruct::mkCLOSXP(formals, body, env)
    }
}

/// Evaluate an `approxfun` closure at `v`.
pub unsafe fn do_approx_apply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, ENCLOS};
        use crate::sexp::constructors::{Rf_ScalarInteger, Rf_ScalarReal};
        use crate::sexp::protect::protect;
        use crate::sexp::symbol::Rf_install;
        let v = CAR(args);
        let env = ENCLOS(rho);
        let x = crate::sexp::envir::R_findVar(Rf_install(c"x".as_ptr()), env);
        let y = crate::sexp::envir::R_findVar(Rf_install(c"y".as_ptr()), env);
        let n = XLENGTH(x);
        let yleft = Rf_ScalarReal(*REAL(y));
        let _yl = protect(yleft);
        let yright = Rf_ScalarReal(*REAL(y).add((n - 1) as usize));
        let _yr = protect(yright);
        let method = Rf_ScalarInteger(1);
        let _m = protect(method);
        let f = Rf_ScalarReal(0.0);
        let _f = protect(f);
        let na_rm = Rf_ScalarInteger(1);
        let _na = protect(na_rm);
        Approx(x, y, v, method, yleft, yright, f, na_rm)
    }
}

#[cfg(test)]
mod ownership_tests {
    use super::*;
    use crate::sexp::{RSession, Sexp, SexpMut};

    fn reals(session: &RSession, values: &[f64]) -> Sexp<'static> {
        let value = session
            .owner_token()
            .unwrap()
            .node_factory()
            .allocate(|arena| {
                arena
                    .alloc_vector_sexp(SEXPTYPE::REALSXP, values.len() as i64)
                    .map(|value| value.as_raw())
            })
            .unwrap()
            .into_owned()
            .unwrap();
        let mut value = SexpMut::try_from_checked(value).unwrap();
        for (index, &element) in values.iter().enumerate() {
            value.try_set_real_elt(index as i64, element).unwrap();
        }
        value.freeze()
    }

    #[test]
    fn native_approx_validation_error_unwinds_and_empty_validation_recovers() {
        let session = RSession::new_for_gc_tests();
        let x = reals(&session, &[1.0, 2.0]);
        let y = reals(&session, &[10.0, 20.0]);
        let method = reals(&session, &[2.0]);
        let f = reals(&session, &[-1.0]);
        let na_rm = reals(&session, &[1.0]);
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session.with_active(|| unsafe {
                ApproxTest(
                    x.as_raw(),
                    y.as_raw(),
                    method.as_raw(),
                    f.as_raw(),
                    na_rm.as_raw(),
                )
            });
        }))
        .expect_err("invalid constant interpolation must raise an R error");
        assert!(
            failure
                .downcast_ref::<crate::sexp::context::RError>()
                .is_some_and(|error| error.message == "approx(): invalid f value")
        );
        session.owner_token().unwrap().full_gc().unwrap();
        let empty = reals(&session, &[]);
        let method = reals(&session, &[1.0]);
        session.with_active(|| unsafe {
            assert_eq!(
                ApproxTest(
                    empty.as_raw(),
                    empty.as_raw(),
                    method.as_raw(),
                    f.as_raw(),
                    na_rm.as_raw()
                ),
                R_NilValue()
            );
        });
    }

    #[test]
    fn native_approx_outputs_survive_torture_and_input_release() {
        let mut session = RSession::new_for_gc_tests();
        session
            .eval_code_with_output_capture("gctorture(TRUE)")
            .0
            .unwrap();
        let x = reals(&session, &[1.0, 2.0]);
        let y = reals(&session, &[10.0, 20.0]);
        let method = reals(&session, &[1.0]);
        let f = reals(&session, &[0.0]);
        let na_rm = reals(&session, &[1.0]);
        let bound = reals(&session, &[crate::sexp::ffi::NA_REAL]);
        let v = reals(&session, &[1.5]);
        let empty = reals(&session, &[]);
        let call = |v: &Sexp<'_>| {
            session.with_active(|| unsafe {
                let raw = Approx(
                    x.as_raw(),
                    y.as_raw(),
                    v.as_raw(),
                    method.as_raw(),
                    bound.as_raw(),
                    bound.as_raw(),
                    f.as_raw(),
                    na_rm.as_raw(),
                );
                session
                    .owner_token()
                    .unwrap()
                    .sexp(raw)
                    .unwrap()
                    .into_owned()
                    .unwrap()
            })
        };
        let output = call(&v);
        let empty_output = call(&empty);
        drop((x, y, method, f, na_rm, bound, v, empty));
        session.owner_token().unwrap().full_gc().unwrap();
        assert_eq!(output.try_real_elt(0), Ok(15.0));
        assert_eq!(empty_output.len(), 0);
        session
            .eval_code_with_output_capture("gctorture(FALSE)")
            .0
            .unwrap();
    }
    use crate::sexp::altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement};
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    struct PlainReals;
    impl AltrepClass for PlainReals {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::REALSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> crate::sexp::SexpResult<i64> {
            Ok(2)
        }
        fn element<'s>(
            &self,
            _: &AltrepContext<'s>,
            i: i64,
        ) -> crate::sexp::SexpResult<AltrepElement<'s>> {
            Ok(AltrepElement::Real([1.0, 2.0][i as usize]))
        }
    }
    struct GcReals {
        values: [f64; 2],
        calls: Rc<Cell<usize>>,
        reenter: Rc<Cell<bool>>,
    }
    impl AltrepClass for GcReals {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::REALSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> crate::sexp::SexpResult<i64> {
            Ok(2)
        }
        fn element<'s>(
            &self,
            context: &AltrepContext<'s>,
            i: i64,
        ) -> crate::sexp::SexpResult<AltrepElement<'s>> {
            self.calls.set(self.calls.get() + 1);
            context.gc()?;
            if self.reenter.replace(false) {
                let previous = context.data1()?;
                assert!(crate::sexp::altrep::is_materialized(&previous));
                crate::sexp::altrep::force_materialization(&previous)?;
                let auxiliary_y = context.data2()?;
                let mut one =
                    SexpMut::try_from_checked(context.alloc_vector(SEXPTYPE::REALSXP, 1)?)?;
                one.try_set_real_elt(0, 1.0)?;
                let one = one.freeze();
                let mut zero =
                    SexpMut::try_from_checked(context.alloc_vector(SEXPTYPE::REALSXP, 1)?)?;
                zero.try_set_real_elt(0, 0.0)?;
                let zero = zero.freeze();
                let routine = crate::library::stats::random::lookup_call("ApproxTest").unwrap();
                unsafe {
                    routine
                        .invoke_call(&[
                            previous.as_raw(),
                            auxiliary_y.as_raw(),
                            one.as_raw(),
                            zero.as_raw(),
                            one.as_raw(),
                        ])
                        .unwrap();
                }
            }
            Ok(AltrepElement::Real(self.values[i as usize]))
        }
    }
    #[test]
    fn registered_approx_altrep_gc_materialization_and_reentry_are_checked() {
        let session = RSession::new_for_gc_tests();
        let x_class = session
            .register_altrep_class("approx.checked.x", PlainReals)
            .unwrap();
        let x = AltrepBuilder::new(x_class)
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        let auxiliary_y = reals(&session, &[10.0, 20.0]);
        let calls = Rc::new(Cell::new(0));
        let reenter = Rc::new(Cell::new(true));
        let y_class = session
            .register_altrep_class(
                "approx.checked.y",
                GcReals {
                    values: [10.0, 20.0],
                    calls: calls.clone(),
                    reenter: reenter.clone(),
                },
            )
            .unwrap();
        let y = AltrepBuilder::new(y_class)
            .data1(x.clone())
            .data2(auxiliary_y)
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        let v_class = session
            .register_altrep_class(
                "approx.checked.v",
                GcReals {
                    values: [1.25, 1.75],
                    calls: calls.clone(),
                    reenter: Rc::new(Cell::new(false)),
                },
            )
            .unwrap();
        let v = AltrepBuilder::new(v_class)
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        let one = reals(&session, &[1.0]);
        let zero = reals(&session, &[0.0]);
        let bound = reals(&session, &[crate::sexp::ffi::NA_REAL]);
        let result = session.with_active(|| unsafe {
            let raw = crate::library::stats::random::lookup_call("Approx")
                .unwrap()
                .invoke_call(&[
                    x.as_raw(),
                    y.as_raw(),
                    v.as_raw(),
                    one.as_raw(),
                    bound.as_raw(),
                    bound.as_raw(),
                    zero.as_raw(),
                    one.as_raw(),
                ])
                .unwrap();
            session
                .owner_token()
                .unwrap()
                .sexp(raw)
                .unwrap()
                .into_owned()
                .unwrap()
        });
        assert_eq!(result.try_real_elt(0), Ok(12.5));
        assert_eq!(result.try_real_elt(1), Ok(17.5));
        assert!(calls.get() > 0);
        assert!(!reenter.get());
        assert!(crate::sexp::altrep::is_materialized(&x));
        assert!(crate::sexp::altrep::is_materialized(&y));
        assert!(crate::sexp::altrep::is_materialized(&v));
        session.with_active(|| unsafe {
            crate::library::stats::random::lookup_call("ApproxTest")
                .unwrap()
                .invoke_call(&[
                    x.as_raw(),
                    y.as_raw(),
                    one.as_raw(),
                    zero.as_raw(),
                    one.as_raw(),
                ])
                .unwrap();
        });
        drop((x, y, v, one, zero, bound));
        session.owner_token().unwrap().full_gc().unwrap();
        assert_eq!(result.try_real_elt(1), Ok(17.5));
    }

    struct RevokingReals(Rc<RefCell<Option<RSession>>>);
    impl AltrepClass for RevokingReals {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::REALSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> crate::sexp::SexpResult<i64> {
            Ok(2)
        }
        fn element<'s>(
            &self,
            _: &AltrepContext<'s>,
            _: i64,
        ) -> crate::sexp::SexpResult<AltrepElement<'s>> {
            self.0.borrow_mut().as_mut().unwrap().close();
            Ok(AltrepElement::Real(10.0))
        }
    }
    #[test]
    fn registered_approx_and_validator_reject_provider_revocation() {
        for name in ["Approx", "ApproxTest"] {
            let session = RSession::new_for_gc_tests();
            let weak = session.owner_token().unwrap().weak_owner().unwrap();
            let facade = Rc::new(RefCell::new(None::<RSession>));
            let class = session
                .register_altrep_class("approx.closed", RevokingReals(facade.clone()))
                .unwrap()
                .into_owned()
                .unwrap();
            let x = reals(&session, &[1.0, 2.0]);
            let y = AltrepBuilder::new(class)
                .build()
                .unwrap()
                .into_owned()
                .unwrap();
            let v = reals(&session, &[1.5]);
            let one = reals(&session, &[1.0]);
            let zero = reals(&session, &[0.0]);
            *facade.borrow_mut() = Some(session);
            let published = Cell::new(false);
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::sexp::owner::with_runtime(&weak, |access| {
                    access.with_native(|owner| {
                        let routine = crate::library::stats::random::lookup_call(name).unwrap();
                        let raw = unsafe {
                            if name == "Approx" {
                                routine
                                    .invoke_call(&[
                                        x.as_raw(),
                                        y.as_raw(),
                                        v.as_raw(),
                                        one.as_raw(),
                                        zero.as_raw(),
                                        zero.as_raw(),
                                        zero.as_raw(),
                                        one.as_raw(),
                                    ])
                                    .unwrap()
                            } else {
                                routine
                                    .invoke_call(&[
                                        x.as_raw(),
                                        y.as_raw(),
                                        one.as_raw(),
                                        zero.as_raw(),
                                        one.as_raw(),
                                    ])
                                    .unwrap()
                            }
                        };
                        published.set(true);
                        owner.sexp(raw)?.into_owned()
                    })
                })
            }));
            assert!(
                !published.get(),
                "revoked provider returned a native result"
            );
            match outcome {
                Err(_) => {}
                Ok(result) => assert!(result.is_err()),
            }
            assert!(y.try_real_elt(0).is_err());
            drop(facade.borrow_mut().take());
        }
    }
}
