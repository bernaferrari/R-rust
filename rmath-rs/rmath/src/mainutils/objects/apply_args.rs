#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables, unused_imports)]

use super::*;

// ---------------------------------------------------------------------------
// GetObject -- get the dispatch object from the calling context
// ---------------------------------------------------------------------------

/// Get the object argument for method dispatch from the calling context.
/// This examines the generic function's formals and matched arguments.
unsafe fn GetObject(cptr: *mut RCNTXT) -> SEXP {
    unsafe {
        if cptr.is_null() {
            return R_NilValue();
        }

        let b = (*cptr).closure; // callfun
        if TYPEOF(b) != SEXPTYPE::CLOSXP {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "generic 'function' is not a function".to_string(),
            });
        }

        let formals = FORMALS(b);
        let tag = TAG(formals);

        let mut s: SEXP = ptr::null_mut();

        if !tag.is_null() && tag != R_NilValue() && tag != sym("...") {
            // Try exact match on first formal's tag name
            s = ptr::null_mut();
            let mut b_iter = (*cptr).promiseargs;
            while !b_iter.is_null() && b_iter != R_NilValue() {
                let b_tag = TAG(b_iter);
                if !b_tag.is_null() && b_tag != R_NilValue() {
                    // Exact match: TAG(b_iter) equals tag
                    if b_tag == tag {
                        if !s.is_null() {
                            // multiple match error
                            s = CAR(b_iter);
                            break;
                        }
                        s = CAR(b_iter);
                    } else {
                        // Partial match: compare tag name prefix against TAG(b_iter)
                        let tag_name = PRINTNAME(tag);
                        let tag_name_c = translateChar(tag_name);
                        let b_tag_name = PRINTNAME(b_tag);
                        let b_tag_name_c = translateChar(b_tag_name);
                        if !tag_name_c.is_null() && !b_tag_name_c.is_null() {
                            let tlen = libc::strlen(tag_name_c);
                            if tlen > 0 {
                                let blen = libc::strlen(b_tag_name_c);
                                if blen >= tlen
                                    && libc::strncmp(b_tag_name_c, tag_name_c, tlen) == 0
                                {
                                    if !s.is_null() {
                                        s = CAR(b_iter); // ambiguous match
                                        break;
                                    }
                                    s = CAR(b_iter);
                                }
                            }
                        }
                    }
                }
                b_iter = CDR(b_iter);
            }

            if s.is_null() {
                // partial match
                let mut b_iter = (*cptr).promiseargs;
                while !b_iter.is_null() && b_iter != R_NilValue() {
                    let b_tag = TAG(b_iter);
                    if !b_tag.is_null() && b_tag != R_NilValue() && b_tag == tag {
                        s = CAR(b_iter);
                        break;
                    }
                    b_iter = CDR(b_iter);
                }
            }

            if s.is_null() {
                // first untagged argument
                let mut b_iter = (*cptr).promiseargs;
                while !b_iter.is_null() && b_iter != R_NilValue() {
                    let b_tag = TAG(b_iter);
                    if b_tag.is_null() || b_tag == R_NilValue() {
                        s = CAR(b_iter);
                        break;
                    }
                    b_iter = CDR(b_iter);
                }
            }

            if s.is_null() {
                let pa = (*cptr).promiseargs;
                if !pa.is_null() && pa != R_NilValue() {
                    s = CAR(pa);
                }
            }
        } else {
            let pa = (*cptr).promiseargs;
            if !pa.is_null() && pa != R_NilValue() {
                s = CAR(pa);
            }
        }

        if TYPEOF(s) == SEXPTYPE::PROMSXP {
            s = crate::sexp::envir::forcePromise(s);
        } else if !s.is_null() && s != R_NilValue() && s != R_MissingArg() {
            let eval_env = if (*cptr).sysparent.is_null() || (*cptr).sysparent == R_NilValue() {
                R_BaseEnv()
            } else {
                (*cptr).sysparent
            };
            s = Rf_eval(s, eval_env);
        }

        s
    }
}

// ---------------------------------------------------------------------------
// applyMethod -- apply a dispatched method
// ---------------------------------------------------------------------------

/// Apply a method (SPECIALSXP, BUILTINSXP, or CLOSXP) with given arguments.
/// Note: This is a simplified version. Full implementation requires eval infrastructure.
unsafe fn applyMethod(call: SEXP, op: SEXP, args: SEXP, rho: SEXP, newvars: SEXP) -> SEXP {
    unsafe {
        if op.is_null() || op == R_NilValue() {
            return R_NilValue();
        }

        let t = TYPEOF(op);
        if t == SEXPTYPE::SPECIALSXP {
            let primfun = crate::eval::builtin::PRIMFUN(op);
            if let Some(fn_ptr) = primfun {
                return fn_ptr(call, op, args, rho);
            }
        } else if t == SEXPTYPE::BUILTINSXP {
            let evald_args = crate::eval::dispatch::evalList(args, rho, call, 0);
            let primfun = crate::eval::builtin::PRIMFUN(op);
            if let Some(fn_ptr) = primfun {
                return fn_ptr(call, op, evald_args, rho);
            }
        } else if t == SEXPTYPE::CLOSXP {
            return crate::eval::closure::applyClosureWithFrameVars(
                call, op, args, rho, rho, newvars, 0,
            );
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// patchArgsByActuals -- 3-pass argument matching for NextMethod
// ---------------------------------------------------------------------------

/// Formal argument matching state for patchArgsByActuals.
/// Ported from match.c:415-422.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum fstype_t {
    Unmatched = 0,
    MatchedPresent = 1,
    MatchedMissing = 2,
    MatchedLocal = 3,
}

/// Patch a single supplied argument into a promise referencing the formal name.
/// If the supplied value is R_MissingArg, looks up a local variable in cloenv.
/// Ported from match.c:424-438.
unsafe fn patch_argument(supplied_slot: SEXP, name: SEXP, farg: *mut fstype_t, cloenv: SEXP) {
    unsafe {
        let value = CAR(supplied_slot);
        if value == R_MissingArg() {
            let local = crate::sexp::envir::R_findVarInFrame(cloenv, name);
            if local == R_MissingArg() {
                if !farg.is_null() {
                    *farg = fstype_t::MatchedMissing;
                }
                return;
            }
            if !farg.is_null() {
                *farg = fstype_t::MatchedLocal;
            }
        } else if !farg.is_null() {
            *farg = fstype_t::MatchedPresent;
        }
        SETCAR(
            supplied_slot,
            crate::sexp::memory_ext::mkPROMISE(name, cloenv),
        );
    }
}

/// 3-pass argument matching: exact tag, partial tag, positional.
/// Creates a shallow copy of supplied args and patches them into promises
/// referencing the closure environment's formals.
/// Ported from match.c:440-559.
unsafe fn patchArgsByActuals(formals: SEXP, supplied: SEXP, cloenv: SEXP) -> SEXP {
    unsafe {
        let nfarg = length(formals).max(1) as usize;
        let mut farg = vec![fstype_t::Unmatched; nfarg];

        // Shallow-duplicate supplied arguments
        let n_supplied = length(supplied);
        let prsupplied = crate::sexp::memory_ext::allocList(n_supplied);
        let _prsupplied_guard = protect(prsupplied);
        let mut b = supplied;
        let mut a = prsupplied;
        while !b.is_null() && b != R_NilValue() && !a.is_null() && a != R_NilValue() {
            SETCAR(a, CAR(b));
            // SET_ARGUSED(a, 0) — clear the argused flag
            let gp = (*a).sxpinfo.gp();
            (*a).sxpinfo.set_gp(gp & !2);
            SETTAG(a, TAG(b));
            b = CDR(b);
            a = CDR(a);
        }

        // Pass 1: exact matches by tag
        let mut f = formals;
        let mut farg_i = 0usize;
        while !f.is_null() && f != R_NilValue() {
            if TAG(f) != crate::sexp::symbol::R_DotsSymbol() {
                let mut b2 = prsupplied;
                while !b2.is_null() && b2 != R_NilValue() {
                    if !TAG(b2).is_null()
                        && TAG(b2) != R_NilValue()
                        && crate::mainutils::match_mod::pmatch(TAG(f), TAG(b2), 1) != 0
                    {
                        patch_argument(b2, TAG(f), &mut farg[farg_i], cloenv);
                        let gp = (*b2).sxpinfo.gp();
                        (*b2).sxpinfo.set_gp((gp & !2) | 2);
                        break;
                    }
                    b2 = CDR(b2);
                }
            }
            f = CDR(f);
            farg_i += 1;
        }

        // Pass 2: partial matches by tag
        let mut seendots = false;
        f = formals;
        farg_i = 0;
        while !f.is_null() && f != R_NilValue() {
            if farg[farg_i] == fstype_t::Unmatched {
                if TAG(f) == crate::sexp::symbol::R_DotsSymbol() && !seendots {
                    seendots = true;
                } else {
                    let mut b2 = prsupplied;
                    while !b2.is_null() && b2 != R_NilValue() {
                        // Check ARGUSED == 0
                        let gp = (*b2).sxpinfo.gp();
                        let argused = (gp & 2) != 0;
                        if !argused
                            && !TAG(b2).is_null()
                            && TAG(b2) != R_NilValue()
                            && crate::mainutils::match_mod::pmatch(
                                TAG(f),
                                TAG(b2),
                                if seendots { 1 } else { 0 },
                            ) != 0
                        {
                            patch_argument(b2, TAG(f), &mut farg[farg_i], cloenv);
                            // SET_ARGUSED(b2, 1)
                            let gp2 = (*b2).sxpinfo.gp();
                            (*b2).sxpinfo.set_gp((gp2 & !2) | 2);
                            break;
                        }
                        b2 = CDR(b2);
                    }
                }
            }
            f = CDR(f);
            farg_i += 1;
        }

        // Pass 3: positional matches
        f = formals;
        let mut b3 = prsupplied;
        farg_i = 0;
        while !f.is_null() && f != R_NilValue() && !b3.is_null() && b3 != R_NilValue() {
            if TAG(f) == crate::sexp::symbol::R_DotsSymbol() {
                break;
            } else if farg[farg_i] == fstype_t::MatchedPresent {
                f = CDR(f);
                farg_i += 1;
            } else {
                let gp = (*b3).sxpinfo.gp();
                let argused = (gp & 2) != 0;
                let has_tag = !TAG(b3).is_null() && TAG(b3) != R_NilValue();
                if argused || has_tag {
                    b3 = CDR(b3);
                } else {
                    if farg[farg_i] == fstype_t::MatchedLocal {
                        SETCAR(b3, R_MissingArg());
                    } else {
                        patch_argument(b3, TAG(f), ptr::null_mut(), cloenv);
                    }
                    // SET_ARGUSED(b3, 1)
                    let gp2 = (*b3).sxpinfo.gp();
                    (*b3).sxpinfo.set_gp((gp2 & !2) | 2);
                    b3 = CDR(b3);
                    f = CDR(f);
                    farg_i += 1;
                }
            }
        }

        prsupplied
    }
}

// ---------------------------------------------------------------------------
// newintoold -- destructive argument matching for NextMethod
// ---------------------------------------------------------------------------

/// Destructive matching of arguments: named elements of newargs replace
/// matching elements in oldargs; the two resulting lists are appended.
unsafe fn newintoold(new: SEXP, old: SEXP) -> SEXP {
    unsafe {
        if new.is_null() || new == R_NilValue() {
            return R_NilValue();
        }
        let rest = CDR(new);
        let result_rest = newintoold(rest, old);
        SETCDR(new, result_rest);

        let mut old_iter = old;
        while !old_iter.is_null() && old_iter != R_NilValue() {
            let old_tag = TAG(old_iter);
            if !old_tag.is_null() && old_tag != R_NilValue() && old_tag == TAG(new) {
                SETCAR(old_iter, CAR(new));
                return CDR(new);
            }
            old_iter = CDR(old_iter);
        }
        new
    }
}

/// Match method arguments: merge old and new argument lists.
unsafe fn matchmethargs(oldargs: SEXP, newargs: SEXP) -> SEXP {
    unsafe {
        let merged = newintoold(newargs, oldargs);
        listAppend(oldargs, merged)
    }
}

// ---------------------------------------------------------------------------
// fixcall -- fix up a call with additional tagged arguments
// ---------------------------------------------------------------------------

/// Fix up the call when arguments to the function may have changed.
/// For now we only worry about tagged args, appending them if they
/// are not already there.
unsafe fn fixcall(call: SEXP, args: SEXP) -> SEXP {
    unsafe {
        if call.is_null() || args.is_null() {
            return call;
        }
        let mut t = args;
        while !t.is_null() && t != R_NilValue() {
            let t_tag = TAG(t);
            if !t_tag.is_null() && t_tag != R_NilValue() {
                let mut found: c_int = FALSE;
                let mut s = call;
                while !s.is_null() && s != R_NilValue() {
                    let cdr_s = CDR(s);
                    if cdr_s.is_null() || cdr_s == R_NilValue() {
                        break;
                    }
                    if TAG(cdr_s) == t_tag {
                        found = TRUE;
                        break;
                    }
                    s = cdr_s;
                }
                if found == FALSE {
                    let new_elem = allocList(1);
                    SETTAG(new_elem, t_tag);
                    SETCAR(new_elem, CAR(t)); // lazy_duplicate would be ideal
                    SETCDR(s, new_elem);
                }
            }
            t = CDR(t);
        }
        call
    }
}

// ---------------------------------------------------------------------------
// findFunInEnvRange -- search for a function in an environment chain
// ---------------------------------------------------------------------------

/// Find a function in the environment chain from rho to target.
unsafe fn findFunInEnvRange(symbol: SEXP, rho: SEXP, target: SEXP) -> SEXP {
    unsafe {
        let mut current_rho = rho;
        while !current_rho.is_null() && current_rho != R_EmptyEnv() {
            let vl = crate::sexp::envir::R_findVarInFrame(current_rho, symbol);
            if vl != R_UnboundValue() {
                if TYPEOF(vl) == SEXPTYPE::PROMSXP {
                    // Would need to eval -- for now skip promise forcing
                }
                let t = TYPEOF(vl);
                if t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
                    return vl;
                }
            }
            if current_rho == target {
                return R_UnboundValue();
            }
            current_rho = ENCLOS(current_rho);
        }
        R_UnboundValue()
    }
}

/// Find a function, searching the global env, then base env.
unsafe fn findFunWithBaseEnvAfterGlobalEnv(symbol: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut current_rho = rho;
        while !current_rho.is_null() && current_rho != R_EmptyEnv() {
            let vl = crate::sexp::envir::R_findVarInFrame(current_rho, symbol);
            if vl != R_UnboundValue() {
                let t = TYPEOF(vl);
                if t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
                    return vl;
                }
            }
            if current_rho == R_GlobalEnv() {
                current_rho = R_BaseEnv();
            } else {
                current_rho = ENCLOS(current_rho);
            }
        }
        R_UnboundValue()
    }
}

// ---------------------------------------------------------------------------
// isBasicClass -- check if a class name is in the S3 methods table
// ---------------------------------------------------------------------------

/// Look up the class name in the methods package table of S3 classes.
/// Returns FALSE when methods package is not loaded.
pub unsafe fn isBasicClass(_ss: *const c_char) -> c_int {
    // Unimplemented: requires R methods package infrastructure
    // Full implementation would consult the methods namespace and S3 classes.
    FALSE
}

// ---------------------------------------------------------------------------
// R_has_methods_attached -- check if the methods package is fully attached
// ---------------------------------------------------------------------------

pub unsafe fn R_has_methods_attached() -> c_int {
    // Unimplemented: requires R methods package infrastructure
    unsafe {
        if isMethodsDispatchOn() == FALSE {
            return FALSE;
        }
        // Full implementation would check R_BindingIsLocked
        FALSE
    }
}

// ---------------------------------------------------------------------------
// addS3Var / createS3Vars -- create S3 dispatch environment variables
// ---------------------------------------------------------------------------

/// Prepend a named variable to the S3 dispatch variable list.
unsafe fn addS3Var(vars: SEXP, name: SEXP, value: SEXP) -> SEXP {
    unsafe {
        let res = Rf_cons(value, vars);
        SETTAG(res, name);
        res
    }
}

/// Create the full list of S3 dispatch variables:
/// .Generic, .Class, .Method, .GenericCallEnv, .GenericDefEnv, .Group
pub unsafe fn createS3Vars(
    dotGeneric: SEXP,
    dotGroup: SEXP,
    dotClass: SEXP,
    dotMethod: SEXP,
    dotGenericCallEnv: SEXP,
    dotGenericDefEnv: SEXP,
) -> SEXP {
    unsafe {
        let mut v = R_NilValue();
        v = addS3Var(v, sym(".GenericDefEnv"), dotGenericDefEnv);
        v = addS3Var(v, sym(".GenericCallEnv"), dotGenericCallEnv);
        v = addS3Var(v, sym(".Group"), dotGroup);
        v = addS3Var(v, sym(".Method"), dotMethod);
        v = addS3Var(v, sym(".Class"), dotClass);
        v = addS3Var(v, sym(".Generic"), dotGeneric);
        v
    }
}

