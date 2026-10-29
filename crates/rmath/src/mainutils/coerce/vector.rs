use super::*;

// ---------------------------------------------------------------------------
// Vector coercion functions
// ---------------------------------------------------------------------------

/// Coerce a vector to logical type.
pub unsafe fn coerceToLogical(v: SEXP) -> SEXP {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let pin = owner
            .pin()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let factory = owner.node_factory();
        let source = factory
            .wrap(v)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let check = || {
            if let Some(pin) = &pin {
                pin.require_live()
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
            factory
                .require_active()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        };
        let n = source.len();
        check();
        // Keep the runtime's typed allocation-denial error and GC retry.
        let raw_output = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let output = factory
            .wrap(raw_output)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        check();
        SHALLOW_DUPLICATE_ATTRIB(output.as_raw(), source.as_raw());
        check();
        let mut output = crate::sexp::object::SexpMut::try_from_checked(output)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let mut warn: c_int = 0;
        for i in 0..n {
            // Copy the provider value before accessing the output. Neither a
            // payload projection nor an arena borrow spans a provider callback.
            let value = match source.typeof_() {
                SEXPTYPE::INTSXP => LogicalFromInteger(
                    source
                        .try_integer_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
                    &mut warn,
                ),
                SEXPTYPE::REALSXP => LogicalFromReal(
                    source
                        .try_real_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
                    &mut warn,
                ),
                SEXPTYPE::CPLXSXP => LogicalFromComplex(
                    source
                        .try_complex_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
                    &mut warn,
                ),
                SEXPTYPE::STRSXP => {
                    let text = source
                        .try_string_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                    check();
                    LogicalFromString(text.as_raw(), &mut warn)
                }
                SEXPTYPE::RAWSXP => LogicalFromInteger(
                    source
                        .try_raw_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                        as c_int,
                    &mut warn,
                ),
                _ => NA_LOGICAL,
            };
            check();
            output
                .try_set_logical_elt(i, value)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        if warn != 0 {
            CoercionWarning(warn);
            check();
        }
        output.freeze().as_raw()
    }
}

/// Coerce a vector to integer type.
pub unsafe fn coerceToInteger(v: SEXP) -> SEXP {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let pin = owner
            .pin()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let factory = owner.node_factory();
        let source = factory
            .wrap(v)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let check = || {
            if let Some(pin) = &pin {
                pin.require_live()
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
            factory
                .require_active()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        };
        let n = source.len();
        check();
        // Keep the runtime's typed allocation-denial error and GC retry.
        let raw_output = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        let output = factory
            .wrap(raw_output)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        check();
        SHALLOW_DUPLICATE_ATTRIB(output.as_raw(), source.as_raw());
        check();
        let mut output = crate::sexp::object::SexpMut::try_from_checked(output)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let mut warn: c_int = 0;
        for i in 0..n {
            // Copy the provider value before accessing the output. Neither a
            // payload projection nor an arena borrow spans a provider callback.
            let value = match source.typeof_() {
                SEXPTYPE::LGLSXP => IntegerFromLogical(
                    source
                        .try_logical_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
                    &mut warn,
                ),
                SEXPTYPE::REALSXP => IntegerFromReal(
                    source
                        .try_real_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
                    &mut warn,
                ),
                SEXPTYPE::CPLXSXP => IntegerFromComplex(
                    source
                        .try_complex_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
                    &mut warn,
                ),
                SEXPTYPE::STRSXP => {
                    let text = source
                        .try_string_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                    check();
                    IntegerFromString(text.as_raw(), &mut warn)
                }
                SEXPTYPE::RAWSXP => source
                    .try_raw_elt(i)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                    as c_int,
                _ => NA_INTEGER,
            };
            check();
            output
                .try_set_integer_elt(i, value)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        if warn != 0 {
            CoercionWarning(warn);
            check();
        }
        output.freeze().as_raw()
    }
}

/// Coerce a vector to real (double) type.
pub unsafe fn coerceToReal(v: SEXP) -> SEXP {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let pin = owner
            .pin()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let factory = owner.node_factory();
        let source = factory
            .wrap(v)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let check = || {
            if let Some(pin) = &pin {
                pin.require_live()
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
            factory
                .require_active()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        };
        let n = source.len();
        check();
        // Keep the runtime's typed allocation-denial error and GC retry.
        let raw_output = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let output = factory
            .wrap(raw_output)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        check();
        SHALLOW_DUPLICATE_ATTRIB(output.as_raw(), source.as_raw());
        check();
        let mut output = crate::sexp::object::SexpMut::try_from_checked(output)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let mut warn: c_int = 0;
        for i in 0..n {
            // Copy the provider value before accessing the output. Neither a
            // payload projection nor an arena borrow spans a provider callback.
            let value = match source.typeof_() {
                SEXPTYPE::LGLSXP => RealFromLogical(
                    source
                        .try_logical_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
                    &mut warn,
                ),
                SEXPTYPE::INTSXP => RealFromInteger(
                    source
                        .try_integer_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
                    &mut warn,
                ),
                SEXPTYPE::CPLXSXP => RealFromComplex(
                    source
                        .try_complex_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
                    &mut warn,
                ),
                SEXPTYPE::STRSXP => {
                    let text = source
                        .try_string_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                    check();
                    RealFromString(text.as_raw(), &mut warn)
                }
                SEXPTYPE::RAWSXP => RealFromInteger(
                    source
                        .try_raw_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                        as c_int,
                    &mut warn,
                ),
                _ => NA_REAL,
            };
            check();
            output
                .try_set_real_elt(i, value)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        if warn != 0 {
            CoercionWarning(warn);
            check();
        }
        output.freeze().as_raw()
    }
}

/// Coerce a vector to complex type.
pub unsafe fn coerceToComplex(v: SEXP) -> SEXP {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let pin = owner
            .pin()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let factory = owner.node_factory();
        let source = factory
            .wrap(v)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let check = || {
            if let Some(pin) = &pin {
                pin.require_live()
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
            factory
                .require_active()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        };
        let n = source.len();
        check();
        // Keep the runtime's typed allocation-denial error and GC retry.
        let raw_output = Rf_allocVector3(SEXPTYPE::CPLXSXP, n);
        let output = factory
            .wrap(raw_output)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        check();
        SHALLOW_DUPLICATE_ATTRIB(output.as_raw(), source.as_raw());
        check();
        let mut output = crate::sexp::object::SexpMut::try_from_checked(output)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let mut warn: c_int = 0;
        for i in 0..n {
            // Copy the provider value before accessing the output. Neither a
            // payload projection nor an arena borrow spans a provider callback.
            let value = match source.typeof_() {
                SEXPTYPE::LGLSXP => ComplexFromLogical(
                    source
                        .try_logical_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
                    &mut warn,
                ),
                SEXPTYPE::INTSXP => ComplexFromInteger(
                    source
                        .try_integer_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
                    &mut warn,
                ),
                SEXPTYPE::REALSXP => ComplexFromReal(
                    source
                        .try_real_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
                    &mut warn,
                ),
                SEXPTYPE::STRSXP => {
                    let text = source
                        .try_string_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                    check();
                    ComplexFromString(text.as_raw(), &mut warn)
                }
                SEXPTYPE::RAWSXP => ComplexFromInteger(
                    source
                        .try_raw_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                        as c_int,
                    &mut warn,
                ),
                _ => Rcomplex {
                    r: NA_REAL,
                    i: NA_REAL,
                },
            };
            check();
            output
                .try_set_complex_elt(i, value)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        if warn != 0 {
            CoercionWarning(warn);
            check();
        }
        output.freeze().as_raw()
    }
}

/// Coerce a vector to raw type.
pub unsafe fn coerceToRaw(v: SEXP) -> SEXP {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let pin = owner
            .pin()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let factory = owner.node_factory();
        let source = factory
            .wrap(v)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let check = || {
            if let Some(pin) = &pin {
                pin.require_live()
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
            factory
                .require_active()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        };
        let n = source.len();
        check();
        // Keep the runtime's typed allocation-denial error and GC retry.
        let raw_output = Rf_allocVector3(SEXPTYPE::RAWSXP, n);
        let output = factory
            .wrap(raw_output)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        check();
        SHALLOW_DUPLICATE_ATTRIB(output.as_raw(), source.as_raw());
        check();
        let mut output = crate::sexp::object::SexpMut::try_from_checked(output)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let mut warn: c_int = 0;
        for i in 0..n {
            let tmp: c_int = match source.typeof_() {
                SEXPTYPE::LGLSXP => {
                    let val = IntegerFromLogical(
                        source.try_logical_elt(i).unwrap_or_else(|error| {
                            crate::sexp::context::r_error(error.to_string())
                        }),
                        &mut warn,
                    );
                    if val == NA_INTEGER {
                        warn |= WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                SEXPTYPE::INTSXP => {
                    let val = source
                        .try_integer_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                    if val == NA_INTEGER || val < 0 || val > 255 {
                        warn |= WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                SEXPTYPE::REALSXP => {
                    let val = IntegerFromReal(
                        source.try_real_elt(i).unwrap_or_else(|error| {
                            crate::sexp::context::r_error(error.to_string())
                        }),
                        &mut warn,
                    );
                    if val == NA_INTEGER || val < 0 || val > 255 {
                        warn |= WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                SEXPTYPE::CPLXSXP => {
                    let val = IntegerFromComplex(
                        source.try_complex_elt(i).unwrap_or_else(|error| {
                            crate::sexp::context::r_error(error.to_string())
                        }),
                        &mut warn,
                    );
                    if val == NA_INTEGER || val < 0 || val > 255 {
                        warn |= WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                SEXPTYPE::STRSXP => {
                    let text = source
                        .try_string_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                    check();
                    let val = IntegerFromString(text.as_raw(), &mut warn);
                    if val == NA_INTEGER || val < 0 || val > 255 {
                        warn |= WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                _ => 0,
            };
            check();
            output
                .try_set_raw_elt(i, tmp as Rbyte)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        if warn != 0 {
            CoercionWarning(warn);
            check();
        }
        output.freeze().as_raw()
    }
}

/// Coerce a vector to string (character) type.
pub unsafe fn coerceToString(v: SEXP) -> SEXP {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let pin = owner
            .pin()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let factory = owner.node_factory();
        let source = factory
            .wrap(v)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let check = || {
            if let Some(pin) = &pin {
                pin.require_live()
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
            factory
                .require_active()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        };
        let n = source.len();
        check();
        // Keep the runtime's typed allocation-denial error and GC retry.
        let raw_output = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let output = factory
            .wrap(raw_output)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        check();
        SHALLOW_DUPLICATE_ATTRIB(output.as_raw(), source.as_raw());
        check();
        let mut output = crate::sexp::object::SexpMut::try_from_checked(output)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        // GNU pins R_print for numeric coercion, not the options map.
        // Cleanup restores the captured instance directly, even after revoke.
        struct RestoreStringPrint<'s> {
            owner: crate::sexp::owner::OwnerToken<'s>,
            _pin: Option<crate::sexp::owner::OwnerPin>,
            previous: crate::mainutils::format::RPrint,
            was_active: bool,
            entry_digits: i32,
        }
        impl Drop for RestoreStringPrint<'_> {
            fn drop(&mut self) {
                unsafe {
                    let state = &mut (*self.owner.as_ptr()).eval_state;
                    if self.was_active {
                        // GNU restores the digits read by PrintDefaults, not
                        // the enclosing conversion's temporary DBL_DIG.
                        state.format_print.digits = self.entry_digits;
                    } else {
                        state.format_print = self.previous;
                    }
                    state.character_print_active = self.was_active;
                }
            }
        }
        let numeric = matches!(source.typeof_(), SEXPTYPE::REALSXP | SEXPTYPE::CPLXSXP);
        let _print_guard = numeric.then(|| {
            let pin = owner
                .pin()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let previous = (*owner.as_ptr()).eval_state.format_print;
            let was_active = (*owner.as_ptr()).eval_state.character_print_active;
            let mut guard = RestoreStringPrint {
                owner,
                _pin: pin,
                previous,
                was_active,
                entry_digits: crate::mainutils::options::GetOptionDigits(),
            };
            crate::mainutils::print::PrintDefaults();
            check();
            let defaults = crate::mainutils::print::get_R_print_data();
            guard.entry_digits = defaults.digits;
            (*owner.as_ptr()).eval_state.format_print = crate::mainutils::format::RPrint {
                digits: 15,
                scipen: defaults.scipen,
                na_width: defaults.na_width,
                na_width_noquote: defaults.na_width_noquote,
            };
            (*owner.as_ptr()).eval_state.character_print_active = true;
            guard
        });
        let mut warn: c_int = 0;
        for i in 0..n {
            let chars = match source.typeof_() {
                SEXPTYPE::LGLSXP => {
                    let v = source
                        .try_logical_elt(i)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                    check();
                    StringFromLogical(v)
                }
                SEXPTYPE::INTSXP => {
                    let v = source
                        .try_integer_elt(i)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                    check();
                    StringFromInteger(v, &mut warn)
                }
                SEXPTYPE::REALSXP => {
                    let v = source
                        .try_real_elt(i)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                    check();
                    crate::mainutils::printutils::StringFromReal(v, &mut warn)
                }
                SEXPTYPE::CPLXSXP => {
                    let v = source
                        .try_complex_elt(i)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                    check();
                    StringFromComplex(v, &mut warn)
                }
                SEXPTYPE::RAWSXP => {
                    let v = source
                        .try_raw_elt(i)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                    check();
                    StringFromRaw(v, &mut warn)
                }
                _ => R_NaString(),
            };
            check();
            let chars = factory
                .wrap(chars)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            output
                .try_set_string_elt(i, chars)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        }
        if warn != 0 {
            CoercionWarning(warn);
            check();
        }
        output.freeze().as_raw()
    }
}

/// Coerce a vector to expression type.
pub unsafe fn coerceToExpression(v: SEXP) -> SEXP {
    unsafe {
        if !isVectorAtomic(v) {
            let ans = Rf_allocVector3(SEXPTYPE::EXPRSXP, 1);
            let _ans_guard = protect(ans);
            SET_VECTOR_ELT(ans, 0, v);
            return ans;
        }

        let n = xlength(v);
        let ans = Rf_allocVector3(SEXPTYPE::EXPRSXP, n);
        let _ans_guard = protect(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            let elt = match vtype {
                t if t == SEXPTYPE::LGLSXP => Rf_ScalarLogical(LOGICAL_ELT(v, ii)),
                t if t == SEXPTYPE::INTSXP => Rf_ScalarInteger(INTEGER_ELT(v, ii)),
                t if t == SEXPTYPE::REALSXP => Rf_ScalarReal(REAL_ELT(v, ii)),
                t if t == SEXPTYPE::CPLXSXP => Rf_ScalarComplex(COMPLEX_ELT(v, ii)),
                t if t == SEXPTYPE::STRSXP => Rf_ScalarString(STRING_ELT(v, i)),
                t if t == SEXPTYPE::RAWSXP => Rf_ScalarRaw(RAW_ELT(v, ii)),
                _ => R_NilValue(),
            };
            SET_VECTOR_ELT(ans, i, elt);
        }

        ans
    }
}

/// Coerce a vector to generic vector (list) type.
pub unsafe fn coerceToVectorList(v: SEXP) -> SEXP {
    unsafe {
        let n = xlength(v);
        let ans = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        let _ans_guard = protect(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            let elt = match vtype {
                t if t == SEXPTYPE::LGLSXP => Rf_ScalarLogical(LOGICAL_ELT(v, ii)),
                t if t == SEXPTYPE::INTSXP => Rf_ScalarInteger(INTEGER_ELT(v, ii)),
                t if t == SEXPTYPE::REALSXP => Rf_ScalarReal(REAL_ELT(v, ii)),
                t if t == SEXPTYPE::CPLXSXP => Rf_ScalarComplex(COMPLEX_ELT(v, ii)),
                t if t == SEXPTYPE::STRSXP => Rf_ScalarString(STRING_ELT(v, i)),
                t if t == SEXPTYPE::RAWSXP => Rf_ScalarRaw(RAW_ELT(v, ii)),
                t if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP => CAR(v.add(i as usize)),
                _ => R_NilValue(),
            };
            SET_VECTOR_ELT(ans, i, elt);
        }

        // Copy names attribute if present
        let names = getAttrib(v, R_NamesSymbol());
        if !isNull(names) {
            setAttrib(ans, R_NamesSymbol(), names);
        }

        ans
    }
}

/// Coerce a vector to pairlist type.
pub unsafe fn coerceToPairList(v: SEXP) -> SEXP {
    unsafe {
        let n = LENGTH(v);
        let ans = Rf_allocList(n);
        let _ans_guard = protect(ans);
        let mut ansp = ans;

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            match vtype {
                t if t == SEXPTYPE::LGLSXP => {
                    let elt = Rf_allocVector3(SEXPTYPE::LGLSXP, 1);
                    *LOGICAL(elt) = LOGICAL_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::INTSXP => {
                    let elt = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
                    *INTEGER(elt) = INTEGER_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::REALSXP => {
                    let elt = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
                    *REAL(elt) = REAL_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::CPLXSXP => {
                    let elt = Rf_allocVector3(SEXPTYPE::CPLXSXP, 1);
                    *COMPLEX(elt) = COMPLEX_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::STRSXP => {
                    SETCAR(ansp, Rf_ScalarString(STRING_ELT(v, i as R_xlen_t)));
                }
                t if t == SEXPTYPE::RAWSXP => {
                    let elt = Rf_allocVector3(SEXPTYPE::RAWSXP, 1);
                    *RAW(elt) = RAW_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                    SETCAR(ansp, VECTOR_ELT(v, i as R_xlen_t));
                }
                _ => {} // intentionally unhandled: unsupported SEXPTYPE for coercion
            }
            ansp = CDR(ansp);
        }

        // Copy names attribute if present
        let names = getAttrib(v, R_NamesSymbol());
        if !isNull(names) {
            setAttrib(ans, R_NamesSymbol(), names);
        }

        ans
    }
}

struct PairlistNative;
impl super::pairlist::Native for PairlistNative {
    fn scalar(
        &mut self,
        access: &crate::sexp::owner::RuntimeAccess,
        value: &Sexp<'static>,
        target: SEXPTYPE,
    ) -> crate::sexp::SexpResult<super::pairlist::Scalar> {
        use super::pairlist::Scalar;
        access.with_native(|_| unsafe {
            Ok(match target {
                SEXPTYPE::LGLSXP => Scalar::Logical(asLogical(value.as_raw())),
                SEXPTYPE::INTSXP => Scalar::Integer(asInteger(value.as_raw())),
                SEXPTYPE::REALSXP => Scalar::Real(asReal(value.as_raw())),
                SEXPTYPE::CPLXSXP => Scalar::Complex(asComplex(value.as_raw())),
                // GNU coercePairList uses the integer cast here, including
                // wrapping -1 and 256. Vector-list raw coercion differs.
                SEXPTYPE::RAWSXP => Scalar::Raw(asInteger(value.as_raw()) as Rbyte),
                _ => {
                    return Err(crate::sexp::SexpError::EvaluationFailed {
                        message: "unsupported pairlist scalar target".into(),
                    });
                }
            })
        })
    }
    fn deparse(
        &mut self,
        access: &crate::sexp::owner::RuntimeAccess,
        value: &Sexp<'static>,
    ) -> crate::sexp::SexpResult<Sexp<'static>> {
        access.with_native(|owner| unsafe {
            owner
                .sexp(crate::mainutils::deparse::deparse1line(
                    value.as_raw(),
                    false,
                ))?
                .into_owned()
        })
    }
    fn names(
        &mut self,
        access: &crate::sexp::owner::RuntimeAccess,
        value: &Sexp<'static>,
        names: &Sexp<'static>,
    ) -> crate::sexp::SexpResult<()> {
        access.with_native(|_| unsafe {
            setAttrib(value.as_raw(), R_NamesSymbol(), names.as_raw());
            Ok(())
        })
    }
}

unsafe fn checked_pairlist(value: SEXP, target: SEXPTYPE) -> SEXP {
    let result = (|| {
        let owner = unsafe { crate::sexp::owner::OwnerToken::current() }?
            .weak_owner()
            .ok_or(crate::sexp::SexpError::RootUnavailable)?;
        crate::sexp::owner::with_runtime(&owner, |access| {
            let value = access.domain().wrap(value)?.into_owned()?;
            super::pairlist::coerce(value, target, access, &mut PairlistNative)
        })?
    })();
    result
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        .as_raw()
}

/// Coerce a pairlist (LISTSXP/LANGSXP) to the given type.
pub unsafe fn coercePairList(v: SEXP, type_: SEXPTYPE) -> SEXP {
    unsafe {
        if type_ == SEXPTYPE::EXPRSXP {
            let rval = Rf_allocVector3(SEXPTYPE::EXPRSXP, 1);
            let _rval_guard = protect(rval);
            SET_VECTOR_ELT(rval, 0, v);
            return rval;
        }

        checked_pairlist(v, type_)
    }
}

/// Coerce a vector list (VECSXP/EXPRSXP) to the given type.
pub unsafe fn coerceVectorList(v: SEXP, type_: SEXPTYPE) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;

        // expression -> list: just change the type tag
        if type_ == SEXPTYPE::VECSXP && TYPEOF(v) == SEXPTYPE::EXPRSXP {
            let rval = Rf_allocVector3(SEXPTYPE::VECSXP, xlength(v));
            // Copy reference cells through the checked vector interface
            for index in 0..xlength(v) {
                SET_VECTOR_ELT(rval, index, VECTOR_ELT(v, index));
            }
            let attrs = crate::sexp::accessors::ATTRIB(v);
            if !isNull(attrs) {
                crate::sexp::accessors::SET_ATTRIB(
                    rval,
                    crate::mainutils::duplicate::duplicate(attrs),
                );
            }
            return rval;
        }

        // list -> expression: just change the type tag
        if type_ == SEXPTYPE::EXPRSXP && TYPEOF(v) == SEXPTYPE::VECSXP {
            let rval = Rf_allocVector3(SEXPTYPE::EXPRSXP, xlength(v));
            for index in 0..xlength(v) {
                SET_VECTOR_ELT(rval, index, VECTOR_ELT(v, index));
            }
            let attrs = crate::sexp::accessors::ATTRIB(v);
            if !isNull(attrs) {
                crate::sexp::accessors::SET_ATTRIB(
                    rval,
                    crate::mainutils::duplicate::duplicate(attrs),
                );
            }
            return rval;
        }

        // list -> pairlist
        if type_ == SEXPTYPE::LISTSXP {
            // VectorToPairList — GNU returns NULL for a length-0 list.
            let n = LENGTH(v);
            if n == 0 {
                return R_NilValue();
            }
            let x = Rf_allocList(n);

            let _x_guard = protect(x);
            let names = getAttrib(v, R_NamesSymbol());
            let _names_guard = protect(names);
            let mut xptr = x;
            for i in 0..n {
                SETCAR(xptr, VECTOR_ELT(v, i as R_xlen_t));
                xptr = CDR(xptr);
            }
            if !isNull(names) {
                let mut xptr2 = x;
                for i in 0..n {
                    let name_elt = STRING_ELT(names, i as R_xlen_t);
                    if !isNull(name_elt) {
                        let pname = CHAR(name_elt);
                        if !pname.is_null() && *pname != 0 {
                            SETTAG(xptr2, Rf_install(pname));
                        }
                    }
                    xptr2 = CDR(xptr2);
                }
            }
            return x;
        }

        // list -> string
        if type_ == SEXPTYPE::STRSXP {
            let n = xlength(v);
            let rval = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            let _rval_guard = protect(rval);
            for i in 0..n {
                let elt = VECTOR_ELT(v, i);
                if isString(elt) && LENGTH(elt) == 1 {
                    SET_STRING_ELT(rval, i, STRING_ELT(elt, 0));
                } else {
                    // coerce.c coerceVectorList: non-trivial entries are
                    // deparsed onto a single line (deparse1line_ex with
                    // NICE_NAMES); e.g. calls -> "f(1)", NULL -> "NULL",
                    // .Primitive("sin") -> ".Primitive(\"sin\")".
                    let dep = crate::mainutils::deparse::deparse1line(elt, false);
                    if !dep.is_null()
                        && dep != R_NilValue()
                        && TYPEOF(dep) == SEXPTYPE::STRSXP
                        && xlength(dep) > 0
                    {
                        SET_STRING_ELT(rval, i, STRING_ELT(dep, 0));
                    } else {
                        SET_STRING_ELT(rval, i, R_NaString());
                    }
                }
            }
            return rval;
        }

        if isVectorizable(v) {
            let n = xlength(v);
            let rval = Rf_allocVector3(type_.0, n);
            let _rval_guard = protect(rval);
            match type_.0 {
                t if t == SEXPTYPE::LGLSXP => {
                    for i in 0..n {
                        *LOGICAL(rval).add(i as usize) = asLogical(VECTOR_ELT(v, i));
                    }
                }
                t if t == SEXPTYPE::INTSXP => {
                    for i in 0..n {
                        *INTEGER(rval).add(i as usize) = asInteger(VECTOR_ELT(v, i));
                    }
                }
                t if t == SEXPTYPE::REALSXP => {
                    for i in 0..n {
                        *REAL(rval).add(i as usize) = asReal(VECTOR_ELT(v, i));
                    }
                }
                t if t == SEXPTYPE::CPLXSXP => {
                    for i in 0..n {
                        *COMPLEX(rval).add(i as usize) = asComplex(VECTOR_ELT(v, i));
                    }
                }
                t if t == SEXPTYPE::RAWSXP => {
                    for i in 0..n {
                        let tmp = asInteger(VECTOR_ELT(v, i));
                        if tmp < 0 || tmp > 255 {
                            warn |= WARN_RAW;
                        }
                        *RAW(rval).add(i as usize) = if tmp < 0 || tmp > 255 {
                            0
                        } else {
                            tmp as Rbyte
                        };
                    }
                }
                _ => {} // intentionally unhandled: unsupported SEXPTYPE for coercion
            }
            if warn != 0 {
                CoercionWarning(warn);
            }
            let names = getAttrib(v, R_NamesSymbol());
            if !isNull(names) {
                setAttrib(rval, R_NamesSymbol(), names);
            }
            return rval;
        }

        let target_name = CStr::from_ptr(type2char(type_.into())).to_string_lossy();
        error(&format!("'list' object cannot be coerced to type '{target_name}'"));
    }
}

/// Coerce to a symbol.
pub unsafe fn coerceToSymbol(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        if LENGTH(v) <= 0 {
            error("invalid data of mode (too short)");
        }

        let ans = match TYPEOF(v) {
            t if t == SEXPTYPE::LGLSXP => StringFromLogical(LOGICAL_ELT(v, 0)),
            t if t == SEXPTYPE::INTSXP => StringFromInteger(INTEGER_ELT(v, 0), &mut warn),
            t if t == SEXPTYPE::REALSXP => {
                crate::mainutils::printutils::StringFromReal(REAL_ELT(v, 0), &mut warn)
            }
            t if t == SEXPTYPE::CPLXSXP => StringFromComplex(COMPLEX_ELT(v, 0), &mut warn),
            t if t == SEXPTYPE::STRSXP => STRING_ELT(v, 0),
            t if t == SEXPTYPE::RAWSXP => StringFromRaw(RAW_ELT(v, 0), &mut warn),
            _ => R_NilValue(),
        };
        let _ans_guard = protect(ans);

        if warn != 0 {
            CoercionWarning(warn);
        }

        let sym = Rf_install(CHAR(ans));
        sym
    }
}

/// Coerce a symbol (SYMSXP) to the given type.
/// This matches R's coerceSymbol() from coerce.c.
pub unsafe fn coerceSymbol(v: SEXP, type_: SEXPTYPE) -> SEXP {
    unsafe {
        if type_ == SEXPTYPE::EXPRSXP {
            let rval = Rf_allocVector3(type_.0, 1);
            let _rval_guard = protect(rval);
            SET_VECTOR_ELT(rval, 0, v);
            rval
        } else if type_ == SEXPTYPE::CHARSXP {
            PRINTNAME(v)
        } else if type_ == SEXPTYPE::STRSXP {
            Rf_ScalarString(PRINTNAME(v))
        } else {
            let target_name = CStr::from_ptr(type2char(type_.0)).to_string_lossy();
            error(&format!(
                "'symbol' object cannot be coerced to type '{}'",
                target_name
            ));
        }
    }
}

/// Create a tag (symbol) from an SEXP.
/// If x is already a symbol or NULL, return it. If x is a string of length >= 1,
/// install it as a symbol.
pub unsafe fn CreateTag(x: SEXP) -> SEXP {
    unsafe {
        if isNull(x) || isSymbol(x) {
            return x;
        }
        if isString(x) && LENGTH(x) >= 1 {
            let s = STRING_ELT(x, 0);
            if !isNull(s) {
                let cs = CHAR(s);
                if !cs.is_null() && *cs != 0 {
                    return installTrChar(s);
                }
            }
        }
        // fallback: return NULL
        R_NilValue()
    }
}

/// Convert an SEXP to a function (closure).
/// This matches R's asFunction() from coerce.c.
pub unsafe fn asFunction(x: SEXP) -> SEXP {
    unsafe {
        if isFunction(x) {
            return x;
        }
        let f = allocSExp(SEXPTYPE::CLOSXP);
        let _f_guard = protect(f);
        SET_CLOENV(f, R_GlobalEnv());
        // For simplicity, create a closure with empty formals and body = x
        SET_FORMALS(f, R_NilValue());
        SET_BODY(f, x);
        f
    }
}

/// Common coercion helper for as.vector / typed coercion dispatch.
/// This matches R's ascommon() from coerce.c.
pub unsafe fn ascommon(call: SEXP, u: SEXP, type_: c_int) -> SEXP {
    unsafe {
        let target_type = SEXPTYPE(type_);

        if target_type == SEXPTYPE::CLOSXP {
            return asFunction(u);
        }

        if isVector(u)
            || isList(u)
            || isLanguage(u)
            || (isSymbol(u) && target_type == SEXPTYPE::EXPRSXP)
        {
            let v = if type_ != SEXPTYPE::ANYSXP && TYPEOF(u) != type_ {
                coerceVector(u, type_)
            } else {
                u
            };

            // Drop attributes for certain types (as.pairlist behavior)
            if target_type == SEXPTYPE::LISTSXP
                && TYPEOF(u) != SEXPTYPE::LANGSXP
                && TYPEOF(u) != SEXPTYPE::LISTSXP
                && TYPEOF(u) != SEXPTYPE::EXPRSXP
                && TYPEOF(u) != SEXPTYPE::VECSXP
            {
                // Clear attributes
                let attr = ATTRIB(v);
                if !isNull(attr) {
                    SET_ATTRIB(v, R_NilValue());
                }
            }
            return v;
        }

        if isSymbol(u) && target_type == SEXPTYPE::STRSXP {
            return Rf_ScalarString(PRINTNAME(u));
        }
        if isSymbol(u) && target_type == SEXPTYPE::SYMSXP {
            return u;
        }
        if isSymbol(u) && target_type == SEXPTYPE::VECSXP {
            let v = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
            SET_VECTOR_ELT(v, 0, u);
            return v;
        }

        errorcall(call, "cannot coerce type to vector of type");
    }
}

// ---------------------------------------------------------------------------
// coerceVector -- main coercion dispatcher
// ---------------------------------------------------------------------------

/// Coerce a vector from one type to another.
///
/// This is the main entry point for type coercion in R, equivalent to
/// R's `coerceVector()` from coerce.c. It dispatches to the appropriate
/// type-specific coercion function based on the source and target types.
pub unsafe fn coerceVector(v: SEXP, type_: c_int) -> SEXP {
    unsafe {
        if v.is_null() {
            return ptr::null_mut();
        }
        let target = SEXPTYPE(type_);

        // If already the right type, return as-is
        if TYPEOF(v) == type_ {
            return v;
        }

        let _v_guard = protect(v);

        let ans = match TYPEOF(v) {
            t if t == SEXPTYPE::SYMSXP => coerceSymbol(v, target),
            t if t == SEXPTYPE::NILSXP => {
                // GNU coercePairList represents NULL as one expression cell,
                // while its atomic/list conversions have no elements.
                if target == SEXPTYPE::EXPRSXP {
                    coercePairList(v, target)
                } else {
                    Rf_allocVector3(target, 0)
                }
            }
            t if t == SEXPTYPE::LISTSXP => {
                if type_ == SEXPTYPE::LISTSXP {
                    v // already pairlist
                } else {
                    coercePairList(v, target)
                }
            }
            // Calls have linked cells, never an atomic vector length field.
            // Reuse the counted pairlist path, including element deparsing.
            t if t == SEXPTYPE::LANGSXP => coercePairList(v, target),
            t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => coerceVectorList(v, target),
            t if t == SEXPTYPE::ENVSXP => {
                error("environments cannot be coerced to other types");
            }
            // Atomic vector types
            t if t == SEXPTYPE::LGLSXP
                || t == SEXPTYPE::INTSXP
                || t == SEXPTYPE::REALSXP
                || t == SEXPTYPE::CPLXSXP
                || t == SEXPTYPE::STRSXP
                || t == SEXPTYPE::RAWSXP =>
            {
                match type_ {
                    t if t == SEXPTYPE::SYMSXP => coerceToSymbol(v),
                    t if t == SEXPTYPE::LGLSXP => coerceToLogical(v),
                    t if t == SEXPTYPE::INTSXP => coerceToInteger(v),
                    t if t == SEXPTYPE::REALSXP => coerceToReal(v),
                    t if t == SEXPTYPE::CPLXSXP => coerceToComplex(v),
                    t if t == SEXPTYPE::RAWSXP => coerceToRaw(v),
                    t if t == SEXPTYPE::STRSXP => coerceToString(v),
                    t if t == SEXPTYPE::EXPRSXP => coerceToExpression(v),
                    t if t == SEXPTYPE::VECSXP => coerceToVectorList(v),
                    t if t == SEXPTYPE::LISTSXP => coerceToPairList(v),
                    _ => {
                        error("cannot coerce type to vector of type");
                    }
                }
            }
            _ => {
                error("cannot coerce type to vector of type");
            }
        };

        ans
    }
}
