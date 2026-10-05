//! GNU `stats/src/deriv.c`: symbolic `D()` and `deriv()`.

use crate::sexp::accessors::{
    CADDR, CADR, CAR, CDR, INTEGER, LENGTH, REAL, SETCAR, SETCDR, STRING_ELT, TYPEOF,
};
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarReal, Rf_allocVector, Rf_cons, Rf_lang2, Rf_lang3, Rf_lang4,
    Rf_mkString,
};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use std::ffi::{CStr, CString};

fn ty(x: SEXP) -> SEXPTYPE {
    unsafe { std::mem::transmute::<i32, SEXPTYPE>(TYPEOF(x)) }
}

unsafe fn sym(name: &str) -> SEXP {
    unsafe { Rf_install(CString::new(name).unwrap_or_default().as_ptr()) }
}

unsafe fn constant(x: f64) -> SEXP {
    unsafe { Rf_ScalarReal(x) }
}

// Raw native boundaries retain the original runtime across every callback and
// unwind. The expression kernel itself owns checked children and result nodes.
#[path = "deriv/owned.rs"]
mod owned;

unsafe fn run_owned_derivative(
    first: SEXP,
    second: Option<SEXP>,
    operation: impl FnOnce(
        &crate::sexp::owner::RuntimeAccess,
        &crate::sexp::object::Sexp<'static>,
        Option<&crate::sexp::object::Sexp<'static>>,
    ) -> crate::sexp::object::SexpResult<crate::sexp::object::Sexp<'static>>,
) -> SEXP {
    use crate::sexp::{
        object::Sexp,
        owner::{OwnerToken, StoredOwner, with_runtime},
    };
    let fail = |message: String| -> ! {
        std::panic::panic_any(crate::sexp::context::RError { message });
    };
    let token = unsafe { OwnerToken::current() }.unwrap_or_else(|error| fail(error.to_string()));
    let authority = StoredOwner::from_token(token);
    let owner = authority
        .managed()
        .unwrap_or_else(|| fail("runtime owner unavailable".into()));
    let _pin = owner.pin().unwrap_or_else(|error| fail(error.to_string()));
    let first = token
        .sexp(first)
        .and_then(Sexp::into_owned)
        .unwrap_or_else(|error| fail(error.to_string()));
    let second = second
        .map(|value| token.sexp(value).and_then(Sexp::into_owned))
        .transpose()
        .unwrap_or_else(|error| fail(error.to_string()));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_runtime(&owner, |access| operation(access, &first, second.as_ref()))
    }));
    authority
        .require_active()
        .unwrap_or_else(|error| fail(error.to_string()));
    match result {
        Ok(result) => result
            .and_then(|result| result)
            .unwrap_or_else(|error| fail(error.to_string()))
            .as_raw(),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

fn derivative_symbol(
    access: &crate::sexp::owner::RuntimeAccess,
    name: &str,
) -> crate::sexp::object::SexpResult<crate::sexp::object::Sexp<'static>> {
    let name =
        CString::new(name).map_err(|_| crate::sexp::object::SexpError::EvaluationFailed {
            message: "invalid variable name".into(),
        })?;
    access.with_native(|owner| unsafe { owner.sexp(Rf_install(name.as_ptr()))?.into_owned() })
}

unsafe fn deriv_expr(expr: SEXP, var: SEXP) -> SEXP {
    unsafe {
        run_owned_derivative(expr, Some(var), |access, expression, variable| {
            let variable = variable.ok_or(crate::sexp::object::SexpError::RootUnavailable)?;
            owned::Kernel::new(access, derivative_symbol).derive(expression, variable)
        })
    }
}

unsafe fn is_form(expr: SEXP, op: &str, n: i32) -> bool {
    unsafe { ty(expr) == SEXPTYPE::LANGSXP && LENGTH(expr) == n && CAR(expr) == sym(op) }
}

unsafe fn add_parens(expr: SEXP) -> SEXP {
    unsafe {
        if ty(expr) == SEXPTYPE::LANGSXP {
            let mut e = CDR(expr);
            while !e.is_null() && e != R_NilValue() {
                SETCAR(e, add_parens(CAR(e)));
                e = CDR(e);
            }
        }
        let paren = sym("(");
        let wrap_second = |e: SEXP| SETCAR(CDR(CDR(e)), Rf_lang2(paren, CADDR(e)));
        let wrap_first = |e: SEXP| SETCAR(CDR(e), Rf_lang2(paren, CADR(e)));
        if is_form(expr, "+", 3) && is_form(CADDR(expr), "+", 3) {
            wrap_second(expr);
        } else if is_form(expr, "-", 3)
            && (is_form(CADDR(expr), "+", 3) || is_form(CADDR(expr), "-", 3))
        {
            wrap_second(expr);
        } else if is_form(expr, "*", 3) || is_form(expr, "/", 3) {
            if is_form(CADR(expr), "+", 3) || is_form(CADR(expr), "-", 3) {
                wrap_first(expr);
            }
            if is_form(CADDR(expr), "+", 3)
                || is_form(CADDR(expr), "-", 3)
                || is_form(CADDR(expr), "*", 3)
                || is_form(CADDR(expr), "/", 3)
            {
                wrap_second(expr);
            }
        } else if is_form(expr, "^", 3) && is_form(CADR(expr), "^", 3) {
            wrap_first(expr);
        }
        expr
    }
}

fn expr_of(given: SEXP) -> SEXP {
    unsafe {
        if ty(given) == SEXPTYPE::EXPRSXP {
            crate::sexp::accessors::VECTOR_ELT(given, 0)
        } else {
            given
        }
    }
}

fn string_at(s: SEXP, i: R_xlen_t) -> String {
    unsafe {
        CStr::from_ptr(crate::sexp::accessors::CHAR(STRING_ELT(s, i)))
            .to_str()
            .unwrap_or("")
            .to_string()
    }
}

/// `.External(C_doD, expr, name)`.
pub unsafe fn do_d(args: SEXP) -> SEXP {
    unsafe {
        run_owned_derivative(args, None, |access, arguments, _| {
            owned::direct(access, arguments, derivative_symbol, |access| {
                access.with_native(|_| {
                    crate::mainutils::errors::Rf_warning(
                        c"only the first element is used as variable name".as_ptr(),
                    );
                    Ok(())
                })
            })
        })
    }
}

unsafe fn equal(a: SEXP, b: SEXP) -> bool {
    unsafe {
        if ty(a) != ty(b) {
            return false;
        }
        match ty(a) {
            SEXPTYPE::NILSXP => true,
            SEXPTYPE::SYMSXP => a == b,
            SEXPTYPE::REALSXP => *REAL(a) == *REAL(b),
            SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => *INTEGER(a) == *INTEGER(b),
            SEXPTYPE::LANGSXP | SEXPTYPE::LISTSXP => equal(CAR(a), CAR(b)) && equal(CDR(a), CDR(b)),
            _ => false,
        }
    }
}

unsafe fn make_variable(k: i32, tag: &str) -> SEXP {
    unsafe { sym(&format!("{tag}{k}")) }
}

unsafe fn accumulate(expr: SEXP, exprlist: SEXP) -> i32 {
    unsafe {
        let mut e = exprlist;
        let mut k = 0;
        while CDR(e) != R_NilValue() {
            e = CDR(e);
            k += 1;
            if equal(expr, CAR(e)) {
                return k;
            }
        }
        SETCDR(e, Rf_cons(expr, R_NilValue()));
        k + 1
    }
}
unsafe fn accumulate_new(expr: SEXP, exprlist: SEXP) {
    unsafe {
        let mut e = exprlist;
        while CDR(e) != R_NilValue() {
            e = CDR(e);
        }
        SETCDR(e, Rf_cons(expr, R_NilValue()));
    }
}
unsafe fn find_subexprs(expr: SEXP, exprlist: SEXP, tag: &str) -> i32 {
    unsafe {
        match ty(expr) {
            SEXPTYPE::SYMSXP
            | SEXPTYPE::LGLSXP
            | SEXPTYPE::INTSXP
            | SEXPTYPE::REALSXP
            | SEXPTYPE::CPLXSXP => 0,
            SEXPTYPE::LANGSXP => {
                if CAR(expr) == sym("(") {
                    return find_subexprs(CADR(expr), exprlist, tag);
                }
                let mut e = CDR(expr);
                while !e.is_null() && e != R_NilValue() {
                    let k = find_subexprs(CAR(e), exprlist, tag);
                    if k != 0 {
                        SETCAR(e, make_variable(k, tag));
                    }
                    e = CDR(e);
                }
                accumulate(expr, exprlist)
            }
            _ => 0,
        }
    }
}

unsafe fn count_occurrences(symbol: SEXP, lst: SEXP) -> i32 {
    unsafe {
        match ty(lst) {
            SEXPTYPE::SYMSXP => i32::from(lst == symbol),
            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => {
                count_occurrences(symbol, CAR(lst)) + count_occurrences(symbol, CDR(lst))
            }
            _ => 0,
        }
    }
}

unsafe fn replace(symbol: SEXP, expr: SEXP, lst: SEXP) -> SEXP {
    unsafe {
        match ty(lst) {
            SEXPTYPE::SYMSXP => {
                if lst == symbol {
                    expr
                } else {
                    lst
                }
            }
            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => {
                SETCAR(lst, replace(symbol, expr, CAR(lst)));
                SETCDR(lst, replace(symbol, expr, CDR(lst)));
                lst
            }
            _ => lst,
        }
    }
}

unsafe fn create_grad(names: SEXP) -> SEXP {
    unsafe {
        let n = LENGTH(names);
        let mut name_args = R_NilValue();
        let mut names_call = sym("c");
        let mut args: Vec<SEXP> = Vec::new();
        for i in 0..n {
            args.push(Rf_mkString(crate::sexp::accessors::CHAR(STRING_ELT(
                names,
                i as R_xlen_t,
            ))));
        }
        let names_call = match args.len() {
            1 => Rf_lang2(sym("c"), args[0]),
            2 => Rf_lang3(sym("c"), args[0], args[1]),
            _ => Rf_lang3(sym("c"), args[0], args[1]),
        };
        let _ = names_call;
        let dimnames = Rf_lang3(
            sym("list"),
            R_NilValue(),
            if n == 2 {
                Rf_lang3(sym("c"), args[0], args[1])
            } else {
                Rf_lang2(sym("c"), args[0])
            },
        );
        let dim = Rf_lang3(
            sym("c"),
            Rf_lang2(sym("length"), sym(".value")),
            Rf_ScalarInteger(n),
        );
        Rf_lang3(
            sym("<-"),
            sym(".grad"),
            Rf_lang4(sym("array"), constant(0.0), dim, dimnames),
        )
    }
}

unsafe fn deriv_assign(name: SEXP, expr: SEXP) -> SEXP {
    unsafe {
        let bracket = Rf_lang4(
            sym("["),
            sym(".grad"),
            R_MissingArg(),
            Rf_mkString(crate::sexp::accessors::CHAR(name)),
        );
        Rf_lang3(sym("<-"), bracket, expr)
    }
}

unsafe fn add_grad() -> SEXP {
    unsafe {
        Rf_lang3(
            sym("<-"),
            Rf_lang3(
                sym("attr"),
                sym(".value"),
                Rf_mkString(c"gradient".as_ptr()),
            ),
            sym(".grad"),
        )
    }
}

unsafe fn prune(lst: SEXP) -> SEXP {
    unsafe {
        if lst == R_NilValue() {
            return lst;
        }
        SETCDR(lst, prune(CDR(lst)));
        if CAR(lst) == R_MissingArg() {
            CDR(lst)
        } else {
            lst
        }
    }
}

unsafe fn closure_with_formals(names: SEXP, body: SEXP) -> SEXP {
    unsafe {
        let n = LENGTH(names);
        let mut formals = R_NilValue();
        for i in (0..n).rev() {
            let cell = Rf_cons(R_MissingArg(), formals);
            crate::sexp::accessors::SETTAG(cell, sym(&string_at(names, i as R_xlen_t)));
            formals = cell;
        }
        crate::mainutils::dstruct::R_mkClosure(formals, body, crate::sexp::globals::R_GlobalEnv())
    }
}
/// `.External(C_deriv, expr, namevec, function.arg, tag, hessian)`.
pub unsafe fn do_deriv(args: SEXP) -> SEXP {
    unsafe {
        let mut args = CDR(args);
        let expr = expr_of(CAR(args));
        args = CDR(args);
        let names = CAR(args);
        let nderiv = LENGTH(names);
        args = CDR(args);
        let funarg = CAR(args);
        args = CDR(args);
        let tag = string_at(CAR(args), 0);
        let exprlist = Rf_lang2(sym("{"), R_NilValue());
        SETCDR(exprlist, R_NilValue());
        let duplicated = crate::mainutils::duplicate::Rf_duplicate(expr);
        let _el = protect(exprlist);
        let f_index = find_subexprs(duplicated, exprlist, &tag);
        let mut d_index = vec![0i32; nderiv as usize];
        for i in 0..nderiv {
            let var = sym(&string_at(names, i as R_xlen_t));
            let derived = deriv_expr(crate::mainutils::duplicate::Rf_duplicate(expr), var);
            d_index[i as usize] = find_subexprs(derived, exprlist, &tag);
        }
        let nexpr = LENGTH(exprlist) - 1;
        if f_index != 0 {
            accumulate_new(make_variable(f_index, &tag), exprlist);
        } else {
            accumulate_new(crate::mainutils::duplicate::Rf_duplicate(expr), exprlist);
        }
        accumulate_new(R_MissingArg(), exprlist);
        for i in 0..nderiv {
            if d_index[i as usize] != 0 {
                accumulate_new(make_variable(d_index[i as usize], &tag), exprlist);
            } else {
                let var = sym(&string_at(names, i as R_xlen_t));
                accumulate_new(
                    deriv_expr(crate::mainutils::duplicate::Rf_duplicate(expr), var),
                    exprlist,
                );
            }
        }
        accumulate_new(R_MissingArg(), exprlist);
        accumulate_new(R_MissingArg(), exprlist);
        accumulate_new(R_MissingArg(), exprlist);
        let mut i = 0;
        let mut ans = CDR(exprlist);
        while i < nexpr {
            let variable = make_variable(i + 1, &tag);
            if count_occurrences(variable, CDR(ans)) < 2 {
                SETCDR(ans, replace(variable, CAR(ans), CDR(ans)));
                SETCAR(ans, R_MissingArg());
            } else {
                SETCAR(ans, Rf_lang3(sym("<-"), variable, add_parens(CAR(ans))));
            }
            i += 1;
            ans = CDR(ans);
        }
        SETCAR(
            ans,
            Rf_lang3(sym("<-"), sym(".value"), add_parens(CAR(ans))),
        );
        ans = CDR(ans);
        SETCAR(ans, create_grad(names));
        ans = CDR(ans);
        for i in 0..nderiv {
            SETCAR(
                ans,
                deriv_assign(STRING_ELT(names, i as R_xlen_t), add_parens(CAR(ans))),
            );
            ans = CDR(ans);
        }
        SETCAR(ans, add_grad());
        ans = CDR(ans);
        SETCAR(ans, sym(".value"));
        SETCDR(exprlist, prune(CDR(exprlist)));
        let body = CDR(exprlist);
        let call = Rf_lang2(sym("{"), CAR(body));
        SETCDR(call, body);
        if ty(funarg) == SEXPTYPE::LGLSXP && *crate::sexp::accessors::LOGICAL(funarg) != 0 {
            return closure_with_formals(names, call);
        }
        if ty(funarg) == SEXPTYPE::CLOSXP {
            let formals = crate::sexp::accessors::FORMALS(funarg);
            let rho = crate::sexp::accessors::CLOENV(funarg);
            return crate::mainutils::dstruct::R_mkClosure(formals, call, rho);
        }
        let result = Rf_allocVector(SEXPTYPE::EXPRSXP, 1);
        crate::sexp::accessors::SET_VECTOR_ELT(result, 0, call);
        result
    }
}

#[cfg(test)]
#[path = "deriv/owned_tests.rs"]
mod owned_tests;
