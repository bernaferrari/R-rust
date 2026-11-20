//! Rooted default anyNA traversal. Recursive mode belongs to the traversal,
//! rather than to an ephemeral child argument list.
use crate::sexp::{
    ffi::{NA_INTEGER, SEXP, SEXPTYPE},
    object::{Sexp, SexpMut},
    owner::OwnerToken,
};

pub fn any_na_impl(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> bool {
    let owner = unsafe { OwnerToken::current() }.expect("active anyNA owner");
    let pin = owner.pin().expect("live anyNA owner");
    let f = owner.node_factory();
    let args = f.wrap(args).expect("anyNA argument owner");
    let call_value = (!call.is_null()).then(|| f.wrap(call).expect("anyNA call owner"));
    let _op = (!op.is_null()).then(|| f.wrap(op).expect("anyNA operation owner"));
    let env_value = (!env.is_null()).then(|| f.wrap(env).expect("anyNA environment owner"));
    let check = || {
        if let Some(pin) = &pin {
            pin.require_live().expect("live anyNA owner after callback");
        }
        f.require_active()
            .expect("original anyNA owner after callback");
    };
    let x = args.try_car().expect("anyNA input");
    let tail = args.try_cdr().expect("anyNA argument tail");
    let recursive = if is_list(&x) && !tail.is_nil() {
        let flag = tail.try_car().expect("anyNA recursive flag");
        let result = unsafe { super::asRbool(flag.as_raw(), call) != 0 };
        check();
        result
    } else {
        false
    };
    let dispatch = call_value
        .as_ref()
        .filter(|call| !call.is_nil())
        .and_then(|call| {
            env_value
                .as_ref()
                .filter(|env| !env.is_nil())
                .map(|_| NestedDispatch {
                    call,
                    op,
                    args: &args,
                    env,
                })
        });
    let result = scan(&x, recursive, env, &owner, dispatch, &check);
    check();
    result
}

fn is_list(x: &Sexp<'_>) -> bool {
    matches!(x.typeof_(), SEXPTYPE::VECSXP | SEXPTYPE::LISTSXP)
}

fn leaf(x: &Sexp<'_>, recursive: bool, env: SEXP, check: &impl Fn()) -> bool {
    if x.typeof_() == SEXPTYPE::LISTSXP && (!recursive || x.is_object()) {
        // is.na's shallow pairlist fallback must not receive a cyclic spine.
        work::validate_pairlist(x);
    }
    if x.is_object() || (is_list(x) && !recursive) {
        // Preserve the default method's any(is.na(x)) fallback. The owning
        // input and expression guards survive arbitrary is.na methods and GC.
        unsafe {
            let is_na = crate::sexp::symbol::Rf_install(c"is.na".as_ptr());
            let any = crate::sexp::symbol::Rf_install(c"any".as_ptr());
            let e0 = crate::sexp::constructors::Rf_lang2(is_na, x.as_raw());
            let _e0 = crate::sexp::protect::protect(e0);
            let e = crate::sexp::constructors::Rf_lang2(any, e0);
            let _e = crate::sexp::protect::protect(e);
            let result = crate::eval::eval::Rf_eval(e, env);
            check();
            let _result = crate::sexp::protect::protect(result);
            // GNU treats an NA answer from any(is.na(x)) as FALSE.
            let result = super::asLogical(result) == 1;
            check();
            return result;
        }
    }
    let kind = x.typeof_();
    if !matches!(
        kind,
        SEXPTYPE::REALSXP
            | SEXPTYPE::INTSXP
            | SEXPTYPE::LGLSXP
            | SEXPTYPE::CPLXSXP
            | SEXPTYPE::STRSXP
    ) {
        if matches!(kind, SEXPTYPE::NILSXP | SEXPTYPE::RAWSXP) {
            return false;
        }
        let name = unsafe {
            std::ffi::CStr::from_ptr(crate::mainutils::util_main::type2char(kind.into()))
        }
        .to_string_lossy();
        crate::sexp::context::r_error(format!(
            "anyNA() applied to non-(list or vector) of type '{name}'"
        ));
    }
    for i in 0..x.len() {
        let found = match kind {
            SEXPTYPE::REALSXP => x.try_real_elt(i).expect("anyNA real element").is_nan(),
            SEXPTYPE::INTSXP => x.try_integer_elt(i).expect("anyNA integer element") == NA_INTEGER,
            SEXPTYPE::LGLSXP => x.try_logical_elt(i).expect("anyNA logical element") == NA_INTEGER,
            SEXPTYPE::CPLXSXP => {
                let v = x.try_complex_elt(i).expect("anyNA complex element");
                v.r.is_nan() || v.i.is_nan()
            }
            SEXPTYPE::STRSXP => {
                x.try_string_elt(i).expect("anyNA string element").as_raw()
                    == crate::sexp::globals::R_NaString()
            }
            _ => false,
        };
        check();
        if found {
            return true;
        }
    }
    false
}

fn scan<'s>(
    x: &Sexp<'s>,
    recursive: bool,
    env: SEXP,
    owner: &OwnerToken<'_>,
    dispatch: Option<NestedDispatch<'_, 's>>,
    check: &impl Fn(),
) -> bool {
    if !recursive || !is_list(x) || x.is_object() {
        return leaf(x, recursive, env, check);
    }
    let mut stack = work::Stack::new(owner);
    stack.push(x.clone());
    while let Some(value) = stack.next(check) {
        if dispatch
            .as_ref()
            .is_some_and(|dispatch| dispatch.test(&value, check))
        {
            return true;
        }
        if is_list(&value) && !value.is_object() {
            stack.push(value);
        } else if leaf(&value, true, env, check) {
            return true;
        }
    }
    false
}

struct NestedDispatch<'a, 's> {
    call: &'a Sexp<'s>,
    op: SEXP,
    args: &'a Sexp<'s>,
    env: SEXP,
}

impl NestedDispatch<'_, '_> {
    fn test(&self, value: &Sexp<'_>, check: &impl Fn()) -> bool {
        if !value.is_object() && unsafe { super::IS_S4_OBJECT(value.as_raw()) } == 0 {
            return false;
        }
        let factory = self
            .args
            .node_factory()
            .expect("anyNA nested dispatch owner");
        let copy = |value: &Sexp<'_>| {
            let raw = unsafe { crate::mainutils::duplicate::shallow_duplicate(value.as_raw()) };
            check();
            factory.wrap(raw).expect("anyNA nested dispatch copy")
        };
        let args = copy(self.args);
        let call = copy(self.call);
        let mut args = SexpMut::try_from_checked(args).expect("anyNA nested arguments");
        args.try_set_pairlist_car(value)
            .expect("anyNA nested value");
        let args = args.freeze();
        {
            let call_args = call.try_cdr().expect("anyNA nested call arguments");
            let mut call_args = SexpMut::try_from_checked(call_args).expect("anyNA nested call");
            call_args
                .try_set_pairlist_car(value)
                .expect("anyNA nested call value");
        }

        let mut answer = crate::sexp::globals::R_NilValue();
        let dispatched = unsafe {
            crate::eval::dispatch::DispatchOrEval(
                call.as_raw(),
                self.op,
                c"anyNA".as_ptr(),
                args.as_raw(),
                self.env,
                &mut answer,
                0,
                1,
            ) != 0
        };
        check();
        if !dispatched {
            return false;
        }
        let answer = factory.wrap(answer).expect("anyNA nested dispatch result");
        // GNU's recursive dispatch checks the integer result of asLogical.
        // FALSE falls through to the default traversal and NA short-circuits.
        let found = unsafe { super::asLogical(answer.as_raw()) } != 0;
        check();
        found
    }
}

mod work;

#[cfg(test)]
mod tests;
