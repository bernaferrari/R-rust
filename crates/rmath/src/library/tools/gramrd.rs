/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/tools/src/gramRd.c
 *
 *  Rd documentation parser — Bison-generated LALR(1) parser.
 *  Ported from gramRd.y / gramRd.c.
 *
 *  This file contains the Bison-generated parser tables and state machine
 *  for parsing .Rd (R documentation) files.
 */

use crate::mainutils::errors::Rf_error_unimplemented;
use crate::sexp::ffi::SEXP;

/* ==================== Parser state ==================== */

/* ==================== Token types ==================== */

/// Rd parser token types.
mod yytokentype {
    use std::os::raw::c_int;
    pub const EOF: c_int = 0;
    pub const SECTION: c_int = 258;
    pub const TEXT: c_int = 259;
    pub const VERB: c_int = 260;
    pub const VCODE: c_int = 261;
    pub const CODE: c_int = 262;
    pub const COMMENT: c_int = 263;
    pub const CONDITIONAL: c_int = 264;
    pub const NEW_CMD: c_int = 265;
    pub const Lbrace: c_int = 266;
    pub const Rbrace: c_int = 267;
    pub const Lbrack: c_int = 268;
    pub const Rbrack: c_int = 269;
    pub const Lparen: c_int = 270;
    pub const Rparen: c_int = 271;
    pub const Lbrack2: c_int = 272;
    pub const Rbrack2: c_int = 273;
    pub const ESCAPE: c_int = 274;
    pub const RCODE: c_int = 275;
    pub const VRCODE: c_int = 276;
    pub const SPECIAL: c_int = 277;
    pub const NEWLINE: c_int = 278;
    pub const WHITESPACE: c_int = 279;
    pub const HASH: c_int = 280;
    pub const EQHASH: c_int = 281;
    pub const BACKTICK: c_int = 282;
    pub const ENC: c_int = 283;
    pub const SIGNED: c_int = 284;
    pub const USERMACRO: c_int = 285;
    pub const ERR: c_int = 256;
    pub const UNK: c_int = 257;
}

/* ==================== Rd parser functions ==================== */

/// R_ParseRd - parse an Rd file (stub).
/// In the real implementation, this calls the Bison-generated yyparse()
/// to produce a parsed Rd object (a list of Rd sections).
pub unsafe fn R_ParseRd(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsupported("tools::R_ParseRd")
}

/// Rd2HTML - convert parsed Rd to HTML (stub).
pub unsafe fn Rd2HTML(_item: SEXP, _args: SEXP) -> SEXP {
    unsupported("tools::Rd2HTML")
}

/// Rd2TXT - convert parsed Rd to plain text (stub).
pub unsafe fn Rd2TXT(_item: SEXP, _args: SEXP) -> SEXP {
    unsupported("tools::Rd2TXT")
}

/// install_Rd2HTML - register Rd2HTML converter (stub).
pub unsafe fn install_Rd2HTML() {
    unsupported("tools::install_Rd2HTML")
}

/// install_Rd2TXT - register Rd2TXT converter (stub).
pub unsafe fn install_Rd2TXT() {
    unsupported("tools::install_Rd2TXT")
}

/// install_Rd2LaTeX - register Rd2LaTeX converter (stub).
pub unsafe fn install_Rd2LaTeX() {
    unsupported("tools::install_Rd2LaTeX")
}

fn unsupported(name: &str) -> ! {
    Rf_error_unimplemented(name);
    unreachable!("Rf_error_unimplemented returned")
}
