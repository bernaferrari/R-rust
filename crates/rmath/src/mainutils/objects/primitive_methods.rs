#![allow(
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_imports
)]

use super::*;
use crate::sexp::{instance::RuntimeValue, object::Sexp};

/// Restore the original physical runtime even when a method throws or revokes
/// its execution authority. Cleanup never executes R or follows ambient TLS.
struct PrimitiveStatusGuard {
    pin: crate::sexp::owner::OwnerPin,
    offset: usize,
    status: prim_methods_t,
    armed: bool,
}

impl PrimitiveStatusGuard {
    fn new(
        access: &crate::sexp::owner::RuntimeAccess,
        offset: usize,
        status: prim_methods_t,
    ) -> crate::sexp::object::SexpResult<Self> {
        let pin = access.with_native(|owner| {
            owner
                .pin()?
                .ok_or(crate::sexp::object::SexpError::RootUnavailable)
        })?;
        Ok(Self {
            pin,
            offset,
            status,
            armed: true,
        })
    }
}

impl Drop for PrimitiveStatusGuard {
    fn drop(&mut self) {
        if self.armed {
            unsafe {
                if let Some(slot) =
                    (&mut (*self.pin.as_ptr()).objects_state.prim_methods).get_mut(self.offset)
                {
                    *slot = self.status;
                }
            }
        }
    }
}

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
            b's' | b'S' => {
                let next = *code_string.add(1) as u8;
                if next == b'e' || next == b'E' {
                    prim_methods_t::HAS_METHODS
                } else if next == b'u' || next == b'U' {
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

        with_objects_runtime(|access| {
            let domain = access.domain();
            let fundef = if fundef.is_null() || fundef == domain.nil().as_raw() {
                None
            } else {
                Some(domain.wrap(fundef)?)
            };
            let mlist = if mlist.is_null() || mlist == domain.nil().as_raw() {
                None
            } else {
                Some(domain.wrap(mlist)?)
            };
            let generic = objects_state_in(access, |state| {
                state.ensure_primitive_slot(offset);
                let previous = state.prim_generics[offset].owned();
                if !matches!(
                    code,
                    prim_methods_t::NO_METHODS | prim_methods_t::SUPPRESSED
                ) && state.prim_generics[offset].is_null()
                    && fundef
                        .as_ref()
                        .is_some_and(|fun| fun.typeof_() != SEXPTYPE::CLOSXP)
                {
                    error("the formal definition of a primitive generic must be a function object");
                }

                state.prim_methods[offset] = code;
                if offset as c_int > state.cur_max_offset {
                    state.cur_max_offset = offset as c_int;
                }

                if code == prim_methods_t::NO_METHODS {
                    state.prim_generics[offset] = RuntimeValue::empty();
                    state.prim_mlist[offset] = RuntimeValue::empty();
                } else if code != prim_methods_t::SUPPRESSED
                    && state.prim_generics[offset].is_null()
                {
                    if let Some(fundef) = fundef {
                        state.prim_generics[offset] = RuntimeValue::from_owned(fundef);
                    }
                }

                if code == prim_methods_t::HAS_METHODS {
                    if let Some(mlist) = mlist {
                        state.prim_mlist[offset] = RuntimeValue::from_owned(mlist);
                    }
                }
                previous
            })?;
            Ok(generic.as_ref().map_or(ptr::null_mut(), Sexp::as_raw))
        })
    }
}

/// R_set_prim_method -- public API for setting primitive methods.
pub unsafe fn R_set_prim_method(
    fname: SEXP,
    mut op: SEXP,
    code_vec: SEXP,
    fundef: SEXP,
    mlist: SEXP,
) -> SEXP {
    unsafe {
        if code_vec.is_null() || isValidString(code_vec) == FALSE {
            error("argument 'code' must be a character string");
        }
        let code_string = CHAR(STRING_ELT(code_vec, 0));
        if op.is_null() || op == R_NilValue() {
            let previous = with_objects_state(|state| state.allow_primitive_methods);
            match *code_string as u8 {
                b'c' | b'C' => {
                    with_objects_state(|state| state.allow_primitive_methods = FALSE);
                }
                b's' | b'S' => {
                    with_objects_state(|state| state.allow_primitive_methods = TRUE);
                }
                _ => {}
            }
            return Rf_ScalarLogical(previous);
        }
        if isPrimitive(op) == FALSE {
            let internal = crate::mainutils::essentials::R_do_slot(
                op,
                crate::sexp::symbol::Rf_install(c"internal".as_ptr()),
            );
            let mut name = if TYPEOF(internal) == SEXPTYPE::STRSXP && XLENGTH(internal) > 0 {
                crate::sexp::symbol::Rf_install(CHAR(STRING_ELT(internal, 0)))
            } else {
                R_NilValue()
            };
            // GNU resetGeneric passes fname as the generic name. extraS4
            // wrappers are closures in SYMVALUE; the FunTab INTERNAL slot
            // still names the .Internal primitive.
            if name.is_null() || name == R_NilValue() {
                if TYPEOF(fname) == SEXPTYPE::STRSXP && XLENGTH(fname) > 0 {
                    name = crate::sexp::symbol::Rf_install(CHAR(STRING_ELT(fname, 0)));
                } else if TYPEOF(fname) == SEXPTYPE::SYMSXP {
                    name = fname;
                }
            }
            op = if !name.is_null() && name != R_NilValue() {
                crate::sexp::accessors::INTERNAL(name)
            } else {
                R_NilValue()
            };
            if op.is_null() || op == R_NilValue() {
                return fname;
            }
        }

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
                .and_then(RuntimeValue::owned)
                .map(|value| value.as_raw())
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
                .and_then(RuntimeValue::owned)
                .map(|value| value.as_raw())
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
    with_objects_runtime(|access| {
        if let Some(marker) =
            unsafe { objects_state_in(access, |state| state.deferred_default_object.owned()) }?
        {
            return Ok(marker.as_raw());
        }
        let marker = access.with_native(|owner| {
            let raw = unsafe { Rf_install(c"__Deferred_Default_Marker__".as_ptr()) };
            owner.sexp(raw)?.into_owned()
        })?;
        let marker = unsafe {
            objects_state_in(access, |state| {
                if state.deferred_default_object.is_null() {
                    state.deferred_default_object = RuntimeValue::from_owned(marker);
                }
                state
                    .deferred_default_object
                    .owned()
                    .expect("published default marker")
            })
        }?;
        Ok(marker.as_raw())
    })
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
    with_objects_runtime(|access| {
        let result = unsafe { possible_dispatch_in(access, call, op, args, rho, promisedArgs) };
        access.require_active()?;
        Ok(result)
    })
}

unsafe fn possible_dispatch_in(
    access: &crate::sexp::owner::RuntimeAccess,
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
    promisedArgs: c_int,
) -> SEXP {
    unsafe {
        let factory = crate::eval::parser::active_factory();
        let _operation = if op.is_null() {
            factory.nil()
        } else {
            factory
                .wrap(op)
                .expect("primitive operation belongs to the active heap")
        };
        let call_owner = if call.is_null() {
            factory.nil()
        } else {
            factory
                .wrap(call)
                .expect("primitive dispatch call belongs to the active heap")
        };
        let _arguments = if args.is_null() {
            factory.nil()
        } else {
            factory
                .wrap(args)
                .expect("primitive dispatch arguments belong to the active heap")
        };
        let environment = if rho.is_null() {
            factory.nil()
        } else {
            factory
                .wrap(rho)
                .expect("primitive dispatch environment belongs to the active heap")
        };
        let offset = PRIMOFFSET(op);
        let cur_max = with_objects_state(|state| state.cur_max_offset);
        if offset < 0 || offset > cur_max {
            error("invalid primitive operation given for dispatch");
        }

        let mut current = with_objects_state(|state| {
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
            let mut reset =
                require_objects(PrimitiveStatusGuard::new(access, offset as usize, current));
            do_set_prim_method(
                op,
                b"suppressed\x00".as_ptr() as *const c_char,
                R_NilValue(),
                R_NilValue(),
            );
            let mlist_owner = require_objects(get_primitive_methods(access, op, rho));
            let mlist = mlist_owner.as_raw();
            require_objects(access.require_active());
            let _mlist_guard = protect(mlist);
            do_set_prim_method(
                op,
                b"set\x00".as_ptr() as *const c_char,
                R_NilValue(),
                mlist,
            );
            current = with_objects_state(|state| state.prim_methods[offset as usize]);
            reset.armed = false;
        }

        let mlist_owner = with_objects_state(|state| {
            state
                .prim_mlist
                .get(offset as usize)
                .and_then(RuntimeValue::owned)
        });
        let mlist = mlist_owner.as_ref().map_or(ptr::null_mut(), Sexp::as_raw);

        // Try the quick method check
        if !mlist.is_null() && isNull(mlist) == FALSE {
            let qmc = with_objects_state(|state| state.quick_method_check_ptr);
            if let Some(check_fn) = qmc {
                let value = require_objects(access.with_native(|owner| {
                    let raw = check_fn(args, mlist, op);
                    if raw.is_null() {
                        Ok(access.domain().nil())
                    } else {
                        owner.sexp(raw)?.into_owned()
                    }
                }));
                let value_owner = value;
                let value = value_owner.as_raw();
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
                    SETTAG(suppliedvars, Rf_install(c".Generic".as_ptr()));
                    require_objects(access.require_active());

                    if promisedArgs == FALSE {
                        let expressions = if call_owner.is_nil() {
                            factory.nil()
                        } else {
                            factory
                                .wrap(CDR(call_owner.as_raw()))
                                .expect("dispatch expressions remain live")
                        };
                        let promised = crate::eval::dispatch::promiseArgs(
                            &factory,
                            expressions,
                            environment.clone(),
                        );
                        let s = promised.as_raw();
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
                        let value = crate::eval::closure::applyClosureWithFrameVars(
                            call,
                            value,
                            s,
                            rho,
                            R_NilValue(),
                            suppliedvars,
                            TRUE,
                        );
                        require_objects(access.require_active());
                        return value;
                    } else {
                        let value = crate::eval::closure::applyClosureWithFrameVars(
                            call,
                            value,
                            args,
                            rho,
                            R_NilValue(),
                            suppliedvars,
                            FALSE,
                        );
                        require_objects(access.require_active());
                        return value;
                    }
                }
            }
        }

        // Fall back to full generic dispatch via prim_generics
        let fundef_owner = with_objects_state(|state| {
            state
                .prim_generics
                .get(offset as usize)
                .and_then(RuntimeValue::owned)
        });
        let fundef = fundef_owner.as_ref().map_or(ptr::null_mut(), Sexp::as_raw);

        if fundef.is_null() || TYPEOF(fundef) != SEXPTYPE::CLOSXP {
            error("primitive function has been set for methods but no generic function supplied");
        }
        let _restore = require_objects(PrimitiveStatusGuard::new(access, offset as usize, current));

        if promisedArgs == FALSE {
            let expressions = if call_owner.is_nil() {
                factory.nil()
            } else {
                factory
                    .wrap(CDR(call_owner.as_raw()))
                    .expect("dispatch expressions remain live")
            };
            let promised = crate::eval::dispatch::promiseArgs(&factory, expressions, environment);
            let s = promised.as_raw();
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
            require_objects(access.require_active());
            let _value_owner = require_objects(factory.wrap(value));
            if value == R_deferred_default_method() {
                return ptr::null_mut();
            }
            return value;
        } else {
            let value =
                crate::eval::closure::applyClosure(call, fundef, args, rho, R_NilValue(), FALSE);
            require_objects(access.require_active());
            let _value_owner = require_objects(factory.wrap(value));
            if value == R_deferred_default_method() {
                return ptr::null_mut();
            }
            return value;
        }
    }
}

unsafe fn get_primitive_methods(
    access: &crate::sexp::owner::RuntimeAccess,
    op: SEXP,
    rho: SEXP,
) -> crate::sexp::object::SexpResult<Sexp<'static>> {
    unsafe {
        crate::mainutils::essentials::ensure_captured_primitive_generic(op);
        access.require_active()?;
        // Discovering the generic can itself evaluate R; maintain the same
        // suppression that GNU applies over the entire reset operation.
        do_set_prim_method(op, c"suppressed".as_ptr(), R_NilValue(), R_NilValue());
        let domain = access.domain();
        let environment = domain.wrap(rho)?;
        let allocator = access.allocator(&domain)?;
        let name =
            std::ffi::CStr::from_ptr(crate::mainutils::relop::PRIMNAME(op)).to_string_lossy();
        let name_value = allocator.strings(&[&name])?;
        let get_generic = access
            .with_native(|owner| owner.sexp(Rf_install(c"getGeneric".as_ptr()))?.into_owned())?;
        let arguments = allocator.pairlist_cell(&name_value, &domain.nil(), &domain.nil())?;
        let expression = allocator.call(&get_generic, &arguments)?;
        let generic = evaluate_s4_value(access, &expression, &environment)?;
        access.require_active()?;
        if generic.typeof_() != SEXPTYPE::CLOSXP || IS_S4_OBJECT(generic.as_raw()) == FALSE {
            return Err(crate::sexp::object::SexpError::EvaluationFailed {
                message: format!(
                    "object returned as generic function \"{name}\" does not appear to be one"
                ),
            });
        }
        generic.try_cloenv()?.into_owned()
    }
}
