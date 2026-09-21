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

unsafe fn is_numeric_const(s: SEXP) -> bool {
    unsafe {
        matches!(ty(s), SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP) && LENGTH(s) >= 1
    }
}

unsafe fn as_real(s: SEXP) -> f64 {
    unsafe {
        match ty(s) {
            SEXPTYPE::REALSXP => *REAL(s),
            SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => *INTEGER(s) as f64,
            _ => f64::NAN,
        }
    }
}

unsafe fn is_zero(s: SEXP) -> bool {
    unsafe { is_numeric_const(s) && as_real(s) == 0.0 }
}

unsafe fn is_one(s: SEXP) -> bool {
    unsafe { is_numeric_const(s) && as_real(s) == 1.0 }
}

unsafe fn is_uminus(s: SEXP) -> bool {
    unsafe { ty(s) == SEXPTYPE::LANGSXP && CAR(s) == sym("-") && LENGTH(s) == 2 }
}

unsafe fn simplify(fun: SEXP, arg1: SEXP, arg2: SEXP) -> SEXP {
    unsafe {
        let plus = sym("+");
        let minus = sym("-");
        let times = sym("*");
        let divide = sym("/");
        let power = sym("^");
        if fun == plus {
            if is_zero(arg1) {
                arg2
            } else if is_zero(arg2) {
                arg1
            } else if is_uminus(arg1) {
                simplify(minus, arg2, CADR(arg1))
            } else if is_uminus(arg2) {
                simplify(minus, arg1, CADR(arg2))
            } else {
                Rf_lang3(plus, arg1, arg2)
            }
        } else if fun == minus {
            if arg2 == R_MissingArg() {
                if is_zero(arg1) {
                    constant(0.0)
                } else if is_uminus(arg1) {
                    CADR(arg1)
                } else {
                    Rf_lang2(minus, arg1)
                }
            } else if is_zero(arg2) {
                arg1
            } else if is_zero(arg1) {
                simplify(minus, arg2, R_MissingArg())
            } else if is_uminus(arg2) {
                simplify(plus, arg1, CADR(arg2))
            } else {
                Rf_lang3(minus, arg1, arg2)
            }
        } else if fun == times {
            if is_zero(arg1) || is_zero(arg2) {
                constant(0.0)
            } else if is_one(arg1) {
                arg2
            } else if is_one(arg2) {
                arg1
            } else if is_uminus(arg1) {
                simplify(minus, simplify(times, CADR(arg1), arg2), R_MissingArg())
            } else if is_uminus(arg2) {
                simplify(minus, simplify(times, arg1, CADR(arg2)), R_MissingArg())
            } else {
                Rf_lang3(times, arg1, arg2)
            }
        } else if fun == divide {
            if is_zero(arg1) {
                constant(0.0)
            } else if is_one(arg2) {
                arg1
            } else {
                Rf_lang3(divide, arg1, arg2)
            }
        } else if fun == power {
            if is_zero(arg2) {
                constant(1.0)
            } else if is_one(arg2) {
                arg1
            } else {
                Rf_lang3(power, arg1, arg2)
            }
        } else if arg2 == R_MissingArg() {
            Rf_lang2(fun, arg1)
        } else {
            Rf_lang3(fun, arg1, arg2)
        }
    }
}

unsafe fn deriv_expr(expr: SEXP, var: SEXP) -> SEXP {
    unsafe {
        match ty(expr) {
            SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP | SEXPTYPE::REALSXP | SEXPTYPE::CPLXSXP => {
                constant(0.0)
            }
            SEXPTYPE::SYMSXP => {
                if expr == var { constant(1.0) } else { constant(0.0) }
            }
            SEXPTYPE::LANGSXP => {
                let head = CAR(expr);
                let a = CADR(expr);
                let b = if LENGTH(expr) >= 3 { CADDR(expr) } else { R_MissingArg() };
                if head == sym("(") {
                    deriv_expr(a, var)
                } else if head == sym("+") {
                    if LENGTH(expr) == 2 {
                        deriv_expr(a, var)
                    } else {
                        simplify(sym("+"), deriv_expr(a, var), deriv_expr(b, var))
                    }
                } else if head == sym("-") {
                    if LENGTH(expr) == 2 {
                        simplify(sym("-"), deriv_expr(a, var), R_MissingArg())
                    } else {
                        simplify(sym("-"), deriv_expr(a, var), deriv_expr(b, var))
                    }
                } else if head == sym("*") {
                    simplify(
                        sym("+"),
                        simplify(sym("*"), deriv_expr(a, var), b),
                        simplify(sym("*"), a, deriv_expr(b, var)),
                    )
                } else if head == sym("/") {
                    simplify(
                        sym("-"),
                        simplify(sym("/"), deriv_expr(a, var), b),
                        simplify(
                            sym("/"),
                            simplify(sym("*"), a, deriv_expr(b, var)),
                            simplify(sym("^"), b, constant(2.0)),
                        ),
                    )
                } else if head == sym("^") && is_numeric_const(b) {
                    simplify(
                        sym("*"),
                        b,
                        simplify(
                            sym("*"),
                            deriv_expr(a, var),
                            simplify(sym("^"), a, constant(as_real(b) - 1.0)),
                        ),
                    )
                } else if head == sym("gamma") {
                    simplify(
                        sym("*"),
                        deriv_expr(a, var),
                        simplify(sym("*"), expr, simplify(sym("digamma"), a, R_MissingArg())),
                    )
                } else if head == sym("lgamma") {
                    simplify(sym("*"), deriv_expr(a, var), simplify(sym("digamma"), a, R_MissingArg()))
                } else if head == sym("digamma") {
                    simplify(sym("*"), deriv_expr(a, var), simplify(sym("trigamma"), a, R_MissingArg()))
                } else if head == sym("trigamma") {
                    simplify(
                        sym("*"),
                        deriv_expr(a, var),
                        Rf_lang3(sym("psigamma"), a, Rf_ScalarInteger(2)),
                    )
                } else if head == sym("psigamma") {
                    let order = if b == R_MissingArg() { Rf_ScalarInteger(1) } else { b };
                    let next = if is_numeric_const(order) {
                        Rf_ScalarInteger(as_real(order) as i32 + 1)
                    } else {
                        Rf_lang3(sym("+"), order, Rf_ScalarInteger(1))
                    };
                    simplify(sym("*"), deriv_expr(a, var), Rf_lang3(sym("psigamma"), a, next))
                } else if head == sym("sin") {
                    simplify(sym("*"), simplify(sym("cos"), a, R_MissingArg()), deriv_expr(a, var))
                } else if head == sym("cos") {
                    simplify(
                        sym("*"),
                        simplify(sym("sin"), a, R_MissingArg()),
                        simplify(sym("-"), deriv_expr(a, var), R_MissingArg()),
                    )
                } else if head == sym("exp") {
                    simplify(sym("*"), expr, deriv_expr(a, var))
                } else if head == sym("log") {
                    simplify(sym("/"), deriv_expr(a, var), a)
                } else if head == sym("sqrt") {
                    deriv_expr(Rf_lang3(sym("^"), a, constant(0.5)), var)
                } else {
                    crate::mainutils::errors::errorcall_str(
                        crate::mainutils::errors::R_getCurrentCall(),
                        "Function is not in the derivatives table",
                    );
                    constant(f64::NAN)
                }
            }
            _ => constant(f64::NAN),
        }
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
        let args = CDR(args);
        let var = sym(&string_at(CADR(args), 0));
        let derived = deriv_expr(expr_of(CAR(args)), var);
        let _d = protect(derived);
        add_parens(crate::mainutils::duplicate::Rf_duplicate(derived))
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
            SEXPTYPE::SYMSXP | SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP | SEXPTYPE::REALSXP
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
                if lst == symbol { expr } else { lst }
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
        let dimnames = Rf_lang3(sym("list"), R_NilValue(), 
            if n == 2 { Rf_lang3(sym("c"), args[0], args[1]) } else { Rf_lang2(sym("c"), args[0]) });
        let dim = Rf_lang3(sym("c"), Rf_lang2(sym("length"), sym(".value")), Rf_ScalarInteger(n));
        Rf_lang3(sym("<-"), sym(".grad"), Rf_lang4(sym("array"), constant(0.0), dim, dimnames))
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
            Rf_lang3(sym("attr"), sym(".value"), Rf_mkString(c"gradient".as_ptr())),
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
        if CAR(lst) == R_MissingArg() { CDR(lst) } else { lst }
    }
}

unsafe fn closure_with_formals(names: SEXP, body: SEXP) -> SEXP {
    unsafe {
        let n = LENGTH(names);
        let mut formals = R_NilValue();
        for i in (0..n).rev() {
            let cell = Rf_cons(R_MissingArg(), formals);
            crate::sexp::accessors::SETTAG(
                cell,
                sym(&string_at(names, i as R_xlen_t)),
            );
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
        SETCAR(ans, Rf_lang3(sym("<-"), sym(".value"), add_parens(CAR(ans))));
        ans = CDR(ans);
        SETCAR(ans, create_grad(names));
        ans = CDR(ans);
        for i in 0..nderiv {
            SETCAR(ans, deriv_assign(STRING_ELT(names, i as R_xlen_t), add_parens(CAR(ans))));
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
