#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Operator and special form symbols.
//!
//! Pre-interns all the symbols used by the evaluator for operator
//! recognition and special form dispatch.

use std::ffi::CString;
use std::ptr;

use crate::sexp::ffi::SEXP;
use crate::sexp::symbol::Rf_install;

/// Install and cache commonly used operator symbols.
///
/// This is the equivalent of R's `R_initEvalSymbols()` in eval.c.
pub unsafe fn R_initEvalSymbols() {
    unsafe {
        // Operator symbols
        let _ = R_IfSymbol();
        let _ = R_WhileSymbol();
        let _ = R_ForSymbol();
        let _ = R_RepeatSymbol();
        let _ = R_BraceSymbol();

        // Additional operator symbols
        Rf_install_sym("(");
        Rf_install_sym("-");
        Rf_install_sym("*");
        Rf_install_sym("/");
        Rf_install_sym("^");
        Rf_install_sym("%%");
        Rf_install_sym("%/%");
        Rf_install_sym("%*%");
        Rf_install_sym("+");
        Rf_install_sym("-");
        Rf_install_sym("*");
        Rf_install_sym("/");
        Rf_install_sym("<");
        Rf_install_sym("<=");
        Rf_install_sym(">=");
        Rf_install_sym(">");
        Rf_install_sym("==");
        Rf_install_sym("!=");
        Rf_install_sym("!");
        Rf_install_sym("&");
        Rf_install_sym("&&");
        Rf_install_sym("|");
        Rf_install_sym("||");
        Rf_install_sym("~");
        Rf_install_sym("->");
        Rf_install_sym("?");
        Rf_install_sym("::");
        Rf_install_sym(":::");
        Rf_install_sym("$");
        Rf_install_sym("@");
        Rf_install_sym("[");
        Rf_install_sym("[[");
        Rf_install_sym("<-");
        Rf_install_sym("<<-");
        Rf_install_sym("=");
        Rf_install_sym("function");
        Rf_install_sym("break");
        Rf_install_sym("next");
        Rf_install_sym("return");
        Rf_install_sym("on.exit");
        Rf_install_sym("missing");
        Rf_install_sym("quote");
        Rf_install_sym("eval");
        Rf_install_sym("sys.call");
        Rf_install_sym("sys.function");
        Rf_install_sym("environment");
        Rf_install_sym("...");
        Rf_install_sym("as.double");
        Rf_install_sym("as.logical");
        Rf_install_sym("as.integer");
        Rf_install_sym("as.character");
        Rf_install_sym("as.complex");
        Rf_install_sym("as.raw");
    }
}

/// Helper to install a symbol by name.
unsafe fn Rf_install_sym(name: &str) -> SEXP {
    unsafe {
        let cs = CString::new(name).unwrap();
        Rf_install(cs.as_ptr())
    }
}

// Convenience re-exports
pub use crate::sexp::symbol::R_BraceSymbol;
pub use crate::sexp::symbol::R_DotsSymbol;
pub use crate::sexp::symbol::R_ForSymbol;
pub use crate::sexp::symbol::R_IfSymbol;
pub use crate::sexp::symbol::R_RepeatSymbol;
pub use crate::sexp::symbol::R_WhileSymbol;
