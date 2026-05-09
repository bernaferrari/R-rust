#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/names.c -- function dispatch table and symbol initialization.
//!
//! Implements the R_FunTab dispatch table (~940 entries), R_Primitive,
//! do_primitive, StrToInternal, installFunTab, SymbolShortcuts,
//! InitNames, installS3Signature, do_internal, do_tilde (stub),
//! DDVALSymbols, installDDVAL, mkSymMarker, getPRIMNAME.

use std::os::raw::{c_char, c_int};
use std::panic::panic_any;
use std::ptr;

use crate::eval::attrib_core::setAttrib;
use crate::mainutils::dstruct::mkPRIMSXP;
use crate::mainutils::duplicate::duplicate;
use crate::sexp::accessors::{
    CAR, CDR, CHAR, INTEGER, LENGTH, OBJECT, PRIMOFFSET, PRINTNAME, SET_ATTRIB, SET_INTERNAL,
    SET_PRINTNAME, SET_STRING_ELT, SET_SYMVALUE, STRING_ELT, TYPEOF,
};
use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar, Rf_mkString};
use crate::sexp::context::RError;
use crate::sexp::ffi::{FALSE, NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance;
use crate::sexp::memory::with_arena;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// PPkind constants (from Defn.h)
// ---------------------------------------------------------------------------

pub const PP_INVALID: c_int = 0;
pub const PP_ASSIGN: c_int = 1;
pub const PP_ASSIGN2: c_int = 2;
pub const PP_BINARY: c_int = 3;
pub const PP_BINARY2: c_int = 4;
pub const PP_BREAK: c_int = 5;
pub const PP_CURLY: c_int = 6;
pub const PP_FOR: c_int = 7;
pub const PP_FUNCALL: c_int = 8;
pub const PP_FUNCTION: c_int = 9;
pub const PP_IF: c_int = 10;
pub const PP_NEXT: c_int = 11;
pub const PP_PAREN: c_int = 12;
pub const PP_RETURN: c_int = 13;
pub const PP_SUBASS: c_int = 14;
pub const PP_SUBSET: c_int = 15;
pub const PP_WHILE: c_int = 16;
pub const PP_UNARY: c_int = 17;
pub const PP_DOLLAR: c_int = 18;
pub const PP_FOREIGN: c_int = 19;
pub const PP_REPEAT: c_int = 20;

// ---------------------------------------------------------------------------
// PPprec constants (from Defn.h)
// ---------------------------------------------------------------------------

pub const PREC_FN: c_int = 0;
pub const PREC_EQ: c_int = 1;
pub const PREC_LEFT: c_int = 2;
pub const PREC_RIGHT: c_int = 3;
pub const PREC_TILDE: c_int = 4;
pub const PREC_OR: c_int = 5;
pub const PREC_AND: c_int = 6;
pub const PREC_NOT: c_int = 7;
pub const PREC_COMPARE: c_int = 8;
pub const PREC_SUM: c_int = 9;
pub const PREC_PROD: c_int = 10;
pub const PREC_PERCENT: c_int = 11;
pub const PREC_COLON: c_int = 12;
pub const PREC_SIGN: c_int = 13;
pub const PREC_POWER: c_int = 14;
pub const PREC_SUBSET: c_int = 15;
pub const PREC_DOLLAR: c_int = 16;
pub const PREC_NS: c_int = 17;

// ---------------------------------------------------------------------------
// Operator offset constants (from Defn.h, enum arith_op_type)
// ---------------------------------------------------------------------------

pub const CTXT_BREAK: c_int = 2;
pub const CTXT_NEXT: c_int = 1;
pub const PLUSOP: c_int = 1;
pub const MINUSOP: c_int = 2;
pub const TIMESOP: c_int = 3;
pub const DIVOP: c_int = 4;
pub const POWOP: c_int = 5;
pub const MODOP: c_int = 6;
pub const IDIVOP: c_int = 7;

// ---------------------------------------------------------------------------
// PPinfo struct
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct PPinfo {
    pub kind: c_int,
    pub prec: c_int,
    pub rightassoc: c_int,
}

impl PPinfo {
    pub const fn new(kind: c_int, prec: c_int, rightassoc: c_int) -> Self {
        PPinfo {
            kind,
            prec,
            rightassoc,
        }
    }
}

// ---------------------------------------------------------------------------
// FunTabEntry
// ---------------------------------------------------------------------------

type PrimFun = Option<unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP>;

#[derive(Clone, Copy)]
pub struct FunTabEntry {
    pub name: &'static [u8], // null-terminated C string bytes
    pub cfun: PrimFun,
    pub offset: c_int,
    pub eval: c_int,
    pub arity: c_int,
    pub pp: PPinfo,
}

impl FunTabEntry {
    pub const fn new(
        name: &'static [u8],
        cfun: PrimFun,
        offset: c_int,
        eval: c_int,
        arity: c_int,
        pp: PPinfo,
    ) -> Self {
        FunTabEntry {
            name,
            cfun,
            offset,
            eval,
            arity,
            pp,
        }
    }

    /// Returns true if the name pointer is null (sentinel).
    pub fn is_sentinel(&self) -> bool {
        self.name.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Helper: null name bytes (empty slice used as sentinel)
// ---------------------------------------------------------------------------

const NULL_NAME: &[u8] = b"";

#[derive(Default)]
pub(crate) struct NamesRuntimeState {
    pub init_names_done: bool,
    pub ddval_symbols: Vec<SEXP>,
}

// ---------------------------------------------------------------------------
// R_FunTab: Master dispatch table
// ---------------------------------------------------------------------------

/// The complete R function dispatch table.
/// Each entry maps an R function name to its C implementation.
/// Entries with `None` for cfun are not yet ported.
pub static R_FunTab: &[FunTabEntry] = &FUNTAB_ENTRIES;

const FUNTAB_ENTRIES: &[FunTabEntry] = &[
    // ===== Language Related Constructs =====
    // Primitives
    FunTabEntry::new(b"if\0", None, 0, 200, -1, PPinfo::new(PP_IF, PREC_FN, 1)),
    FunTabEntry::new(
        b"while\0",
        None,
        0,
        100,
        2,
        PPinfo::new(PP_WHILE, PREC_FN, 0),
    ),
    FunTabEntry::new(b"for\0", None, 0, 100, 3, PPinfo::new(PP_FOR, PREC_FN, 0)),
    FunTabEntry::new(
        b"repeat\0",
        None,
        0,
        100,
        1,
        PPinfo::new(PP_REPEAT, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"break\0",
        None,
        CTXT_BREAK,
        0,
        0,
        PPinfo::new(PP_BREAK, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"next\0",
        None,
        CTXT_NEXT,
        0,
        0,
        PPinfo::new(PP_NEXT, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"return\0",
        None,
        0,
        0,
        -1,
        PPinfo::new(PP_RETURN, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"function\0",
        None,
        0,
        0,
        -1,
        PPinfo::new(PP_FUNCTION, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"<-\0",
        None,
        1,
        100,
        -1,
        PPinfo::new(PP_ASSIGN, PREC_LEFT, 1),
    ),
    FunTabEntry::new(b"=\0", None, 3, 100, -1, PPinfo::new(PP_ASSIGN, PREC_EQ, 1)),
    FunTabEntry::new(
        b"<<-\0",
        None,
        2,
        100,
        -1,
        PPinfo::new(PP_ASSIGN2, PREC_LEFT, 1),
    ),
    FunTabEntry::new(b"{\0", None, 0, 200, -1, PPinfo::new(PP_CURLY, PREC_FN, 0)),
    FunTabEntry::new(b"(\0", None, 0, 1, 1, PPinfo::new(PP_PAREN, PREC_FN, 0)),
    FunTabEntry::new(
        b".subset\0",
        None,
        1,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".subset2\0",
        None,
        2,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"[\0",
        None,
        1,
        0,
        -1,
        PPinfo::new(PP_SUBSET, PREC_SUBSET, 0),
    ),
    FunTabEntry::new(
        b"[[\0",
        None,
        2,
        0,
        -1,
        PPinfo::new(PP_SUBSET, PREC_SUBSET, 0),
    ),
    FunTabEntry::new(
        b"$\0",
        None,
        3,
        0,
        2,
        PPinfo::new(PP_DOLLAR, PREC_DOLLAR, 0),
    ),
    FunTabEntry::new(
        b"@\0",
        None,
        0,
        0,
        2,
        PPinfo::new(PP_DOLLAR, PREC_DOLLAR, 0),
    ),
    FunTabEntry::new(
        b"[<-\0",
        None,
        0,
        0,
        3,
        PPinfo::new(PP_SUBASS, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"[[<-\0",
        None,
        1,
        0,
        3,
        PPinfo::new(PP_SUBASS, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"$<-\0",
        None,
        1,
        0,
        3,
        PPinfo::new(PP_SUBASS, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"switch\0",
        None,
        0,
        200,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"browser\0",
        None,
        0,
        101,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".primTrace\0",
        None,
        0,
        101,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".primUntrace\0",
        None,
        1,
        101,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".Internal\0",
        None,
        0,
        200,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".Primitive\0",
        None,
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"call\0",
        None,
        0,
        0,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"quote\0",
        None,
        0,
        0,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"substitute\0",
        None,
        0,
        0,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"missing\0",
        None,
        1,
        0,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"nargs\0",
        None,
        1,
        1,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"on.exit\0",
        None,
        0,
        100,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"forceAndCall\0",
        None,
        0,
        0,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"declare\0",
        None,
        0,
        100,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // .Internals (error handling, conditions, etc.)
    FunTabEntry::new(
        b"stop\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"warning\0",
        None,
        0,
        111,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"gettext\0",
        None,
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"ngettext\0",
        None,
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"bindtextdomain\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".addCondHands\0",
        None,
        0,
        111,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".addGlobHands\0",
        None,
        0,
        111,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".resetCondHands\0",
        None,
        0,
        111,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".signalCondition\0",
        None,
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".dfltStop\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".dfltWarn\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".addRestart\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".getRestart\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".invokeRestart\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".addTryHandlers\0",
        None,
        0,
        111,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"geterrmessage\0",
        None,
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"seterrmessage\0",
        None,
        0,
        111,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"printDeferredWarnings\0",
        None,
        0,
        111,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"interruptsSuspended\0",
        None,
        0,
        11,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.function.default\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCTION, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"debug\0",
        None,
        0,
        111,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"undebug\0",
        None,
        1,
        111,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"isdebugged\0",
        None,
        2,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"debugonce\0",
        None,
        3,
        111,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Recall\0",
        None,
        0,
        210,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"delayedAssign\0",
        None,
        0,
        111,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"makeLazy\0",
        None,
        0,
        111,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"identical\0",
        None,
        0,
        11,
        8,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"C_tryCatchHelper\0",
        None,
        0,
        11,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"getNamespaceValue\0",
        None,
        0,
        211,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // ===== Binary Operators (primitives) =====
    FunTabEntry::new(
        b"+\0",
        None,
        PLUSOP,
        1,
        -1,
        PPinfo::new(PP_BINARY, PREC_SUM, 0),
    ),
    FunTabEntry::new(
        b"-\0",
        None,
        MINUSOP,
        1,
        -1,
        PPinfo::new(PP_BINARY, PREC_SUM, 0),
    ),
    FunTabEntry::new(
        b"*\0",
        None,
        TIMESOP,
        1,
        2,
        PPinfo::new(PP_BINARY, PREC_PROD, 0),
    ),
    FunTabEntry::new(
        b"/\0",
        None,
        DIVOP,
        1,
        2,
        PPinfo::new(PP_BINARY2, PREC_PROD, 0),
    ),
    FunTabEntry::new(
        b"^\0",
        None,
        POWOP,
        1,
        2,
        PPinfo::new(PP_BINARY2, PREC_POWER, 1),
    ),
    FunTabEntry::new(
        b"%%\0",
        None,
        MODOP,
        1,
        2,
        PPinfo::new(PP_BINARY2, PREC_PERCENT, 0),
    ),
    FunTabEntry::new(
        b"%/%\0",
        None,
        IDIVOP,
        1,
        2,
        PPinfo::new(PP_BINARY2, PREC_PERCENT, 0),
    ),
    FunTabEntry::new(
        b"%*%\0",
        None,
        0,
        1,
        2,
        PPinfo::new(PP_BINARY, PREC_PERCENT, 0),
    ),
    // Comparison operators
    FunTabEntry::new(
        b"==\0",
        None,
        1,
        1,
        2,
        PPinfo::new(PP_BINARY, PREC_COMPARE, 0),
    ),
    FunTabEntry::new(
        b"!=\0",
        None,
        2,
        1,
        2,
        PPinfo::new(PP_BINARY, PREC_COMPARE, 0),
    ),
    FunTabEntry::new(
        b"<\0",
        None,
        3,
        1,
        2,
        PPinfo::new(PP_BINARY, PREC_COMPARE, 0),
    ),
    FunTabEntry::new(
        b"<=\0",
        None,
        5,
        1,
        2,
        PPinfo::new(PP_BINARY, PREC_COMPARE, 0),
    ),
    FunTabEntry::new(
        b">=\0",
        None,
        6,
        1,
        2,
        PPinfo::new(PP_BINARY, PREC_COMPARE, 0),
    ),
    FunTabEntry::new(
        b">\0",
        None,
        4,
        1,
        2,
        PPinfo::new(PP_BINARY, PREC_COMPARE, 0),
    ),
    // Logical operators
    FunTabEntry::new(b"&\0", None, 1, 1, 2, PPinfo::new(PP_BINARY, PREC_AND, 0)),
    FunTabEntry::new(b"|\0", None, 2, 1, 2, PPinfo::new(PP_BINARY, PREC_OR, 0)),
    FunTabEntry::new(b"!\0", None, 3, 1, 1, PPinfo::new(PP_UNARY, PREC_NOT, 0)),
    FunTabEntry::new(b"&&\0", None, 1, 0, 2, PPinfo::new(PP_BINARY, PREC_AND, 0)),
    FunTabEntry::new(b"||\0", None, 2, 0, 2, PPinfo::new(PP_BINARY, PREC_OR, 0)),
    // Colon and special operators
    FunTabEntry::new(
        b":\0",
        None,
        0,
        1,
        2,
        PPinfo::new(PP_BINARY2, PREC_COLON, 0),
    ),
    FunTabEntry::new(
        b"~\0",
        None,
        0,
        0,
        -1,
        PPinfo::new(PP_BINARY, PREC_TILDE, 0),
    ),
    FunTabEntry::new(
        b"::\0",
        None,
        0,
        200,
        2,
        PPinfo::new(PP_BINARY2, PREC_NS, 0),
    ),
    FunTabEntry::new(
        b":::\0",
        None,
        0,
        200,
        2,
        PPinfo::new(PP_BINARY2, PREC_NS, 0),
    ),
    // ===== Logic Related Functions =====
    FunTabEntry::new(
        b"all\0",
        None,
        1,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"any\0",
        None,
        2,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // ===== Vectors, Matrices and Arrays =====
    // Primitives
    FunTabEntry::new(
        b"...elt\0",
        None,
        0,
        201,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"...length\0",
        None,
        0,
        1,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"...names\0",
        None,
        0,
        1,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"length\0",
        None,
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"length<-\0",
        None,
        0,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_LEFT, 1),
    ),
    FunTabEntry::new(b"c\0", None, 0, 1, -1, PPinfo::new(PP_FUNCALL, PREC_FN, 0)),
    FunTabEntry::new(
        b"oldClass\0",
        None,
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"oldClass<-\0",
        None,
        0,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"class\0",
        None,
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".cache_class\0",
        None,
        1,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".class2\0",
        None,
        2,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"class<-\0",
        None,
        0,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"unclass\0",
        None,
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"names\0",
        None,
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"names<-\0",
        None,
        0,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"dimnames\0",
        None,
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dimnames<-\0",
        None,
        0,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_LEFT, 1),
    ),
    FunTabEntry::new(b"dim\0", None, 0, 1, 1, PPinfo::new(PP_FUNCALL, PREC_FN, 0)),
    FunTabEntry::new(
        b"dim<-\0",
        None,
        0,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"attributes\0",
        None,
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"attributes<-\0",
        None,
        0,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"attr\0",
        None,
        0,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"attr<-\0",
        None,
        0,
        1,
        3,
        PPinfo::new(PP_FUNCALL, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"@<-\0",
        None,
        1,
        0,
        3,
        PPinfo::new(PP_SUBASS, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"levels<-\0",
        None,
        0,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_LEFT, 1),
    ),
    // .Internals (vectors, matrices, arrays)
    FunTabEntry::new(
        b"vector\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"complex\0",
        None,
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"matrix\0",
        None,
        0,
        11,
        7,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"array\0",
        None,
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"diag\0",
        None,
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"backsolve\0",
        None,
        0,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"max.col\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"row\0",
        None,
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"col\0",
        None,
        2,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"unlist\0",
        None,
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"cbind\0",
        None,
        1,
        10,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rbind\0",
        None,
        2,
        10,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"drop\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"all.names\0",
        None,
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"comment\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"comment<-\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"get\0",
        None,
        1,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"get0\0",
        None,
        2,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"mget\0",
        None,
        1,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"exists\0",
        None,
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"assign\0",
        None,
        0,
        111,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"list2env\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"remove\0",
        None,
        0,
        111,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"duplicated\0",
        None,
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"unique\0",
        None,
        1,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"anyDuplicated\0",
        None,
        2,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"anyNA\0",
        None,
        0,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"which\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"which.min\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pmin\0",
        None,
        0,
        11,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pmax\0",
        None,
        1,
        11,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"which.max\0",
        None,
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"match\0",
        None,
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pmatch\0",
        None,
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"charmatch\0",
        None,
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"match.call\0",
        None,
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"crossprod\0",
        None,
        1,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"tcrossprod\0",
        None,
        2,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"asplit\0",
        None,
        0,
        11,
        8,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"lengths\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sequence\0",
        None,
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"vhash\0",
        None,
        0,
        11,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"attach\0",
        None,
        0,
        111,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"detach\0",
        None,
        0,
        111,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"search\0",
        None,
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"setFileTime\0",
        None,
        0,
        111,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // ===== Mathematical Functions =====
    FunTabEntry::new(
        b"round\0",
        None,
        10001,
        0,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"signif\0",
        None,
        10004,
        0,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"log\0",
        None,
        10003,
        0,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"log10\0",
        None,
        10010,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"log2\0",
        None,
        10002,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(b"abs\0", None, 6, 1, 1, PPinfo::new(PP_FUNCALL, PREC_FN, 0)),
    FunTabEntry::new(
        b"floor\0",
        None,
        1,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"ceiling\0",
        None,
        2,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sqrt\0",
        None,
        3,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sign\0",
        None,
        4,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"trunc\0",
        None,
        5,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"exp\0",
        None,
        10,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"expm1\0",
        None,
        11,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"log1p\0",
        None,
        12,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"cos\0",
        None,
        20,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sin\0",
        None,
        21,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"tan\0",
        None,
        22,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"acos\0",
        None,
        23,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"asin\0",
        None,
        24,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"atan\0",
        None,
        25,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"cosh\0",
        None,
        30,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sinh\0",
        None,
        31,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"tanh\0",
        None,
        32,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"acosh\0",
        None,
        33,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"asinh\0",
        None,
        34,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"atanh\0",
        None,
        35,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"lgamma\0",
        None,
        40,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"gamma\0",
        None,
        41,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"digamma\0",
        None,
        42,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"trigamma\0",
        None,
        43,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"cospi\0",
        None,
        47,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sinpi\0",
        None,
        48,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"tanpi\0",
        None,
        49,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Math2 functions
    FunTabEntry::new(
        b"atan2\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"lbeta\0",
        None,
        2,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"beta\0",
        None,
        3,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"lchoose\0",
        None,
        4,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"choose\0",
        None,
        5,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dchisq\0",
        None,
        6,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pchisq\0",
        None,
        7,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qchisq\0",
        None,
        8,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dexp\0",
        None,
        9,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pexp\0",
        None,
        10,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qexp\0",
        None,
        11,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dgeom\0",
        None,
        12,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pgeom\0",
        None,
        13,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qgeom\0",
        None,
        14,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dpois\0",
        None,
        15,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"ppois\0",
        None,
        16,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qpois\0",
        None,
        17,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dt\0",
        None,
        18,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pt\0",
        None,
        19,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qt\0",
        None,
        20,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dsignrank\0",
        None,
        21,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"psignrank\0",
        None,
        22,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qsignrank\0",
        None,
        23,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"besselJ\0",
        None,
        24,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"besselY\0",
        None,
        25,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"psigamma\0",
        None,
        26,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Complex functions
    FunTabEntry::new(b"Re\0", None, 1, 1, 1, PPinfo::new(PP_FUNCALL, PREC_FN, 0)),
    FunTabEntry::new(b"Im\0", None, 2, 1, 1, PPinfo::new(PP_FUNCALL, PREC_FN, 0)),
    FunTabEntry::new(b"Mod\0", None, 3, 1, 1, PPinfo::new(PP_FUNCALL, PREC_FN, 0)),
    FunTabEntry::new(b"Arg\0", None, 4, 1, 1, PPinfo::new(PP_FUNCALL, PREC_FN, 0)),
    FunTabEntry::new(
        b"Conj\0",
        None,
        5,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Math3 functions (d/p/q for beta, binom, cauchy, f, gamma, lnorm, logis, nbinom, norm, unif, weibull, nchisq, nt, wilcox, bessel)
    FunTabEntry::new(
        b"dbeta\0",
        None,
        1,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pbeta\0",
        None,
        2,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qbeta\0",
        None,
        3,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dbinom\0",
        None,
        4,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pbinom\0",
        None,
        5,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qbinom\0",
        None,
        6,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dcauchy\0",
        None,
        7,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pcauchy\0",
        None,
        8,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qcauchy\0",
        None,
        9,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"df\0",
        None,
        10,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pf\0",
        None,
        11,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qf\0",
        None,
        12,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dgamma\0",
        None,
        13,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pgamma\0",
        None,
        14,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qgamma\0",
        None,
        15,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dlnorm\0",
        None,
        16,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"plnorm\0",
        None,
        17,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qlnorm\0",
        None,
        18,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dlogis\0",
        None,
        19,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"plogis\0",
        None,
        20,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qlogis\0",
        None,
        21,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dnbinom\0",
        None,
        22,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pnbinom\0",
        None,
        23,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qnbinom\0",
        None,
        24,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dnorm\0",
        None,
        25,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pnorm\0",
        None,
        26,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qnorm\0",
        None,
        27,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dunif\0",
        None,
        28,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"punif\0",
        None,
        29,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qunif\0",
        None,
        30,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dweibull\0",
        None,
        31,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pweibull\0",
        None,
        32,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qweibull\0",
        None,
        33,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dnchisq\0",
        None,
        34,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pnchisq\0",
        None,
        35,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qnchisq\0",
        None,
        36,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dnt\0",
        None,
        37,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pnt\0",
        None,
        38,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qnt\0",
        None,
        39,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dwilcox\0",
        None,
        40,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pwilcox\0",
        None,
        41,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qwilcox\0",
        None,
        42,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"besselI\0",
        None,
        43,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"besselK\0",
        None,
        44,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dnbinom_mu\0",
        None,
        45,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pnbinom_mu\0",
        None,
        46,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qnbinom_mu\0",
        None,
        47,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Math4 functions
    FunTabEntry::new(
        b"dhyper\0",
        None,
        1,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"phyper\0",
        None,
        2,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qhyper\0",
        None,
        3,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dnbeta\0",
        None,
        4,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pnbeta\0",
        None,
        5,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qnbeta\0",
        None,
        6,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dnf\0",
        None,
        7,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pnf\0",
        None,
        8,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qnf\0",
        None,
        9,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dtukey\0",
        None,
        10,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"ptukey\0",
        None,
        11,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qtukey\0",
        None,
        12,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Random number generators
    FunTabEntry::new(
        b"rchisq\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rexp\0",
        None,
        1,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rgeom\0",
        None,
        2,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rpois\0",
        None,
        3,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(b"rt\0", None, 4, 11, 2, PPinfo::new(PP_FUNCALL, PREC_FN, 0)),
    FunTabEntry::new(
        b"rsignrank\0",
        None,
        5,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rbeta\0",
        None,
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rbinom\0",
        None,
        1,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rcauchy\0",
        None,
        2,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(b"rf\0", None, 3, 11, 3, PPinfo::new(PP_FUNCALL, PREC_FN, 0)),
    FunTabEntry::new(
        b"rgamma\0",
        None,
        4,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rlnorm\0",
        None,
        5,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rlogis\0",
        None,
        6,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rnbinom\0",
        None,
        7,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rnchisq\0",
        None,
        12,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rnorm\0",
        None,
        8,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"runif\0",
        None,
        9,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rweibull\0",
        None,
        10,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rwilcox\0",
        None,
        11,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rnbinom_mu\0",
        None,
        13,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rhyper\0",
        None,
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sample\0",
        None,
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sample2\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"RNGkind\0",
        None,
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"set.seed\0",
        None,
        0,
        111,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Data Summaries
    FunTabEntry::new(
        b"sum\0",
        None,
        0,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"min\0",
        None,
        2,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"max\0",
        None,
        3,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"prod\0",
        None,
        4,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"mean\0",
        None,
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"range\0",
        None,
        0,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"cumsum\0",
        None,
        1,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"cumprod\0",
        None,
        2,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"cummax\0",
        None,
        3,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"cummin\0",
        None,
        4,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Type coercion
    FunTabEntry::new(
        b"as.character\0",
        None,
        0,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.integer\0",
        None,
        1,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.double\0",
        None,
        2,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.numeric\0",
        None,
        2,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.complex\0",
        None,
        3,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.logical\0",
        None,
        4,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.raw\0",
        None,
        5,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.call\0",
        None,
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.environment\0",
        None,
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"storage.mode<-\0",
        None,
        0,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"asCharacterFactor\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.vector\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"paste\0",
        None,
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"paste0\0",
        None,
        1,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.path\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"format\0",
        None,
        0,
        11,
        9,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"format.info\0",
        None,
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"cat\0",
        None,
        0,
        111,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"do.call\0",
        None,
        0,
        211,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"str2lang\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"str2expression\0",
        None,
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // String Manipulation
    FunTabEntry::new(
        b"nchar\0",
        None,
        1,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"nzchar\0",
        None,
        1,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"substr\0",
        None,
        1,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"startsWith\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"endsWith\0",
        None,
        1,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"substr<-\0",
        None,
        1,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"strsplit\0",
        None,
        1,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"abbreviate\0",
        None,
        1,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"make.names\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pcre_config\0",
        None,
        1,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"grep\0",
        None,
        0,
        11,
        8,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"grepl\0",
        None,
        1,
        11,
        8,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"grepRaw\0",
        None,
        0,
        11,
        8,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sub\0",
        None,
        0,
        11,
        7,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"gsub\0",
        None,
        1,
        11,
        7,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"regexpr\0",
        None,
        0,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"gregexpr\0",
        None,
        1,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"regexec\0",
        None,
        1,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"agrep\0",
        None,
        0,
        11,
        8,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"agrepl\0",
        None,
        1,
        11,
        8,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"adist\0",
        None,
        1,
        11,
        8,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"aregexec\0",
        None,
        1,
        11,
        7,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"tolower\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"toupper\0",
        None,
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"chartr\0",
        None,
        1,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sprintf\0",
        None,
        1,
        11,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"make.unique\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"charToRaw\0",
        None,
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rawToChar\0",
        None,
        1,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rawShift\0",
        None,
        1,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"intToBits\0",
        None,
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"numToBits\0",
        None,
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"numToInts\0",
        None,
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rawToBits\0",
        None,
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"packBits\0",
        None,
        1,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"utf8ToInt\0",
        None,
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"intToUtf8\0",
        None,
        1,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"validUTF8\0",
        None,
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"validEnc\0",
        None,
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"encodeString\0",
        None,
        1,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"iconv\0",
        None,
        0,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"strtrim\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"strtoi\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"strrep\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Type Checking
    FunTabEntry::new(
        b"is.null\0",
        None,
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.logical\0",
        None,
        10,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.integer\0",
        None,
        13,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.double\0",
        None,
        14,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.complex\0",
        None,
        15,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.character\0",
        None,
        16,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.symbol\0",
        None,
        1,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.name\0",
        None,
        1,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.environment\0",
        None,
        5,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.list\0",
        None,
        19,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.pairlist\0",
        None,
        2,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.expression\0",
        None,
        20,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.raw\0",
        None,
        24,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.object\0",
        None,
        50,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"isS4\0",
        None,
        51,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.numeric\0",
        None,
        100,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.matrix\0",
        None,
        101,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.array\0",
        None,
        102,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.atomic\0",
        None,
        200,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.recursive\0",
        None,
        201,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.call\0",
        None,
        300,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.language\0",
        None,
        301,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.function\0",
        None,
        302,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.single\0",
        None,
        999,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.na\0",
        None,
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.nan\0",
        None,
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.finite\0",
        None,
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.infinite\0",
        None,
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.vector\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Miscellaneous
    FunTabEntry::new(
        b"proc.time\0",
        None,
        0,
        1,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"gc.time\0",
        None,
        0,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"withVisible\0",
        None,
        1,
        10,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"expression\0",
        None,
        1,
        0,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"interactive\0",
        None,
        0,
        1,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"invisible\0",
        None,
        0,
        101,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rep\0",
        None,
        0,
        0,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rep.int\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rep_len\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"seq.int\0",
        None,
        0,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"seq_len\0",
        None,
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"seq_along\0",
        None,
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"list\0",
        None,
        1,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"xtfrm\0",
        None,
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"enc2native\0",
        None,
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"enc2utf8\0",
        None,
        1,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"emptyenv\0",
        None,
        0,
        1,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"baseenv\0",
        None,
        0,
        1,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"globalenv\0",
        None,
        0,
        1,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"environment<-\0",
        None,
        0,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"pos.to.env\0",
        None,
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(b".C\0", None, 0, 1, -1, PPinfo::new(PP_FOREIGN, PREC_FN, 0)),
    FunTabEntry::new(
        b".Fortran\0",
        None,
        1,
        1,
        -1,
        PPinfo::new(PP_FOREIGN, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".External\0",
        None,
        0,
        1,
        -1,
        PPinfo::new(PP_FOREIGN, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".External2\0",
        None,
        1,
        201,
        -1,
        PPinfo::new(PP_FOREIGN, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".Call\0",
        None,
        0,
        1,
        -1,
        PPinfo::new(PP_FOREIGN, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".External.graphics\0",
        None,
        0,
        1,
        -1,
        PPinfo::new(PP_FOREIGN, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".Call.graphics\0",
        None,
        0,
        1,
        -1,
        PPinfo::new(PP_FOREIGN, PREC_FN, 0),
    ),
    // More .Internals
    FunTabEntry::new(
        b"eapply\0",
        None,
        0,
        10,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"lapply\0",
        None,
        0,
        10,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"vapply\0",
        None,
        0,
        10,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"mapply\0",
        None,
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Version\0",
        None,
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"machine\0",
        None,
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"commandArgs\0",
        None,
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"internalsID\0",
        None,
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"system\0",
        None,
        0,
        211,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"parse\0",
        None,
        0,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"save\0",
        None,
        0,
        111,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"load\0",
        None,
        0,
        111,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"deparse\0",
        None,
        0,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"quit\0",
        None,
        0,
        111,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"readline\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"print.default\0",
        None,
        0,
        111,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(b"gc\0", None, 0, 11, 3, PPinfo::new(PP_FUNCALL, PREC_FN, 0)),
    FunTabEntry::new(
        b"gcinfo\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"split\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(b"ls\0", None, 1, 11, 3, PPinfo::new(PP_FUNCALL, PREC_FN, 0)),
    FunTabEntry::new(
        b"typeof\0",
        None,
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"eval\0",
        None,
        0,
        211,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sort\0",
        None,
        1,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"radixsort\0",
        None,
        0,
        11,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qsort\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"order\0",
        None,
        0,
        11,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"scan\0",
        None,
        0,
        11,
        19,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"t.default\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"options\0",
        None,
        0,
        211,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"getOption\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"inspect\0",
        None,
        0,
        111,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"capabilities\0",
        None,
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"new.env\0",
        None,
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.time\0",
        None,
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.POSIXct\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.POSIXlt\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"format.POSIXlt\0",
        None,
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"strptime\0",
        None,
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Date2POSIXlt\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"POSIXlt2Date\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"balancePOSIXlt\0",
        None,
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"polyroot\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"inherits\0",
        None,
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"UseMethod\0",
        None,
        0,
        200,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"NextMethod\0",
        None,
        0,
        210,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"standardGeneric\0",
        None,
        0,
        201,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"compareNumericVersion\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // OS interaction
    FunTabEntry::new(
        b"file.show\0",
        None,
        0,
        111,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.create\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.remove\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.rename\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.append\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.symlink\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.link\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.copy\0",
        None,
        0,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"list.files\0",
        None,
        0,
        11,
        9,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"list.dirs\0",
        None,
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.exists\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.choose\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.info\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.access\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dir.exists\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dir.create\0",
        None,
        0,
        111,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"R.home\0",
        None,
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"date\0",
        None,
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.getenv\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.which\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.getlocale\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.setlocale\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.localeconv\0",
        None,
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"path.expand\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.getpid\0",
        None,
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"unlink\0",
        None,
        0,
        111,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.sleep\0",
        None,
        0,
        111,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.info\0",
        None,
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.chmod\0",
        None,
        0,
        111,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.umask\0",
        None,
        0,
        211,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.readlink\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"l10n_info\0",
        None,
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Cstack_info\0",
        None,
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"extSoftVersion\0",
        None,
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"mkjunction\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Unimplemented entries for remaining functions
    FunTabEntry::new(
        b"gctorture\0",
        None,
        0,
        111,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"gctorture2\0",
        None,
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"memory.profile\0",
        None,
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"mem.maxVSize\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"mem.maxNSize\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"builtins\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"args\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"formals\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"body\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"environment\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"getenv\0",
        None,
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.setenv\0",
        None,
        0,
        111,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.unsetenv\0",
        None,
        0,
        111,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"getwd\0",
        None,
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"setwd\0",
        None,
        0,
        111,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"basename\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dirname\0",
        None,
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"readDCF\0",
        None,
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"bitwiseAnd\0",
        None,
        1,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"bitwiseNot\0",
        None,
        2,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"bitwiseOr\0",
        None,
        3,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"bitwiseXor\0",
        None,
        4,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"bitwiseShiftL\0",
        None,
        5,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"bitwiseShiftR\0",
        None,
        6,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Sentinel
    FunTabEntry::new(
        NULL_NAME,
        None,
        0,
        0,
        0,
        PPinfo::new(PP_INVALID, PREC_FN, 0),
    ),
];

// ---------------------------------------------------------------------------
// Spec_name table
// ---------------------------------------------------------------------------

/// Special names that are marked with SET_SPECIAL_SYMBOL.
static SPEC_NAMES: &[&[u8]] = &[
    b"if\0",
    b"while\0",
    b"repeat\0",
    b"for\0",
    b"break\0",
    b"next\0",
    b"return\0",
    b"function\0",
    b"(\0",
    b"{\0",
    b"+\0",
    b"-\0",
    b"*\0",
    b"/\0",
    b"^\0",
    b"%%\0",
    b"%/%\0",
    b"%*%\0",
    b":\0",
    b"::\0",
    b":::\0",
    b"?\0",
    b"|>\0",
    b"~\0",
    b"@\0",
    b"=>\0",
    b"==\0",
    b"!=\0",
    b"<\0",
    b">\0",
    b"<=\0",
    b">=\0",
    b"&\0",
    b"|\0",
    b"&&\0",
    b"||\0",
    b"!\0",
    b"<-\0",
    b"<<-\0",
    b"=\0",
    b"$\0",
    b"[\0",
    b"[[\0",
    b"$<-\0",
    b"[<-\0",
    b"[[<-\0",
];

// ---------------------------------------------------------------------------
// R_Primitive
// ---------------------------------------------------------------------------

/// Look up a primitive function by name.
/// Returns a BUILTINSXP/SPECIALSXP for primitives, R_NilValue for .Internal functions.
pub unsafe fn R_Primitive(primname: *const c_char) -> SEXP {
    unsafe {
        if primname.is_null() {
            return R_NilValue();
        }
        let name_str = std::ffi::CStr::from_ptr(primname);
        let name = name_str.to_bytes();

        for (idx, entry) in R_FunTab.iter().enumerate() {
            if entry.is_sentinel() {
                break;
            }
            // Extract name bytes (without null terminator)
            let entry_name = {
                let mut end = entry.name.len();
                if end > 0 && entry.name[end - 1] == 0 {
                    end -= 1;
                }
                &entry.name[..end]
            };
            if entry_name == name {
                if (entry.eval % 100) / 10 != 0 {
                    // It's a .Internal
                    return R_NilValue();
                } else {
                    return mkPRIMSXP(idx as c_int, entry.eval % 10);
                }
            }
        }
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_primitive
// ---------------------------------------------------------------------------

/// Implementation of .Primitive()
/// Looks up a primitive function by name and returns it as a SPECIALSXP or BUILTINSXP.
pub unsafe fn do_primitive(call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let name = CAR(args);
        // Check that name is a single string
        if TYPEOF(name) != SEXPTYPE::STRSXP || LENGTH(name) != 1 {
            panic_any(RError {
                message: "string argument required".to_string(),
            });
        }
        let name_elt = STRING_ELT(name, 0);
        if name_elt.is_null() || name_elt == R_NilValue() {
            panic_any(RError {
                message: "string argument required".to_string(),
            });
        }
        let name_c = CHAR(name_elt);
        if name_c.is_null() {
            panic_any(RError {
                message: "string argument required".to_string(),
            });
        }
        let prim = R_Primitive(name_c);
        if prim.is_null() || prim == R_NilValue() {
            panic_any(RError {
                message: format!(
                    "no such primitive function '{}'",
                    std::ffi::CStr::from_ptr(name_c).to_str().unwrap_or("?")
                ),
            });
        }
        prim
    }
}

// ---------------------------------------------------------------------------
// StrToInternal
// ---------------------------------------------------------------------------

/// Convert a function name to its index in R_FunTab, or NA_INTEGER if not found.
pub unsafe fn StrToInternal(s: *const c_char) -> c_int {
    unsafe {
        if s.is_null() {
            return NA_INTEGER;
        }
        let name_str = std::ffi::CStr::from_ptr(s);
        let name = name_str.to_bytes();

        for (i, entry) in R_FunTab.iter().enumerate() {
            if entry.is_sentinel() {
                break;
            }
            let entry_name = {
                let mut end = entry.name.len();
                if end > 0 && entry.name[end - 1] == 0 {
                    end -= 1;
                }
                &entry.name[..end]
            };
            if entry_name == name {
                return i as c_int;
            }
        }
        NA_INTEGER
    }
}

// ---------------------------------------------------------------------------
// installFunTab
// ---------------------------------------------------------------------------

/// Install function i from the FunTab into the symbol table.
unsafe fn installFunTab(i: usize) {
    unsafe {
        let entry = &R_FunTab[i];
        let entry_name = {
            let mut end = entry.name.len();
            if end > 0 && entry.name[end - 1] == 0 {
                end -= 1;
            }
            &entry.name[..end]
        };

        let name_cstr = std::ffi::CString::new(entry_name).unwrap_or_default();
        let sym = Rf_install(name_cstr.as_ptr());

        let prim = mkPRIMSXP(i as c_int, entry.eval % 10);

        if (entry.eval % 100) / 10 != 0 {
            // .Internal: store in internal slot
            SET_INTERNAL(sym, prim);
        } else {
            // Primitive: store as value
            SET_SYMVALUE(sym, prim);
        }
    }
}

// ---------------------------------------------------------------------------
// SymbolShortcuts: install commonly-used symbol shortcuts
// ---------------------------------------------------------------------------

/// Install all the symbol shortcuts used by the interpreter.
/// These are global pointers to frequently accessed symbols.
pub unsafe fn SymbolShortcuts() {
    unsafe {
        // These use Rf_install which is our HashMap-based implementation.
        // The results are cached in the symbol table.
        let _ = Rf_install(b"[[\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"[\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"{\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"class\0".as_ptr() as *const c_char);
        let _ = Rf_install(b".Device\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"dimnames\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"dim\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"$\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"@\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"...\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"drop\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"eval\0".as_ptr() as *const c_char);
        let _ = Rf_install(b".Last.value\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"levels\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"mode\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"name\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"names\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"na.rm\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"package\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"previous\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"quote\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"row.names\0".as_ptr() as *const c_char);
        let _ = Rf_install(b".Random.seed\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"sort.list\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"source\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"tsp\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"comment\0".as_ptr() as *const c_char);
        let _ = Rf_install(b".Environment\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"exact\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"recursive\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"srcfile\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"srcref\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"wholeSrcref\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"*tmp*\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"use.names\0".as_ptr() as *const c_char);
        let _ = Rf_install(b":\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"::\0".as_ptr() as *const c_char);
        let _ = Rf_install(b":::\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"conn_id\0".as_ptr() as *const c_char);
        let _ = Rf_install(b".Devices\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"base\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"spec\0".as_ptr() as *const c_char);
        let _ = Rf_install(b".__NAMESPACE__.\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"as.character\0".as_ptr() as *const c_char);
        let _ = Rf_install(b"function\0".as_ptr() as *const c_char);
        let _ = Rf_install(b".Generic\0".as_ptr() as *const c_char);
        let _ = Rf_install(b".Method\0".as_ptr() as *const c_char);
        let _ = Rf_install(b".Methods\0".as_ptr() as *const c_char);
        let _ = Rf_install(b".defined\0".as_ptr() as *const c_char);
        let _ = Rf_install(b".target\0".as_ptr() as *const c_char);
        let _ = Rf_install(b".Group\0".as_ptr() as *const c_char);
        let _ = Rf_install(b".Class\0".as_ptr() as *const c_char);
        let _ = Rf_install(b".GenericCallEnv\0".as_ptr() as *const c_char);
        let _ = Rf_install(b".GenericDefEnv\0".as_ptr() as *const c_char);
        let _ = Rf_install(b".packageName\0".as_ptr() as *const c_char);
    }
}

// ---------------------------------------------------------------------------
// DDVALSymbols
// ---------------------------------------------------------------------------

const N_DDVAL_SYMBOLS: usize = 65;

/// Get or create a DDVAL symbol for index n.
pub unsafe fn installDDVAL(n: c_int) -> SEXP {
    unsafe {
        if n >= 0 {
            let n = n as usize;
            let cached = instance::with_required_current_instance(|inst| {
                inst.names_state.ddval_symbols.get(n).copied()
            });
            if let Some(sym) = cached {
                return sym;
            }
        }

        if n >= 0 && (n as usize) < N_DDVAL_SYMBOLS {
            let mut symbols = Vec::with_capacity(N_DDVAL_SYMBOLS);
            for i in 0..N_DDVAL_SYMBOLS {
                let name = format!("..{}", i);
                let sym = Rf_install(std::ffi::CString::new(name).unwrap_or_default().as_ptr());
                symbols.push(sym);
            }
            let sym = symbols[n as usize];
            instance::with_required_current_instance(|inst| {
                if inst.names_state.ddval_symbols.len() < N_DDVAL_SYMBOLS {
                    inst.names_state.ddval_symbols = symbols;
                }
            });
            return sym;
        }

        let name = format!("..{}", n);
        Rf_install(std::ffi::CString::new(name).unwrap_or_default().as_ptr())
    }
}

// ---------------------------------------------------------------------------
// mkSymMarker
// ---------------------------------------------------------------------------

/// Create a symbol marker (SYMSXP whose value points to itself).
unsafe fn mkSymMarker(pname: SEXP) -> SEXP {
    unsafe {
        let sym = with_arena(|arena| arena.alloc_node(SEXPTYPE::SYMSXP));
        SET_SYMVALUE(sym, sym);
        SET_ATTRIB(sym, R_NilValue());
        SET_PRINTNAME(sym, pname);
        sym
    }
}

// ---------------------------------------------------------------------------
// InitNames: initialize the symbol table
// ---------------------------------------------------------------------------

/// Initialize the R symbol table.
/// This must be called once before any symbol lookup operations.
pub unsafe fn InitNames() {
    unsafe {
        if instance::with_required_current_instance(|inst| inst.names_state.init_names_done) {
            return;
        }

        // Create marker values
        // (R_UnboundValue, R_MissingArg, etc. are in globals.rs)

        // Initialize the symbol table shortcuts
        SymbolShortcuts();

        // Install all builtin functions from the FunTab
        for (i, entry) in R_FunTab.iter().enumerate() {
            if entry.is_sentinel() {
                break;
            }
            installFunTab(i);
        }

        // Initialize pre-allocated DDVAL symbols for this instance.
        let _ = installDDVAL(0);

        instance::with_required_current_instance(|inst| {
            inst.names_state.init_names_done = true;
        });
    }
}

// ---------------------------------------------------------------------------
// installS3Signature
// ---------------------------------------------------------------------------

/// Create an S3 method signature by concatenating className.methodName
/// and installing it in the symbol table.
pub unsafe fn installS3Signature(className: *const c_char, methodName: *const c_char) -> SEXP {
    unsafe {
        if className.is_null() || methodName.is_null() {
            return ptr::null_mut();
        }

        let class_str = std::ffi::CStr::from_ptr(className);
        let method_str = std::ffi::CStr::from_ptr(methodName);

        let max_len: usize = 512;
        let mut sig = Vec::with_capacity(max_len);
        sig.extend_from_slice(class_str.to_bytes());
        if sig.len() >= max_len {
            return ptr::null_mut();
        }
        sig.push(b'.');
        if sig.len() >= max_len {
            return ptr::null_mut();
        }
        sig.extend_from_slice(method_str.to_bytes());
        if sig.len() >= max_len {
            return ptr::null_mut();
        }

        let sig_cstr = std::ffi::CString::new(sig).unwrap_or_default();
        Rf_install(sig_cstr.as_ptr())
    }
}

// ---------------------------------------------------------------------------
// do_internal
// ---------------------------------------------------------------------------

/// Implementation of .Internal()
/// Looks up an internal function and dispatches to its C implementation.
pub unsafe fn do_internal(call: SEXP, _op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let s = CAR(args);
        // s is the unevaluated call supplied to .Internal, represented as a
        // language object in ordinary source and as a pairlist in a few
        // low-level call paths.
        if s.is_null() || (TYPEOF(s) != SEXPTYPE::LISTSXP && TYPEOF(s) != SEXPTYPE::LANGSXP) {
            panic_any(RError {
                message: "invalid .Internal() argument".to_string(),
            });
        }
        let fun = CAR(s);
        // fun must be a symbol
        if fun.is_null() || TYPEOF(fun) != SEXPTYPE::SYMSXP {
            panic_any(RError {
                message: "invalid .Internal() argument".to_string(),
            });
        }
        let pname = PRINTNAME(fun);
        let name_str = if !pname.is_null() {
            let pc = CHAR(pname);
            if !pc.is_null() {
                std::ffi::CStr::from_ptr(pc)
                    .to_str()
                    .unwrap_or("?")
                    .to_string()
            } else {
                "?".to_string()
            }
        } else {
            "?".to_string()
        };

        let mut internal_val = crate::sexp::accessors::INTERNAL(fun);
        if internal_val.is_null() || internal_val == R_NilValue() {
            let name_cstr = std::ffi::CString::new(name_str.as_str()).unwrap_or_default();
            let idx = StrToInternal(name_cstr.as_ptr());
            if idx != NA_INTEGER {
                let entry = &R_FunTab[idx as usize];
                if (entry.eval % 100) / 10 != 0 {
                    internal_val = mkPRIMSXP(idx, entry.eval % 10);
                }
            }
        }
        if internal_val.is_null() || internal_val == R_NilValue() {
            panic_any(RError {
                message: format!("there is no .Internal function '{}'", name_str),
            });
        }

        // Get the actual arguments (CDR of the pairlist)
        let actual_args = CDR(s);

        // For BUILTINSXP, evaluate the argument list; for SPECIALSXP, pass as-is
        let evaluated_args = if TYPEOF(internal_val) == SEXPTYPE::BUILTINSXP {
            crate::eval::dispatch::evalList(actual_args, env, call, -1)
        } else {
            actual_args
        };
        let _evaluated_args_guard = protect(evaluated_args);

        // Get the PRIMPRINT flag (visibility hint)
        let flag = crate::eval::eval::PRIMPRINT(internal_val);
        // Set R_Visible: flag != 1 means visible
        crate::sexp::globals::set_R_Visible(if flag != 1 { 1 } else { 0 });

        let offset = PRIMOFFSET(internal_val);
        let entry = &R_FunTab[offset as usize];
        let mut end = entry.name.len();
        if end > 0 && entry.name[end - 1] == 0 {
            end -= 1;
        }
        let name = std::str::from_utf8(&entry.name[..end]).unwrap_or("<invalid>");

        if let Some(handler) = internal_builtin_handler(name) {
            let ans = handler(s, internal_val, evaluated_args, env);
            if flag < 2 {
                crate::sexp::globals::set_R_Visible(if flag != 1 { 1 } else { 0 });
            }
            return ans;
        }

        if let Some(handler) = crate::eval::builtin::evaluated_builtin_handler(name) {
            let ans = handler(s, internal_val, evaluated_args, env);
            if flag < 2 {
                crate::sexp::globals::set_R_Visible(if flag != 1 { 1 } else { 0 });
            }
            return ans;
        }

        // Get the function pointer from the FunTab via offset.
        let cfun = entry.cfun;

        let ans = if let Some(f) = cfun {
            f(s, internal_val, evaluated_args, env)
        } else {
            panic_any(RError {
                message: format!("internal function '{name}' is not implemented"),
            });
        };

        // Reset visibility if flag < 2
        if flag < 2 {
            crate::sexp::globals::set_R_Visible(if flag != 1 { 1 } else { 0 });
        }

        ans
    }
}

type InternalBuiltinHandler = unsafe fn(SEXP, SEXP, SEXP, SEXP) -> SEXP;

fn internal_builtin_handler(name: &str) -> Option<InternalBuiltinHandler> {
    match name {
        "builtins" => Some(do_builtins),
        "stop" => Some(crate::mainutils::errors::do_stop_internal),
        "warning" => Some(crate::mainutils::errors::do_warning),
        "gettext" => Some(crate::mainutils::errors::do_gettext),
        "ngettext" => Some(crate::mainutils::errors::do_ngettext),
        "bindtextdomain" => Some(crate::mainutils::errors::do_bindtextdomain),
        ".dfltStop" => Some(crate::mainutils::errors::do_dfltStop),
        ".dfltWarn" => Some(crate::mainutils::errors::do_dfltWarn),
        "geterrmessage" => Some(crate::mainutils::errors::do_geterrmessage),
        "seterrmessage" => Some(crate::mainutils::errors::do_seterrmessage),
        "printDeferredWarnings" => Some(crate::mainutils::errors::do_printDeferredWarnings),
        "interruptsSuspended" => Some(crate::mainutils::errors::do_interruptsSuspended),
        "debug" | "undebug" | "isdebugged" | "debugonce" => Some(crate::mainutils::debug::do_debug),
        "delayedAssign" => Some(crate::mainutils::builtin::do_delayed),
        _ => None,
    }
}

/// R's `.Internal(builtins(internal))` — sorted builtin/internal name listing.
pub unsafe fn do_builtins(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let internal = if args.is_null() || args == R_NilValue() || LENGTH(args) == 0 {
            false
        } else {
            let first = CAR(args);
            if first.is_null() || first == R_NilValue() || LENGTH(first) == 0 {
                false
            } else if TYPEOF(first) == SEXPTYPE::LGLSXP || TYPEOF(first) == SEXPTYPE::INTSXP {
                let value = *INTEGER(first);
                value != FALSE && value != NA_INTEGER
            } else {
                false
            }
        };

        let mut names = builtin_names_from_funtab(internal);
        names.sort();
        names.dedup();
        string_vector_from_bytes(&names)
    }
}

fn builtin_names_from_funtab(internal: bool) -> Vec<Vec<u8>> {
    R_FunTab
        .iter()
        .take_while(|entry| !entry.is_sentinel())
        .filter(|entry| {
            let is_internal = (entry.eval % 100) / 10 != 0;
            !internal || is_internal
        })
        .map(|entry| {
            let mut end = entry.name.len();
            if end > 0 && entry.name[end - 1] == 0 {
                end -= 1;
            }
            entry.name[..end].to_vec()
        })
        .collect()
}

unsafe fn string_vector_from_bytes(values: &[Vec<u8>]) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, values.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _guard = protect(result);
        for (i, value) in values.iter().enumerate() {
            let cstr = std::ffi::CString::new(value.as_slice()).unwrap_or_default();
            SET_STRING_ELT(result, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }
        result
    }
}

// ---------------------------------------------------------------------------
// do_tilde (stub)
// ---------------------------------------------------------------------------

/// Implementation of the ~ operator.
/// Creates a formula object with class "formula" and .Environment attribute.
pub unsafe fn do_tilde(call: SEXP, _op: SEXP, _args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if !call.is_null() && OBJECT(call) != 0 {
            duplicate(call)
        } else {
            let klass = Rf_mkString(b"formula\0".as_ptr() as *const c_char);
            let _klass_guard = protect(klass);

            let result = duplicate(call);
            let _result_guard = protect(result);

            let class_sym = Rf_install(b"class\0".as_ptr() as *const c_char);
            setAttrib(result, class_sym, klass);

            let dot_env_sym = Rf_install(b".Environment\0".as_ptr() as *const c_char);
            setAttrib(result, dot_env_sym, rho);

            result
        }
    }
}

// ---------------------------------------------------------------------------
// getPRIMNAME
// ---------------------------------------------------------------------------

/// Get the name of a primitive function.
/// For use in packages.
pub unsafe fn getPRIMNAME(object: SEXP) -> *const c_char {
    unsafe {
        if object.is_null() {
            return ptr::null();
        }
        let t = TYPEOF(object);
        if t != SEXPTYPE::SPECIALSXP && t != SEXPTYPE::BUILTINSXP {
            return ptr::null();
        }
        let offset = PRIMOFFSET(object) as usize;
        if offset >= R_FunTab.len() || R_FunTab[offset].is_sentinel() {
            return ptr::null();
        }
        // Return pointer to the name bytes (null-terminated C string)
        R_FunTab[offset].name.as_ptr() as *const c_char
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::sexp::accessors::*;
    use crate::sexp::constructors::*;
    use crate::sexp::session::RSession;

    use super::*;

    #[test]
    fn test_pp_kind_constants() {
        assert_eq!(PP_INVALID, 0);
        assert_eq!(PP_ASSIGN, 1);
        assert_eq!(PP_BINARY, 3);
        assert_eq!(PP_FUNCALL, 8);
        assert_eq!(PP_IF, 10);
        assert_eq!(PP_DOLLAR, 18);
        assert_eq!(PP_FOREIGN, 19);
        assert_eq!(PP_REPEAT, 20);
    }

    #[test]
    fn test_prec_constants() {
        assert_eq!(PREC_FN, 0);
        assert_eq!(PREC_LEFT, 2);
        assert_eq!(PREC_COMPARE, 8);
        assert_eq!(PREC_SUM, 9);
        assert_eq!(PREC_COLON, 12);
        assert_eq!(PREC_NS, 17);
    }

    #[test]
    fn test_funtab_not_empty() {
        assert!(R_FunTab.len() > 100);
    }

    #[test]
    fn test_funtab_sentinel() {
        let last = R_FunTab
            .last()
            .unwrap_or_else(|| panic!("unexpected None in test"));
        assert!(last.is_sentinel());
    }

    #[test]
    fn test_funtab_first_entry() {
        let first = &R_FunTab[0];
        let name = {
            let mut end = first.name.len();
            if end > 0 && first.name[end - 1] == 0 {
                end -= 1;
            }
            &first.name[..end]
        };
        assert_eq!(name, b"if");
        assert_eq!(first.eval, 200);
        assert_eq!(first.arity, -1);
    }

    #[test]
    fn test_spec_names() {
        assert!(SPEC_NAMES.len() > 40);
        assert_eq!(SPEC_NAMES[0], b"if\0");
    }

    #[test]
    fn test_r_primitive_not_found() {
        let _session = RSession::new();

        unsafe {
            let result = R_Primitive(b"nonexistent\0".as_ptr() as *const c_char);
            assert!(Rf_isNull(result) != 0 || result.is_null());
        }
    }

    #[test]
    fn test_r_primitive_is_internal() {
        let _session = RSession::new();

        unsafe {
            // "stop" is a .Internal (eval=11, 11/10=1)
            let result = R_Primitive(b"stop\0".as_ptr() as *const c_char);
            assert!(Rf_isNull(result) != 0 || result.is_null());
        }
    }

    #[test]
    fn test_str_to_internal_not_found() {
        unsafe {
            let result = StrToInternal(b"nonexistent\0".as_ptr() as *const c_char);
            assert_eq!(result, NA_INTEGER);
        }
    }

    #[test]
    fn test_str_to_internal_found() {
        unsafe {
            let result = StrToInternal(b"if\0".as_ptr() as *const c_char);
            assert_eq!(result, 0);
        }
    }

    #[test]
    fn test_str_to_internal_finds_sys_which() {
        unsafe {
            let result = StrToInternal(b"Sys.which\0".as_ptr() as *const c_char);
            assert_ne!(result, NA_INTEGER);
            let entry = &R_FunTab[result as usize];
            assert_eq!(entry.name, b"Sys.which\0");
            assert_eq!(entry.arity, 1);
            assert_eq!(entry.eval, 11);
        }
    }

    #[test]
    fn test_str_to_internal_finds_ext_soft_version() {
        unsafe {
            let result = StrToInternal(b"extSoftVersion\0".as_ptr() as *const c_char);
            assert_ne!(result, NA_INTEGER);
            let entry = &R_FunTab[result as usize];
            assert_eq!(entry.name, b"extSoftVersion\0");
            assert_eq!(entry.arity, 0);
            assert_eq!(entry.eval, 11);
            assert_eq!(
                StrToInternal(b"eSoftVersion\0".as_ptr() as *const c_char),
                NA_INTEGER
            );
        }
    }

    #[test]
    fn test_builtin_name_listing_uses_funtab() {
        let all = builtin_names_from_funtab(false);
        assert!(all.iter().any(|name| name == b"+"));
        assert!(all.iter().any(|name| name == b"Sys.which"));
        assert!(all.iter().any(|name| name == b"builtins"));

        let internal = builtin_names_from_funtab(true);
        assert!(internal.iter().any(|name| name == b"stop"));
        assert!(internal.iter().any(|name| name == b"builtins"));
        assert!(!internal.iter().any(|name| name == b"+"));
    }

    #[test]
    fn test_install_s3_signature() {
        let _session = RSession::new();

        unsafe {
            let sym = installS3Signature(
                b"foo\0".as_ptr() as *const c_char,
                b"bar\0".as_ptr() as *const c_char,
            );
            assert!(!sym.is_null());
        }
    }

    #[test]
    fn test_init_names() {
        let _session = RSession::new();

        unsafe {
            InitNames();
            // Verify that a few symbols exist in the table
            let sym_if = Rf_install(b"if\0".as_ptr() as *const c_char);
            assert!(!sym_if.is_null());
            let sym_colon = Rf_install(b":\0".as_ptr() as *const c_char);
            assert!(!sym_colon.is_null());
        }
    }

    #[test]
    fn test_init_names_idempotent() {
        let _session = RSession::new();

        unsafe {
            InitNames();
            InitNames();
            // Should not crash
        }
    }

    #[test]
    fn test_operator_constants() {
        assert_eq!(PLUSOP, 1);
        assert_eq!(MINUSOP, 2);
        assert_eq!(TIMESOP, 3);
        assert_eq!(DIVOP, 4);
        assert_eq!(POWOP, 5);
        assert_eq!(MODOP, 6);
        assert_eq!(IDIVOP, 7);
        assert_eq!(CTXT_BREAK, 2);
        assert_eq!(CTXT_NEXT, 1);
    }

    #[test]
    fn test_ppinfo() {
        let pp = PPinfo::new(PP_FUNCALL, PREC_FN, 0);
        assert_eq!(pp.kind, PP_FUNCALL);
        assert_eq!(pp.prec, PREC_FN);
        assert_eq!(pp.rightassoc, 0);
    }

    #[test]
    fn test_ddval_symbols() {
        let _session = RSession::new();

        unsafe {
            let sym0 = installDDVAL(0);
            assert!(!sym0.is_null());
            let sym64 = installDDVAL(64);
            assert!(!sym64.is_null());
            let sym65 = installDDVAL(65); // beyond pre-allocated
            assert!(!sym65.is_null());
        }
    }

    #[test]
    fn test_ddval_symbols_are_session_local_on_same_thread() {
        let mut left = RSession::new();
        let mut right = RSession::new();

        let mut left_sym0 = ptr::null_mut();
        left.with_arena(|_| unsafe {
            InitNames();
            left_sym0 = installDDVAL(0);
            assert_eq!(installDDVAL(0), left_sym0);
        })
        .unwrap();

        right
            .with_arena(|_| unsafe {
                InitNames();
                let right_sym0 = installDDVAL(0);
                assert_eq!(installDDVAL(0), right_sym0);
                assert_ne!(right_sym0, left_sym0);
            })
            .unwrap();

        left.with_arena(|_| unsafe {
            assert_eq!(installDDVAL(0), left_sym0);
        })
        .unwrap();
    }

    #[test]
    fn test_mk_sym_marker() {
        let _session = RSession::new();

        unsafe {
            let pname = Rf_mkChar(b"test\0".as_ptr() as *const c_char);
            let sym = mkSymMarker(pname);
            assert!(!sym.is_null());
            // Value should point to itself
            assert_eq!(crate::sexp::accessors::SYMVALUE(sym), sym);
        }
    }
}
