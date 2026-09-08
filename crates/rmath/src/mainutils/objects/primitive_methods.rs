#![allow(
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_imports
)]

use super::*;

// ---------------------------------------------------------------------------
// Primitive method dispatch infrastructure
// ---------------------------------------------------------------------------

/// Set or query the primitive method table for a given operation.
pub unsafe fn do_set_prim_method(
    op: SEXP,
    code_string: *const c_char,
    fundef: SEXP,
    mlist: SEXP,
) -> SEXP {
    unsafe {
        if code_string.is_null() {
            error(
                "invalid primitive methods code: should be \"clear\", \"reset\", \"set\", or \"suppress\"",
            );
        }

        let code = match *code_string as u8 {
            b'c' | b'C' => prim_methods_t::NO_METHODS,
            b'r' | b'R' => prim_methods_t::NEEDS_RESET,
            b's' => {
                if *code_string.add(1) as u8 == b'e' {
                    prim_methods_t::HAS_METHODS
                } else if *code_string.add(1) as u8 == b'u' {
                    prim_methods_t::SUPPRESSED
                } else {
                    error(
                        "invalid primitive methods code: should be \"clear\", \"reset\", \"set\", or \"suppress\"",
                    );
                }
            }
            _ => error(
                "invalid primitive methods code: should be \"clear\", \"reset\", \"set\", or \"suppress\"",
            ),
        };

        let Some(offset) = primitive_offset(op) else {
            error("invalid object: must be a primitive function");
        };

        with_objects_state(|state| {
            state.ensure_primitive_slot(offset);

            state.prim_methods[offset] = code;
            if offset as c_int > state.cur_max_offset {
                state.cur_max_offset = offset as c_int;
            }

            if code == prim_methods_t::NO_METHODS {
                state.prim_generics[offset] = ptr::null_mut();
                state.prim_mlist[offset] = ptr::null_mut();
            } else if !fundef.is_null()
                && fundef != R_NilValue()
                && state.prim_generics[offset].is_null()
            {
                state.prim_generics[offset] = fundef;
            }

            if code == prim_methods_t::HAS_METHODS && !mlist.is_null() && mlist != R_NilValue() {
                state.prim_mlist[offset] = mlist;
            }

            if state.prim_generics[offset].is_null() {
                R_NilValue()
            } else {
                state.prim_generics[offset]
            }
        })
    }
}

/// R_set_prim_method -- public API for setting primitive methods.
pub unsafe fn R_set_prim_method(
    fname: SEXP,
    op: SEXP,
    code_vec: SEXP,
    fundef: SEXP,
    mlist: SEXP,
) -> SEXP {
    unsafe {
        if code_vec.is_null() || isValidString(code_vec) == FALSE {
            return R_NilValue();
        }
        let code_string = CHAR(STRING_ELT(code_vec, 0));
        do_set_prim_method(op, code_string, fundef, mlist);
        fname
    }
}

/// R_primitive_methods -- get the methods list for a primitive.
pub unsafe fn R_primitive_methods(op: SEXP) -> SEXP {
    unsafe {
        let Some(offset) = primitive_offset(op) else {
            return R_NilValue();
        };
        with_objects_state(|state| {
            state
                .prim_mlist
                .get(offset)
                .copied()
                .filter(|value| !value.is_null())
                .unwrap_or_else(|| unsafe { R_NilValue() })
        })
    }
}

/// R_primitive_generic -- get the generic function for a primitive.
pub unsafe fn R_primitive_generic(op: SEXP) -> SEXP {
    unsafe {
        let Some(offset) = primitive_offset(op) else {
            return R_NilValue();
        };
        with_objects_state(|state| {
            state
                .prim_generics
                .get(offset)
                .copied()
                .filter(|value| !value.is_null())
                .unwrap_or_else(|| unsafe { R_NilValue() })
        })
    }
}

/// R_has_methods -- check whether methods might exist for this op.
pub unsafe fn R_has_methods(_op: SEXP) -> c_int {
    unsafe {
        let ptr = R_get_standardGeneric_ptr();
        if ptr.is_none() {
            return FALSE;
        }
        if _op.is_null() || TYPEOF(_op) == SEXPTYPE::CLOSXP {
            return TRUE;
        }
        if with_objects_state(|state| state.allow_primitive_methods) == FALSE {
            return FALSE;
        }
        let Some(offset) = primitive_offset(_op) else {
            return FALSE;
        };
        with_objects_state(|state| {
            !matches!(
                state
                    .prim_methods
                    .get(offset)
                    .copied()
                    .unwrap_or(prim_methods_t::NO_METHODS),
                prim_methods_t::NO_METHODS | prim_methods_t::SUPPRESSED
            ) as c_int
        })
    }
}

/// R_deferred_default_method -- return the deferred default method marker.
pub unsafe fn R_deferred_default_method() -> SEXP {
    unsafe {
        with_objects_state(|state| {
            if state.deferred_default_object.is_null() {
                state.deferred_default_object =
                    Rf_install(b"__Deferred_Default_Marker__\x00".as_ptr() as *const c_char);
            }
            state.deferred_default_object
        })
    }
}

/// R_set_quick_method_check -- set the quick method check function pointer.
pub unsafe fn R_set_quick_method_check(_value: R_stdGen_ptr_t) {
    with_objects_state(|state| {
        state.quick_method_check_ptr = _value;
    });
}

/// R_possible_dispatch -- try to dispatch a formal method for a primitive.
///
/// Main entry point for S4 method dispatch on primitive functions.
/// Ported from objects.c:1610-1696.
pub unsafe fn R_possible_dispatch(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
    promisedArgs: c_int,
) -> SEXP {
    unsafe {
        let offset = PRIMOFFSET(op);
        let cur_max = with_objects_state(|state| state.cur_max_offset);
        if offset < 0 || offset > cur_max {
            error("invalid primitive operation given for dispatch");
        }

        let current = with_objects_state(|state| {
            state
                .prim_methods
                .get(offset as usize)
                .copied()
                .unwrap_or(prim_methods_t::NO_METHODS)
        });
        if current == prim_methods_t::NO_METHODS {
            return ptr::null_mut();
        }

        if current == prim_methods_t::SUPPRESSED {
            return ptr::null_mut();
        }

        if current == prim_methods_t::NEEDS_RESET {
            do_set_prim_method(
                op,
                b"suppressed\x00".as_ptr() as *const c_char,
                R_NilValue(),
                R_NilValue(),
            );
            let mlist = get_primitive_methods(op, rho);
            let _mlist_guard = protect(mlist);
            do_set_prim_method(
                op,
                b"set\x00".as_ptr() as *const c_char,
                R_NilValue(),
                mlist,
            );
        }

        let mlist = with_objects_state(|state| {
            state
                .prim_mlist
                .get(offset as usize)
                .copied()
                .unwrap_or(ptr::null_mut())
        });

        // Try the quick method check
        if !mlist.is_null() && isNull(mlist) == FALSE {
            let qmc = with_objects_state(|state| state.quick_method_check_ptr);
            if let Some(check_fn) = qmc {
                let value = check_fn(args, mlist, op);
                if isPrimitive(value) != FALSE {
                    return ptr::null_mut();
                }
                if isFunction(value) != FALSE {
                    if inherits2(
                        value,
                        b"internalDispatchMethod\x00".as_ptr() as *const c_char,
                    ) != FALSE
                    {
                        return ptr::null_mut();
                    }

                    let prim_name_ptr = crate::mainutils::relop::PRIMNAME(op);
                    let suppliedvars = crate::sexp::memory_ext::allocList(1);
                    let _suppliedvars_guard = protect(suppliedvars);
                    SETCAR(suppliedvars, Rf_mkString(prim_name_ptr));
                    SETTAG(
                        suppliedvars,
                        Rf_install(b"Generic\x00".as_ptr() as *const c_char),
                    );

                    if promisedArgs == FALSE {
                        let s = crate::eval::dispatch::promiseArgs(CDR(call), rho);
                        let _s_guard = protect(s);
                        if length(s) != length(args) {
                            error("dispatch error");
                        }
                        let mut a = args;
                        let mut b = s;
                        while !a.is_null() && a != R_NilValue() {
                            if !b.is_null()
                                && b != R_NilValue()
                                && TYPEOF(CAR(b)) == SEXPTYPE::PROMSXP
                            {
                                SET_PRVALUE(CAR(b), CAR(a));
                            }
                            a = CDR(a);
                            b = CDR(b);
                        }
                        let value = crate::eval::closure::applyClosure(
                            call,
                            value,
                            s,
                            rho,
                            suppliedvars,
                            TRUE,
                        );
                        return value;
                    } else {
                        let value = crate::eval::closure::applyClosure(
                            call,
                            value,
                            args,
                            rho,
                            suppliedvars,
                            FALSE,
                        );
                        return value;
                    }
                }
            }
        }

        // Fall back to full generic dispatch via prim_generics
        let fundef = with_objects_state(|state| {
            state
                .prim_generics
                .get(offset as usize)
                .copied()
                .unwrap_or(ptr::null_mut())
        });

        if fundef.is_null() || TYPEOF(fundef) != SEXPTYPE::CLOSXP {
            error("primitive function has been set for methods but no generic function supplied");
        }

        if promisedArgs == FALSE {
            let s = crate::eval::dispatch::promiseArgs(CDR(call), rho);
            let _s_guard = protect(s);
            if length(s) != length(args) {
                error("dispatch error");
            }
            let mut a = args;
            let mut b = s;
            while !a.is_null() && a != R_NilValue() {
                if !b.is_null() && b != R_NilValue() && TYPEOF(CAR(b)) == SEXPTYPE::PROMSXP {
                    SET_PRVALUE(CAR(b), CAR(a));
                }
                a = CDR(a);
                b = CDR(b);
            }
            let value =
                crate::eval::closure::applyClosure(call, fundef, s, rho, R_NilValue(), TRUE);
            with_objects_state(|state| {
                if let Some(slot) = state.prim_methods.get_mut(offset as usize) {
                    *slot = current;
                }
            });
            if value == R_deferred_default_method() {
                return ptr::null_mut();
            }
            return value;
        } else {
            let value =
                crate::eval::closure::applyClosure(call, fundef, args, rho, R_NilValue(), FALSE);
            with_objects_state(|state| {
                if let Some(slot) = state.prim_methods.get_mut(offset as usize) {
                    *slot = current;
                }
            });
            if value == R_deferred_default_method() {
                return ptr::null_mut();
            }
            return value;
        }
    }
}

unsafe fn get_primitive_methods(op: SEXP, _rho: SEXP) -> SEXP {
    unsafe { R_primitive_methods(op) }
}
