/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1998-2025   The R Core Team.
 *  Copyright (C) 2004-2017   The R Foundation
 *  Copyright (C) 1995, 1996  Robert Gentleman and Ross Ihaka
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/
 *
 *
 *  Symbolic Differentiation
 *
 *  Ported from r-source/src/library/stats/src/deriv.c
 */

use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::memory_ext::{R_alloc, allocLang, vmaxget, vmaxset};
use crate::sexp::protect::{Rf_protect, Rf_unprotect, protect};
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Re-exports of functions defined elsewhere
// ---------------------------------------------------------------------------

use crate::attrib_core::{getAttrib, setAttrib};

// ---------------------------------------------------------------------------
// Local helper functions
// ---------------------------------------------------------------------------

unsafe fn asReal(x: SEXP) -> c_double {
    crate::main::coerce::asReal(x)
}

unsafe fn asInteger(x: SEXP) -> c_int {
    crate::main::coerce::asInteger(x)
}

unsafe fn asLogical(x: SEXP) -> c_int {
    crate::main::coerce::asLogical(x)
}

unsafe fn length(x: SEXP) -> c_int {
    Rf_length(x)
}

unsafe fn isLogical(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::LGLSXP
}

unsafe fn isNumeric(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::INTSXP || TYPEOF(x) == SEXPTYPE::REALSXP
}

unsafe fn isComplex(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::CPLXSXP
}

unsafe fn isString(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::STRSXP
}

unsafe fn isLanguage(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::LANGSXP
}

unsafe fn isSymbol(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::SYMSXP
}

unsafe fn isExpression(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::EXPRSXP
}

unsafe fn isNull(x: SEXP) -> bool {
    x == R_NilValue()
}

unsafe fn duplicate(x: SEXP) -> SEXP {
    crate::mainutils::duplicate::duplicate(x)
}

unsafe fn deparse1(call: SEXP, abbrev: bool, opts: c_int) -> SEXP {
    crate::mainutils::deparse::deparse1(call, abbrev, opts)
}

unsafe fn translateChar_local(x: SEXP) -> *const c_char {
    crate::sexp::accessors::translateChar(x)
}

unsafe fn installTrChar(input: SEXP) -> SEXP {
    let c = CHAR(input);
    if c.is_null() {
        return R_NilValue();
    }
    Rf_install(c)
}

unsafe fn R_mkClosure(formals: SEXP, body: SEXP, rho: SEXP) -> SEXP {
    crate::mainutils::dstruct::R_mkClosure(formals, body, rho)
}

unsafe fn error(msg: &str) -> ! {
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    crate::main::errors::Rf_error(c_msg.as_ptr());
    unreachable!()
}

unsafe fn warning(msg: &str) {
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    crate::main::errors::Rf_warning(c_msg.as_ptr());
}

// ---------------------------------------------------------------------------
// Local SETCADR / SETCADDR / SETCADDDR helpers
// ---------------------------------------------------------------------------

unsafe fn SETCADR(x: SEXP, y: SEXP) {
    if !x.is_null() {
        let cdr = CDR(x);
        if !cdr.is_null() {
            SETCAR(cdr, y);
        }
    }
}

unsafe fn SETCADDR(x: SEXP, y: SEXP) {
    if !x.is_null() {
        let cddr = CDR(CDR(x));
        if !cddr.is_null() {
            SETCAR(cddr, y);
        }
    }
}

unsafe fn SETCADDDR(x: SEXP, y: SEXP) {
    if !x.is_null() {
        let cdddr = CDR(CDR(CDR(x)));
        if !cdddr.is_null() {
            SETCAR(cdddr, y);
        }
    }
}

// ---------------------------------------------------------------------------
// Local lang2, lang3, lang4, lang5, LCONS helpers
// ---------------------------------------------------------------------------

unsafe fn lang2(a: SEXP, b: SEXP) -> SEXP {
    let b_cell = crate::sexp::constructors::Rf_cons(b, R_NilValue());
    let cell = crate::sexp::constructors::Rf_cons(a, b_cell);
    if !cell.is_null() {
        (*cell).sxpinfo.set_type(SEXPTYPE::LANGSXP);
    }
    cell
}

unsafe fn lang3(a: SEXP, b: SEXP, c: SEXP) -> SEXP {
    let c_cell = crate::sexp::constructors::Rf_cons(c, R_NilValue());
    let b_cell = crate::sexp::constructors::Rf_cons(b, c_cell);
    let cell = crate::sexp::constructors::Rf_cons(a, b_cell);
    if !cell.is_null() {
        (*cell).sxpinfo.set_type(SEXPTYPE::LANGSXP);
    }
    cell
}

unsafe fn lang4(a: SEXP, b: SEXP, c: SEXP, d: SEXP) -> SEXP {
    let d_cell = crate::sexp::constructors::Rf_cons(d, R_NilValue());
    let c_cell = crate::sexp::constructors::Rf_cons(c, d_cell);
    let b_cell = crate::sexp::constructors::Rf_cons(b, c_cell);
    let cell = crate::sexp::constructors::Rf_cons(a, b_cell);
    if !cell.is_null() {
        (*cell).sxpinfo.set_type(SEXPTYPE::LANGSXP);
    }
    cell
}

unsafe fn lang5(a: SEXP, b: SEXP, c: SEXP, d: SEXP, e: SEXP) -> SEXP {
    let e_cell = crate::sexp::constructors::Rf_cons(e, R_NilValue());
    let d_cell = crate::sexp::constructors::Rf_cons(d, e_cell);
    let c_cell = crate::sexp::constructors::Rf_cons(c, d_cell);
    let b_cell = crate::sexp::constructors::Rf_cons(b, c_cell);
    let cell = crate::sexp::constructors::Rf_cons(a, b_cell);
    if !cell.is_null() {
        (*cell).sxpinfo.set_type(SEXPTYPE::LANGSXP);
    }
    cell
}

unsafe fn LCONS(car: SEXP, cdr: SEXP) -> SEXP {
    let cell = crate::sexp::constructors::Rf_cons(car, cdr);
    if !cell.is_null() {
        (*cell).sxpinfo.set_type(SEXPTYPE::LANGSXP);
    }
    cell
}

/// Local install from &str helper
unsafe fn install_str(name: &str) -> SEXP {
    let c_name = std::ffi::CString::new(name).unwrap_or_default();
    Rf_install(c_name.as_ptr())
}

/// inherits check (simplified: check "expression" class)
unsafe fn inherits(x: SEXP, class_name: &str) -> bool {
    if x.is_null() {
        return false;
    }
    let class_attr = getAttrib(x, install_str("class"));
    if isNull(class_attr) {
        return false;
    }
    let class_str = crate::attrib_core::R_ClassSymbol();
    let cls = getAttrib(x, class_str);
    if isNull(cls) {
        return false;
    }
    let n = Rf_length(cls);
    for i in 0..n as i64 {
        let elt = STRING_ELT(cls, i);
        if !elt.is_null() {
            let c = CHAR(elt);
            if !c.is_null() {
                let s = std::ffi::CStr::from_ptr(c);
                if let Ok(ss) = s.to_str() {
                    if ss == class_name {
                        return true;
                    }
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Per-session derivative symbols
// ---------------------------------------------------------------------------

unsafe fn deriv_symbol(name: &str) -> SEXP {
    if let Some(symbol) = crate::sexp::instance::with_current_instance(|inst| {
        inst.stats_deriv_symbols.get(name).copied()
    })
    .flatten()
    {
        return symbol;
    }

    let c = std::ffi::CString::new(name).unwrap_or_default();
    let symbol = Rf_install(c.as_ptr());
    crate::sexp::instance::with_required_current_instance(|inst| {
        *inst
            .stats_deriv_symbols
            .entry(name.to_string())
            .or_insert(symbol)
    })
}

unsafe fn ParenSymbol() -> SEXP {
    deriv_symbol("(")
}
unsafe fn PlusSymbol() -> SEXP {
    deriv_symbol("+")
}
unsafe fn MinusSymbol() -> SEXP {
    deriv_symbol("-")
}
unsafe fn TimesSymbol() -> SEXP {
    deriv_symbol("*")
}
unsafe fn DivideSymbol() -> SEXP {
    deriv_symbol("/")
}
unsafe fn PowerSymbol() -> SEXP {
    deriv_symbol("^")
}
unsafe fn ExpSymbol() -> SEXP {
    deriv_symbol("exp")
}
unsafe fn LogSymbol() -> SEXP {
    deriv_symbol("log")
}
unsafe fn SinSymbol() -> SEXP {
    deriv_symbol("sin")
}
unsafe fn CosSymbol() -> SEXP {
    deriv_symbol("cos")
}
unsafe fn TanSymbol() -> SEXP {
    deriv_symbol("tan")
}
unsafe fn SinhSymbol() -> SEXP {
    deriv_symbol("sinh")
}
unsafe fn CoshSymbol() -> SEXP {
    deriv_symbol("cosh")
}
unsafe fn TanhSymbol() -> SEXP {
    deriv_symbol("tanh")
}
unsafe fn SqrtSymbol() -> SEXP {
    deriv_symbol("sqrt")
}
unsafe fn PnormSymbol() -> SEXP {
    deriv_symbol("pnorm")
}
unsafe fn DnormSymbol() -> SEXP {
    deriv_symbol("dnorm")
}
unsafe fn AsinSymbol() -> SEXP {
    deriv_symbol("asin")
}
unsafe fn AcosSymbol() -> SEXP {
    deriv_symbol("acos")
}
unsafe fn AtanSymbol() -> SEXP {
    deriv_symbol("atan")
}
unsafe fn GammaSymbol() -> SEXP {
    deriv_symbol("gamma")
}
unsafe fn LGammaSymbol() -> SEXP {
    deriv_symbol("lgamma")
}
unsafe fn DiGammaSymbol() -> SEXP {
    deriv_symbol("digamma")
}
unsafe fn TriGammaSymbol() -> SEXP {
    deriv_symbol("trigamma")
}
unsafe fn PsiSymbol() -> SEXP {
    deriv_symbol("psigamma")
}
unsafe fn PiSymbol() -> SEXP {
    deriv_symbol("pi")
}
unsafe fn ExpM1Symbol() -> SEXP {
    deriv_symbol("expm1")
}
unsafe fn Log1PSymbol() -> SEXP {
    deriv_symbol("log1p")
}
unsafe fn Log2Symbol() -> SEXP {
    deriv_symbol("log2")
}
unsafe fn Log10Symbol() -> SEXP {
    deriv_symbol("log10")
}
unsafe fn SinPiSymbol() -> SEXP {
    deriv_symbol("sinpi")
}
unsafe fn CosPiSymbol() -> SEXP {
    deriv_symbol("cospi")
}
unsafe fn TanPiSymbol() -> SEXP {
    deriv_symbol("tanpi")
}
unsafe fn FactorialSymbol() -> SEXP {
    deriv_symbol("factorial")
}
unsafe fn LFactorialSymbol() -> SEXP {
    deriv_symbol("lfactorial")
}

// ---------------------------------------------------------------------------
// InitDerivSymbols -- no-op, symbols auto-init per RInstance
// ---------------------------------------------------------------------------

/// Initialize derivative symbols. In the Rust port, symbols are lazily
/// initialized in the active `RInstance`, so this is a no-op kept for API
/// compatibility.
#[allow(dead_code)]
unsafe fn InitDerivSymbols() {
    // Touch all symbols to ensure they're initialized
    let _ = ParenSymbol();
    let _ = PlusSymbol();
    let _ = MinusSymbol();
    let _ = TimesSymbol();
    let _ = DivideSymbol();
    let _ = PowerSymbol();
    let _ = ExpSymbol();
    let _ = LogSymbol();
    let _ = SinSymbol();
    let _ = CosSymbol();
    let _ = TanSymbol();
    let _ = SinhSymbol();
    let _ = CoshSymbol();
    let _ = TanhSymbol();
    let _ = SqrtSymbol();
    let _ = PnormSymbol();
    let _ = DnormSymbol();
    let _ = AsinSymbol();
    let _ = AcosSymbol();
    let _ = AtanSymbol();
    let _ = GammaSymbol();
    let _ = LGammaSymbol();
    let _ = DiGammaSymbol();
    let _ = TriGammaSymbol();
    let _ = PsiSymbol();
    let _ = PiSymbol();
    let _ = ExpM1Symbol();
    let _ = Log1PSymbol();
    let _ = Log2Symbol();
    let _ = Log10Symbol();
    let _ = SinPiSymbol();
    let _ = CosPiSymbol();
    let _ = TanPiSymbol();
    let _ = FactorialSymbol();
    let _ = LFactorialSymbol();
}

// ---------------------------------------------------------------------------
// Constant, isZero, isOne, isUminus
// ---------------------------------------------------------------------------

unsafe fn Constant(x: c_double) -> SEXP {
    Rf_ScalarReal(x)
}

unsafe fn isZero(s: SEXP) -> bool {
    asReal(s) == 0.0
}

unsafe fn isOne(s: SEXP) -> bool {
    asReal(s) == 1.0
}

unsafe fn isUminus(s: SEXP) -> bool {
    if TYPEOF(s) == SEXPTYPE::LANGSXP && CAR(s) == MinusSymbol() {
        match length(s) {
            2 => true,
            3 => CADDR(s) == R_MissingArg(),
            _ => {
                error("invalid form in unary minus check");
            }
        }
    } else {
        false
    }
}

/// Pointer protect and return the argument
unsafe fn PP(s: SEXP) -> SEXP {
    Rf_protect(s);
    s
}

// ---------------------------------------------------------------------------
// simplify
// ---------------------------------------------------------------------------

unsafe fn simplify(fun: SEXP, arg1: SEXP, arg2: SEXP) -> SEXP {
    let ans: SEXP;
    if fun == PlusSymbol() {
        if isZero(arg1) {
            ans = arg2;
        } else if isZero(arg2) {
            ans = arg1;
        } else if isUminus(arg1) {
            ans = simplify(MinusSymbol(), arg2, CADR(arg1));
        } else if isUminus(arg2) {
            ans = simplify(MinusSymbol(), arg1, CADR(arg2));
        } else {
            ans = lang3(PlusSymbol(), arg1, arg2);
        }
    } else if fun == MinusSymbol() {
        if arg2 == R_MissingArg() {
            if isZero(arg1) {
                ans = Constant(0.0);
            } else if isUminus(arg1) {
                ans = CADR(arg1);
            } else {
                ans = lang2(MinusSymbol(), arg1);
            }
        }
    } else if fun == TimesSymbol() {
        if isZero(arg1) || isZero(arg2) {
            ans = Constant(0.0);
        } else if isOne(arg1) {
            ans = arg2;
        } else if isOne(arg2) {
            ans = arg1;
        } else if isUminus(arg1) {
            ans = simplify(
                MinusSymbol(),
                PP(simplify(TimesSymbol(), CADR(arg1), arg2)),
                R_MissingArg(),
            );
            Rf_unprotect(1);
        } else if isUminus(arg2) {
            ans = simplify(
                MinusSymbol(),
                PP(simplify(TimesSymbol(), arg1, CADR(arg2))),
                R_MissingArg(),
            );
            Rf_unprotect(1);
        } else {
            ans = lang3(TimesSymbol(), arg1, arg2);
        }
    } else if fun == DivideSymbol() {
        if isZero(arg1) {
            ans = Constant(0.0);
        } else if isZero(arg2) {
            ans = Constant(NA_REAL);
        } else if isOne(arg2) {
            ans = arg1;
        } else if isUminus(arg1) {
            ans = simplify(
                MinusSymbol(),
                PP(simplify(DivideSymbol(), CADR(arg1), arg2)),
                R_MissingArg(),
            );
            Rf_unprotect(1);
        } else if isUminus(arg2) {
            ans = simplify(
                MinusSymbol(),
                PP(simplify(DivideSymbol(), arg1, CADR(arg2))),
                R_MissingArg(),
            );
            Rf_unprotect(1);
        } else {
            ans = lang3(DivideSymbol(), arg1, arg2);
        }
    } else if fun == PowerSymbol() {
        if isZero(arg2) {
            ans = Constant(1.0);
        } else if isZero(arg1) {
            ans = Constant(0.0);
        } else if isOne(arg1) {
            ans = Constant(1.0);
        } else if isOne(arg2) {
            ans = arg1;
        } else {
            ans = lang3(PowerSymbol(), arg1, arg2);
        }
    } else if fun == ExpSymbol() {
        ans = lang2(ExpSymbol(), arg1);
    } else if fun == LogSymbol() {
        ans = lang2(LogSymbol(), arg1);
    } else if fun == CosSymbol() {
        ans = lang2(CosSymbol(), arg1);
    } else if fun == SinSymbol() {
        ans = lang2(SinSymbol(), arg1);
    } else if fun == TanSymbol() {
        ans = lang2(TanSymbol(), arg1);
    } else if fun == CoshSymbol() {
        ans = lang2(CoshSymbol(), arg1);
    } else if fun == SinhSymbol() {
        ans = lang2(SinhSymbol(), arg1);
    } else if fun == TanhSymbol() {
        ans = lang2(TanhSymbol(), arg1);
    } else if fun == SqrtSymbol() {
        ans = lang2(SqrtSymbol(), arg1);
    } else if fun == PnormSymbol() {
        ans = lang2(PnormSymbol(), arg1);
    } else if fun == DnormSymbol() {
        ans = lang2(DnormSymbol(), arg1);
    } else if fun == AsinSymbol() {
        ans = lang2(AsinSymbol(), arg1);
    } else if fun == AcosSymbol() {
        ans = lang2(AcosSymbol(), arg1);
    } else if fun == AtanSymbol() {
        ans = lang2(AtanSymbol(), arg1);
    } else if fun == GammaSymbol() {
        ans = lang2(GammaSymbol(), arg1);
    } else if fun == LGammaSymbol() {
        ans = lang2(LGammaSymbol(), arg1);
    } else if fun == DiGammaSymbol() {
        ans = lang2(DiGammaSymbol(), arg1);
    } else if fun == TriGammaSymbol() {
        ans = lang2(TriGammaSymbol(), arg1);
    } else if fun == PsiSymbol() {
        if arg2 == R_MissingArg() {
            ans = lang2(PsiSymbol(), arg1);
        } else {
            ans = lang3(PsiSymbol(), arg1, arg2);
        }
    } else if fun == ExpM1Symbol() {
        ans = lang2(ExpM1Symbol(), arg1);
    } else if fun == Log1PSymbol() {
        ans = lang2(Log1PSymbol(), arg1);
    } else if fun == Log2Symbol() {
        ans = lang2(Log2Symbol(), arg1);
    } else if fun == Log10Symbol() {
        ans = lang2(Log10Symbol(), arg1);
    } else if fun == CosPiSymbol() {
        ans = lang2(CosPiSymbol(), arg1);
    } else if fun == SinPiSymbol() {
        ans = lang2(SinPiSymbol(), arg1);
    } else if fun == TanPiSymbol() {
        ans = lang2(TanPiSymbol(), arg1);
    } else if fun == FactorialSymbol() {
        ans = lang2(FactorialSymbol(), arg1);
    } else if fun == LFactorialSymbol() {
        ans = lang2(LFactorialSymbol(), arg1);
    } else {
        ans = Constant(NA_REAL);
    }
    ans
}

// ---------------------------------------------------------------------------
// D() -- symbolic derivative
// ---------------------------------------------------------------------------

macro_rules! PP_S {
    ($f:expr, $a1:expr, $a2:expr) => {
        PP(simplify($f, $a1, $a2))
    };
}

macro_rules! PP_S2 {
    ($f:expr, $a1:expr) => {
        PP(simplify($f, $a1, R_MissingArg()))
    };
}

unsafe fn D(expr: SEXP, var: SEXP) -> SEXP {
    let mut ans: SEXP = R_NilValue();
    let mut expr1: SEXP;
    let mut expr2: SEXP;

    match TYPEOF(expr) {
        t if t == SEXPTYPE::LGLSXP
            || t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::CPLXSXP =>
        {
            ans = Constant(0.0);
        }
    }
    ans
}

// ---------------------------------------------------------------------------
// isPlusForm, isMinusForm, isTimesForm, isDivideForm, isPowerForm
// ---------------------------------------------------------------------------

unsafe fn isPlusForm(expr: SEXP) -> bool {
    TYPEOF(expr) == SEXPTYPE::LANGSXP && length(expr) == 3 && CAR(expr) == PlusSymbol()
}

unsafe fn isMinusForm(expr: SEXP) -> bool {
    TYPEOF(expr) == SEXPTYPE::LANGSXP && length(expr) == 3 && CAR(expr) == MinusSymbol()
}

unsafe fn isTimesForm(expr: SEXP) -> bool {
    TYPEOF(expr) == SEXPTYPE::LANGSXP && length(expr) == 3 && CAR(expr) == TimesSymbol()
}

unsafe fn isDivideForm(expr: SEXP) -> bool {
    TYPEOF(expr) == SEXPTYPE::LANGSXP && length(expr) == 3 && CAR(expr) == DivideSymbol()
}

unsafe fn isPowerForm(expr: SEXP) -> bool {
    TYPEOF(expr) == SEXPTYPE::LANGSXP && length(expr) == 3 && CAR(expr) == PowerSymbol()
}

// ---------------------------------------------------------------------------
// AddParens
// ---------------------------------------------------------------------------

unsafe fn AddParens(expr: SEXP) -> SEXP {
    if TYPEOF(expr) == SEXPTYPE::LANGSXP {
        let mut e = CDR(expr);
        while e != R_NilValue() {
            SETCAR(e, AddParens(CAR(e)));
            e = CDR(e);
        }
    }
    if isPlusForm(expr) {
        if isPlusForm(CADDR(expr)) {
            SETCADDR(expr, lang2(ParenSymbol(), CADDR(expr)));
        }
    } else if isMinusForm(expr) {
        if isPlusForm(CADDR(expr)) || isMinusForm(CADDR(expr)) {
            SETCADDR(expr, lang2(ParenSymbol(), CADDR(expr)));
        }
    } else if isTimesForm(expr) {
        if isPlusForm(CADDR(expr))
            || isMinusForm(CADDR(expr))
            || isTimesForm(CADDR(expr))
            || isDivideForm(CADDR(expr))
        {
            SETCADDR(expr, lang2(ParenSymbol(), CADDR(expr)));
        }
    }
    expr
}

// ---------------------------------------------------------------------------
// doD
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub unsafe fn doD(args: SEXP) -> SEXP {
    let args = CDR(args);
    let expr: SEXP;
    if isExpression(CAR(args)) {
        expr = VECTOR_ELT(CAR(args), 0);
    } else {
        expr = CAR(args);
    }
    if !(isLanguage(expr) || isSymbol(expr) || isNumeric(expr) || isComplex(expr)) {
        error("expression must not be type 'invalid'");
    }
    let mut var = CADR(args);
    if !isString(var) || length(var) < 1 {
        error("variable must be a character string");
    }
    if length(var) > 1 {
        warning("only the first element is used as variable name");
    }
    var = installTrChar(STRING_ELT(var, 0));
    InitDerivSymbols();
    let mut expr = expr;
    expr = D(expr, var);
    Rf_protect(expr);
    expr = AddParens(expr);
    Rf_unprotect(1);
    expr
}

// ---------------------------------------------------------------------------
// InvalidExpression (never returns)
// ---------------------------------------------------------------------------

fn invalid_expression(where_: &str) -> ! {
    let msg = format!("invalid expression in '{}'", where_);
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    unsafe {
        crate::main::errors::Rf_error(c_msg.as_ptr());
    }
    unreachable!()
}

// ---------------------------------------------------------------------------
// equal -- deep equality of expressions
// ---------------------------------------------------------------------------

unsafe fn equal(expr1: SEXP, expr2: SEXP) -> bool {
    if TYPEOF(expr1) == TYPEOF(expr2) {
        match TYPEOF(expr1) {
            t if t == SEXPTYPE::NILSXP => true,
            t if t == SEXPTYPE::SYMSXP => expr1 == expr2,
            t if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP => {
                *INTEGER(expr1) == *INTEGER(expr2)
            }
        }
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Accumulate
// ---------------------------------------------------------------------------

unsafe fn Accumulate(expr: SEXP, exprlist: SEXP) -> c_int {
    let mut e = exprlist;
    let mut k: c_int = 0;
    while CDR(e) != R_NilValue() {
        e = CDR(e);
        k = k + 1;
        if equal(expr, CAR(e)) {
            return k;
        }
    }
    SETCDR(e, crate::sexp::constructors::Rf_cons(expr, R_NilValue()));
    k + 1
}

unsafe fn Accumulate2(expr: SEXP, exprlist: SEXP) -> c_int {
    let mut e = exprlist;
    let mut k: c_int = 0;
    while CDR(e) != R_NilValue() {
        e = CDR(e);
        k = k + 1;
    }
    SETCDR(e, crate::sexp::constructors::Rf_cons(expr, R_NilValue()));
    k + 1
}

// ---------------------------------------------------------------------------
// MakeVariable
// ---------------------------------------------------------------------------

unsafe fn MakeVariable(k: c_int, tag: &str) -> SEXP {
    let buf = format!("{}{}", tag, k);
    if buf.len() >= 64 {
        error("too many variables");
    }
    install_str(&buf)
}

// ---------------------------------------------------------------------------
// FindSubexprs
// ---------------------------------------------------------------------------

unsafe fn FindSubexprs(expr: SEXP, exprlist: SEXP, tag: &str) -> c_int {
    match TYPEOF(expr) {
        t if t == SEXPTYPE::SYMSXP
            || t == SEXPTYPE::LGLSXP
            || t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::CPLXSXP =>
        {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// CountOccurrences
// ---------------------------------------------------------------------------

unsafe fn CountOccurrences(sym: SEXP, lst: SEXP) -> c_int {
    match TYPEOF(lst) {
        t if t == SEXPTYPE::SYMSXP => {
            if lst == sym {
                1
            } else {
                0
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Replace
// ---------------------------------------------------------------------------

unsafe fn Replace(sym: SEXP, expr: SEXP, lst: SEXP) -> SEXP {
    match TYPEOF(lst) {
        t if t == SEXPTYPE::SYMSXP => {
            if lst == sym {
                expr
            } else {
                lst
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CreateGrad
// ---------------------------------------------------------------------------

unsafe fn CreateGrad(names: SEXP) -> SEXP {
    let n = length(names);

    let dimnames = lang3(R_NilValue(), R_NilValue(), R_NilValue());
    let _dimnames_guard = protect(dimnames);
    SETCAR(dimnames, install_str("list"));
    let p = install_str("c");
    let q = crate::sexp::constructors::Rf_allocList(n);
    let _q_guard = protect(q);
    SETCADDR(dimnames, LCONS(p, q));
    let mut qq = CADDR(dimnames);
    // skip the LCONS head to get to the list chain
    qq = CDR(qq); // skip the "c" symbol
    for i in 0..n {
        SETCAR(qq, Rf_ScalarString(STRING_ELT(names, i as i64)));
        qq = CDR(qq);
    }

    let dim = lang3(R_NilValue(), R_NilValue(), R_NilValue());
    let _dim_guard = protect(dim);
    SETCAR(dim, install_str("c"));
    SETCADR(dim, lang2(install_str("length"), install_str(".value")));
    SETCADDR(dim, Rf_ScalarInteger(n));

    let data = Rf_ScalarReal(0.0);
    let _data_guard = protect(data);
    let p = lang4(install_str("array"), data, dim, dimnames);
    let _p_guard = protect(p);
    let p = lang3(install_str("<-"), install_str(".grad"), p);
    p
}

// ---------------------------------------------------------------------------
// CreateHess
// ---------------------------------------------------------------------------

unsafe fn CreateHess(names: SEXP) -> SEXP {
    let n = length(names);

    let dimnames = lang4(
        R_NilValue(),
        R_NilValue(),
        R_NilValue(),
        R_NilValue(),
    );
    let _dimnames_guard = protect(dimnames);
    SETCAR(dimnames, install_str("list"));
    let p = install_str("c");
    let q = crate::sexp::constructors::Rf_allocList(n);
    let _q_guard = protect(q);
    SETCADDR(dimnames, LCONS(p, q));
    let mut qq = CADDR(dimnames);
    qq = CDR(qq); // skip the "c" symbol
    for i in 0..n {
        SETCAR(qq, Rf_ScalarString(STRING_ELT(names, i as i64)));
        qq = CDR(qq);
    }
    SETCADDDR(dimnames, duplicate(CADDR(dimnames)));

    let dim = lang4(
        R_NilValue(),
        R_NilValue(),
        R_NilValue(),
        R_NilValue(),
    );
    let _dim_guard = protect(dim);
    SETCAR(dim, install_str("c"));
    SETCADR(dim, lang2(install_str("length"), install_str(".value")));
    SETCADDR(dim, Rf_ScalarInteger(n));
    SETCADDDR(dim, Rf_ScalarInteger(n));

    let data = Rf_ScalarReal(0.0);
    let _data_guard = protect(data);
    let p = lang4(install_str("array"), data, dim, dimnames);
    let _p_guard = protect(p);
    let p = lang3(install_str("<-"), install_str(".hessian"), p);
    p
}

// ---------------------------------------------------------------------------
// DerivAssign
// ---------------------------------------------------------------------------

unsafe fn DerivAssign(name: SEXP, expr: SEXP) -> SEXP {
    let ans = lang3(install_str("<-"), R_NilValue(), expr);
    let _ans_guard = protect(ans);
    let newname = Rf_ScalarString(name);
    let _newname_guard = protect(newname);
    let bracket_sym = crate::sexp::symbol::R_BracketSymbol();
    SETCADR(
        ans,
        lang4(bracket_sym, install_str(".grad"), R_MissingArg(), newname),
    );
    ans
}

// ---------------------------------------------------------------------------
// HessAssign1
// ---------------------------------------------------------------------------

unsafe fn HessAssign1(name: SEXP, expr: SEXP) -> SEXP {
    let ans = lang3(install_str("<-"), R_NilValue(), expr);
    let _ans_guard = protect(ans);
    let newname = Rf_ScalarString(name);
    let _newname_guard = protect(newname);
    let bracket_sym = crate::sexp::symbol::R_BracketSymbol();
    SETCADR(
        ans,
        lang5(
            bracket_sym,
            install_str(".hessian"),
            R_MissingArg(),
            newname,
            newname,
        ),
    );
    ans
}

// ---------------------------------------------------------------------------
// HessAssign2
// ---------------------------------------------------------------------------

unsafe fn HessAssign2(name1: SEXP, name2: SEXP, expr: SEXP) -> SEXP {
    let newname1 = Rf_ScalarString(name1);
    let _newname1_guard = protect(newname1);
    let newname2 = Rf_ScalarString(name2);
    let _newname2_guard = protect(newname2);
    let bracket_sym = crate::sexp::symbol::R_BracketSymbol();
    let tmp1 = lang5(
        bracket_sym,
        install_str(".hessian"),
        R_MissingArg(),
        newname1,
        newname2,
    );
    let _tmp1_guard = protect(tmp1);
    let tmp2 = lang5(
        bracket_sym,
        install_str(".hessian"),
        R_MissingArg(),
        newname2,
        newname1,
    );
    let _tmp2_guard = protect(tmp2);
    let tmp3 = lang3(install_str("<-"), tmp2, expr);
    let _tmp3_guard = protect(tmp3);
    let ans = lang3(install_str("<-"), tmp1, tmp3);
    ans
}

// ---------------------------------------------------------------------------
// AddGrad
// ---------------------------------------------------------------------------

unsafe fn AddGrad() -> SEXP {
    let ans = crate::sexp::constructors::Rf_mkString(
        b"gradient\0".as_ptr() as *const c_char,
    );
    let _string_guard = protect(ans);
    let ans = lang3(install_str("attr"), install_str(".value"), ans);
    let _attr_guard = protect(ans);
    let ans = lang3(install_str("<-"), ans, install_str(".grad"));
    ans
}

// ---------------------------------------------------------------------------
// AddHess
// ---------------------------------------------------------------------------

unsafe fn AddHess() -> SEXP {
    let ans = crate::sexp::constructors::Rf_mkString(
        b"hessian\0".as_ptr() as *const c_char,
    );
    let _string_guard = protect(ans);
    let ans = lang3(install_str("attr"), install_str(".value"), ans);
    let _attr_guard = protect(ans);
    let ans = lang3(install_str("<-"), ans, install_str(".hessian"));
    ans
}

// ---------------------------------------------------------------------------
// Prune
// ---------------------------------------------------------------------------

unsafe fn Prune(lst: SEXP) -> SEXP {
    if lst == R_NilValue() {
        return lst;
    }
    SETCDR(lst, Prune(CDR(lst)));
    if CAR(lst) == R_MissingArg() {
        CDR(lst)
    } else {
        lst
    }
}

// ---------------------------------------------------------------------------
// deriv -- main deriv() function
// ---------------------------------------------------------------------------

/// deriv(expr, namevec, function.arg, tag, hessian)
#[allow(dead_code)]
pub unsafe fn deriv(args: SEXP) -> SEXP {
    let mut ans: SEXP;
    let mut ans2: SEXP;
    let mut expr: SEXP;
    let mut funarg: SEXP;
    let mut names: SEXP;
    let f_index: c_int;
    let d_index: *mut c_int;
    let d2_index: *mut c_int;
    let i: c_int;
    let mut j: c_int;
    let mut k: c_int;
    let nexpr: c_int;
    let nderiv: c_int;
    let hessian: bool;
    let exprlist: SEXP;
    let tag: &str;

    let vmax = vmaxget();

    let mut args = CDR(args);
    InitDerivSymbols();
    let exprlist = Rf_protect(LCONS(crate::sexp::symbol::R_BraceSymbol(), R_NilValue()));

    /* expr: */
    if isExpression(CAR(args)) {
        expr = VECTOR_ELT(CAR(args), 0);
        Rf_protect(expr);
    } else {
        expr = CAR(args);
        Rf_protect(expr);
    }
    args = CDR(args);

    /* namevec: */
    names = CAR(args);
    if !isString(names) || length(names) < 1 {
        error("invalid variable names");
    }
    nderiv = length(names);
    args = CDR(args);

    /* function.arg: */
    funarg = CAR(args);
    args = CDR(args);

    /* tag: */
    let stag = CAR(args);
    if !isString(stag) || length(stag) < 1 {
        error("invalid tag");
    }
    let tag_sexp = STRING_ELT(stag, 0);
    let tag_ptr = translateChar_local(tag_sexp);
    if tag_ptr.is_null() {
        error("invalid tag");
    }
    let tag_cstr = std::ffi::CStr::from_ptr(tag_ptr);
    let tag_str = tag_cstr.to_str().unwrap_or("");
    if tag_str.len() > 60 {
        error("invalid tag");
    }
    tag = tag_str;

    args = CDR(args);

    /* hessian: */
    hessian = asLogical(CAR(args)) != 0;

    /* NOTE: FindSubexprs is destructive, hence the duplication. */
    ans = duplicate(expr);
    Rf_protect(ans);
    f_index = FindSubexprs(ans, exprlist, tag);
    Rf_unprotect(1); // ans

    d_index = R_alloc(std::mem::size_of::<c_int>(), nderiv as usize) as *mut c_int;
    if hessian {
        d2_index = R_alloc(
            std::mem::size_of::<c_int>(),
            ((nderiv * (1 + nderiv)) / 2) as usize,
        ) as *mut c_int;
    } else {
        d2_index = d_index; // -Wall
    }

    let mut ii: c_int = 0;
    k = 0;
    while ii < nderiv {
        ans = duplicate(expr);
        Rf_protect(ans);
        ans = D(ans, installTrChar(STRING_ELT(names, ii as i64)));
        Rf_protect(ans);
        ans2 = duplicate(ans);
        Rf_protect(ans2);
        *d_index.add(ii as usize) = FindSubexprs(ans, exprlist, tag);
        ans = duplicate(ans2);
        Rf_protect(ans);
        if hessian {
            j = ii;
            while j < nderiv {
                ans2 = duplicate(ans);
                Rf_protect(ans2);
                ans2 = D(ans2, installTrChar(STRING_ELT(names, j as i64)));
                Rf_protect(ans2);
                *d2_index.add(k as usize) = FindSubexprs(ans2, exprlist, tag);
                k += 1;
                Rf_unprotect(2);
                j += 1;
            }
        }
    }

    nexpr = length(exprlist) - 1;

    if f_index != 0 {
        Accumulate2(MakeVariable(f_index, tag), exprlist);
    } else {
        ans = duplicate(expr);
        Rf_protect(ans);
        Accumulate2(expr, exprlist);
        Rf_unprotect(1);
    }
    Accumulate2(R_NilValue(), exprlist);
    if hessian {
        Accumulate2(R_NilValue(), exprlist);
    }

    ii = 0;
    k = 0;
    while ii < nderiv {
        if *d_index.add(ii as usize) != 0 {
            Accumulate2(MakeVariable(*d_index.add(ii as usize), tag), exprlist);
            if hessian {
                ans = duplicate(expr);
                Rf_protect(ans);
                ans = D(ans, installTrChar(STRING_ELT(names, ii as i64)));
                Rf_protect(ans);
                j = ii;
                while j < nderiv {
                    if *d2_index.add(k as usize) != 0 {
                        Accumulate2(MakeVariable(*d2_index.add(k as usize), tag), exprlist);
                    } else {
                        ans2 = duplicate(ans);
                        Rf_protect(ans2);
                        ans2 = D(ans2, installTrChar(STRING_ELT(names, j as i64)));
                        Rf_protect(ans2);
                        Accumulate2(ans2, exprlist);
                        Rf_unprotect(2);
                    }
                }
            }
        }
    }

    Accumulate2(R_NilValue(), exprlist);
    Accumulate2(R_NilValue(), exprlist);
    if hessian {
        Accumulate2(R_NilValue(), exprlist);
    }

    let mut ii: c_int = 0;
    let mut ans_ptr = CDR(exprlist);
    while ii < nexpr {
        if CountOccurrences(MakeVariable(ii + 1, tag), CDR(ans_ptr)) < 2 {
            SETCDR(
                ans_ptr,
                Replace(MakeVariable(ii + 1, tag), CAR(ans_ptr), CDR(ans_ptr)),
            );
            SETCAR(ans_ptr, R_MissingArg());
        } else {
            let var = Rf_protect(MakeVariable(ii + 1, tag));
            SETCAR(
                ans_ptr,
                lang3(install_str("<-"), var, AddParens(CAR(ans_ptr))),
            );
            Rf_unprotect(1);
        }
    }

    /* .value <- ... */
    SETCAR(
        ans_ptr,
        lang3(
            install_str("<-"),
            install_str(".value"),
            AddParens(CAR(ans_ptr)),
        ),
    );
    ans_ptr = CDR(ans_ptr);

    /* .grad <- ... */
    SETCAR(ans_ptr, CreateGrad(names));
    ans_ptr = CDR(ans_ptr);

    /* .hessian <- ... */
    if hessian {
        SETCAR(ans_ptr, CreateHess(names));
        ans_ptr = CDR(ans_ptr);
    }

    /* .grad[, "..."] <- ... */
    let mut ii: c_int = 0;
    let mut kk: c_int = 0;
    while ii < nderiv {
        SETCAR(
            ans_ptr,
            DerivAssign(STRING_ELT(names, ii as i64), AddParens(CAR(ans_ptr))),
        );
        ans_ptr = CDR(ans_ptr);
        if hessian {
            j = ii;
            while j < nderiv {
                if CAR(ans_ptr) != R_MissingArg() {
                    if ii == j {
                        SETCAR(
                            ans_ptr,
                            HessAssign1(STRING_ELT(names, ii as i64), AddParens(CAR(ans_ptr))),
                        );
                    } else {
                        SETCAR(
                            ans_ptr,
                            HessAssign2(
                                STRING_ELT(names, ii as i64),
                                STRING_ELT(names, j as i64),
                                AddParens(CAR(ans_ptr)),
                            ),
                        );
                    }
                }
            }
        }
    }

    /* attr(.value, "gradient") <- .grad */
    SETCAR(ans_ptr, AddGrad());
    ans_ptr = CDR(ans_ptr);
    if hessian {
        SETCAR(ans_ptr, AddHess());
        ans_ptr = CDR(ans_ptr);
    }

    /* .value */
    SETCAR(ans_ptr, install_str(".value"));

    /* Prune the expression list removing eliminated sub-expressions */
    SETCDR(exprlist, Prune(CDR(exprlist)));

    if TYPEOF(funarg) == SEXPTYPE::LGLSXP && *LOGICAL(funarg) != 0 {
        /* fun = TRUE */
        funarg = names;
    }

    if TYPEOF(funarg) == SEXPTYPE::CLOSXP {
        let formals = crate::sexp::accessors::FORMALS(funarg);
        let rho = crate::sexp::accessors::CLOENV(funarg);
        funarg = R_mkClosure(formals, exprlist, rho);
    } else if isString(funarg) {
        names = duplicate(funarg);
        Rf_protect(names);
        let a = Rf_protect(crate::sexp::constructors::Rf_allocList(length(names)));
        let mut aa = a;
        for ii in 0..length(names) {
            SETTAG(aa, installTrChar(STRING_ELT(names, ii as i64)));
            SETCAR(aa, R_MissingArg());
            aa = CDR(aa);
        }
    }

    vmaxset(vmax);
    Rf_unprotect(2); // exprlist, expr
    funarg
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;

    #[test]
    fn derivative_symbols_are_owned_by_active_session() {
        let mut left = RSession::new();
        let left_plus = unsafe { PlusSymbol() };
        let mut right = RSession::new();
        let right_plus = unsafe { PlusSymbol() };

        assert!(!left_plus.is_null());
        assert!(!right_plus.is_null());
        assert_ne!(left_plus, right_plus);

        let left_again = left
            .with_arena(|_| unsafe { PlusSymbol() })
            .expect("left session should be active");
        assert_eq!(left_plus, left_again);

        let right_again = right
            .with_arena(|_| unsafe { PlusSymbol() })
            .expect("right session should be active");
        assert_eq!(right_plus, right_again);
    }
}
