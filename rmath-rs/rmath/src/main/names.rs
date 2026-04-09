#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/names.c -- function dispatch table and symbol initialization.
//!
//! Implements the R_FunTab dispatch table (~940 entries), R_Primitive,
//! do_primitive, StrToInternal, installFunTab, SymbolShortcuts,
//! InitNames, installS3Signature, do_internal (stub), do_tilde (stub),
//! DDVALSymbols, installDDVAL, mkSymMarker, getPRIMNAME.

use std::os::raw::{c_char, c_int};
use std::panic::panic_any;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::attrib_core::setAttrib;
use crate::main::dstruct::mkPRIMSXP;
use crate::main::duplicate::duplicate;
use crate::sexp::accessors::{
    CAR, CDR, CHAR, LENGTH, OBJECT, PRIMOFFSET, PRINTNAME, SET_ATTRIB, SET_INTERNAL, SET_PRINTNAME,
    SET_SYMVALUE, STRING_ELT, TYPEOF,
};
use crate::sexp::constructors::Rf_mkString;
use crate::sexp::ffi::{NA_INTEGER, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory::with_arena;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::Rf_install;

// Import langutils functions for FunTab
use crate::main::langutils::{
    do_C_tryCatchHelper, do_Recall, do_Version, do_addGlobHands, do_agrepl, do_as_function_default,
    do_at_assign, do_browser, do_builtins, do_cache_class, do_call_fn, do_cat, do_class2,
    do_compareNumericVersion, do_debugonce, do_declare, do_delayedAssign, do_do_call, do_dot_elt,
    do_dot_length, do_dot_names, do_double_colon, do_eapply, do_environment_assign, do_eval_fn,
    do_forceAndCall, do_foreign_C, do_foreign_Call, do_foreign_Call_graphics, do_foreign_External,
    do_foreign_External_graphics, do_foreign_External2, do_foreign_Fortran, do_get0,
    do_getNamespaceValue, do_inspect, do_internalsID, do_isdebugged, do_length_assign, do_list2env,
    do_makeLazy, do_match_call, do_memory_profile, do_mget, do_missing, do_nargs, do_new_env,
    do_on_exit, do_order, do_parse_fn, do_polyroot_fn, do_pos_to_env, do_primTrace, do_primUntrace,
    do_quit, do_quote, do_readline, do_sample2, do_setFileTime, do_sort, do_storage_mode_assign,
    do_str2expression, do_str2lang, do_strsplit, do_substitute, do_substr_assign, do_switch,
    do_system, do_triple_colon, do_undebug, do_vector, do_vhash, do_xtfrm,
};
use crate::main::platform::do_gcinfo;
use crate::sexp::context::RError;

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
    pub(crate) fn is_sentinel(&self) -> bool {
        self.name.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Helper: null name bytes (empty slice used as sentinel)
// ---------------------------------------------------------------------------

const NULL_NAME: &[u8] = b"";

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
        Some(crate::main::subset::do_subset),
        1,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".subset2\0",
        Some(crate::main::subset::do_subset2),
        2,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"[\0",
        Some(crate::main::subset::do_subset),
        1,
        0,
        -1,
        PPinfo::new(PP_SUBSET, PREC_SUBSET, 0),
    ),
    FunTabEntry::new(
        b"[[\0",
        Some(crate::main::subset::do_subset2),
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
        Some(crate::main::subset::do_subassign),
        0,
        0,
        3,
        PPinfo::new(PP_SUBASS, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"[[<-\0",
        Some(crate::main::subset::do_subassign2),
        1,
        0,
        3,
        PPinfo::new(PP_SUBASS, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"$<-\0",
        Some(crate::main::subset::do_subassign3),
        1,
        0,
        3,
        PPinfo::new(PP_SUBASS, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"switch\0",
        Some(do_switch),
        0,
        200,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"browser\0",
        Some(do_browser),
        0,
        101,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".primTrace\0",
        Some(do_primTrace),
        0,
        101,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".primUntrace\0",
        Some(do_primUntrace),
        1,
        101,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".Internal\0",
        Some(crate::main::names::do_internal),
        0,
        200,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".Primitive\0",
        Some(crate::main::names::do_primitive),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"call\0",
        Some(do_call_fn),
        0,
        0,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"quote\0",
        Some(do_quote),
        0,
        0,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"substitute\0",
        Some(do_substitute),
        0,
        0,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"missing\0",
        Some(do_missing),
        1,
        0,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"nargs\0",
        Some(do_nargs),
        1,
        1,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"on.exit\0",
        Some(do_on_exit),
        0,
        100,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"forceAndCall\0",
        Some(do_forceAndCall),
        0,
        0,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"declare\0",
        Some(do_declare),
        0,
        100,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // .Internals (error handling, conditions, etc.)
    FunTabEntry::new(
        b"stop\0",
        Some(crate::main::errors::do_stop),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"warning\0",
        Some(crate::main::errors::do_warning),
        0,
        111,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"gettext\0",
        Some(crate::main::errors::do_gettext),
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"ngettext\0",
        Some(crate::main::errors::do_ngettext),
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"bindtextdomain\0",
        Some(crate::main::errors::do_bindtextdomain),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".addCondHands\0",
        Some(crate::main::errors::do_addCondHands),
        0,
        111,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".addGlobHands\0",
        Some(do_addGlobHands),
        0,
        111,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".resetCondHands\0",
        Some(crate::main::errors::do_resetCondHands),
        0,
        111,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".signalCondition\0",
        Some(crate::main::errors::do_signalCondition),
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".dfltStop\0",
        Some(crate::main::errors::do_dfltStop),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".dfltWarn\0",
        Some(crate::main::errors::do_dfltWarn),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".addRestart\0",
        Some(crate::main::errors::do_addRestart),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".getRestart\0",
        Some(crate::main::errors::do_getRestart),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".invokeRestart\0",
        Some(crate::main::errors::do_invokeRestart),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".addTryHandlers\0",
        Some(crate::main::errors::do_addTryHandlers),
        0,
        111,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"geterrmessage\0",
        Some(crate::main::errors::do_geterrmessage),
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"seterrmessage\0",
        Some(crate::main::errors::do_seterrmessage),
        0,
        111,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"printDeferredWarnings\0",
        Some(crate::main::errors::do_printDeferredWarnings),
        0,
        111,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"interruptsSuspended\0",
        Some(crate::main::errors::do_interruptsSuspended),
        0,
        11,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.function.default\0",
        Some(do_as_function_default),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCTION, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"debug\0",
        Some(crate::main::debug::do_debug),
        0,
        111,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"undebug\0",
        Some(do_undebug),
        1,
        111,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"isdebugged\0",
        Some(do_isdebugged),
        2,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"debugonce\0",
        Some(do_debugonce),
        3,
        111,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Recall\0",
        Some(do_Recall),
        0,
        210,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"delayedAssign\0",
        Some(do_delayedAssign),
        0,
        111,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"makeLazy\0",
        Some(do_makeLazy),
        0,
        111,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"identical\0",
        Some(crate::main::identical::do_identical),
        0,
        11,
        8,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"C_tryCatchHelper\0",
        Some(do_C_tryCatchHelper),
        0,
        11,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"getNamespaceValue\0",
        Some(do_getNamespaceValue),
        0,
        211,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // ===== Binary Operators (primitives) =====
    FunTabEntry::new(
        b"+\0",
        Some(crate::main::arithmetic::do_arith),
        PLUSOP,
        1,
        -1,
        PPinfo::new(PP_BINARY, PREC_SUM, 0),
    ),
    FunTabEntry::new(
        b"-\0",
        Some(crate::main::arithmetic::do_arith),
        MINUSOP,
        1,
        -1,
        PPinfo::new(PP_BINARY, PREC_SUM, 0),
    ),
    FunTabEntry::new(
        b"*\0",
        Some(crate::main::arithmetic::do_arith),
        TIMESOP,
        1,
        2,
        PPinfo::new(PP_BINARY, PREC_PROD, 0),
    ),
    FunTabEntry::new(
        b"/\0",
        Some(crate::main::arithmetic::do_arith),
        DIVOP,
        1,
        2,
        PPinfo::new(PP_BINARY2, PREC_PROD, 0),
    ),
    FunTabEntry::new(
        b"^\0",
        Some(crate::main::arithmetic::do_arith),
        POWOP,
        1,
        2,
        PPinfo::new(PP_BINARY2, PREC_POWER, 1),
    ),
    FunTabEntry::new(
        b"%%\0",
        Some(crate::main::arithmetic::do_arith),
        MODOP,
        1,
        2,
        PPinfo::new(PP_BINARY2, PREC_PERCENT, 0),
    ),
    FunTabEntry::new(
        b"%/%\0",
        Some(crate::main::arithmetic::do_arith),
        IDIVOP,
        1,
        2,
        PPinfo::new(PP_BINARY2, PREC_PERCENT, 0),
    ),
    FunTabEntry::new(
        b"%*%\0",
        Some(crate::main::arithmetic::do_arith),
        0,
        1,
        2,
        PPinfo::new(PP_BINARY, PREC_PERCENT, 0),
    ),
    // Comparison operators
    FunTabEntry::new(
        b"==\0",
        Some(crate::main::relop::do_relop),
        1,
        1,
        2,
        PPinfo::new(PP_BINARY, PREC_COMPARE, 0),
    ),
    FunTabEntry::new(
        b"!=\0",
        Some(crate::main::relop::do_relop),
        2,
        1,
        2,
        PPinfo::new(PP_BINARY, PREC_COMPARE, 0),
    ),
    FunTabEntry::new(
        b"<\0",
        Some(crate::main::relop::do_relop),
        3,
        1,
        2,
        PPinfo::new(PP_BINARY, PREC_COMPARE, 0),
    ),
    FunTabEntry::new(
        b"<=\0",
        Some(crate::main::relop::do_relop),
        5,
        1,
        2,
        PPinfo::new(PP_BINARY, PREC_COMPARE, 0),
    ),
    FunTabEntry::new(
        b">=\0",
        Some(crate::main::relop::do_relop),
        6,
        1,
        2,
        PPinfo::new(PP_BINARY, PREC_COMPARE, 0),
    ),
    FunTabEntry::new(
        b">\0",
        Some(crate::main::relop::do_relop),
        4,
        1,
        2,
        PPinfo::new(PP_BINARY, PREC_COMPARE, 0),
    ),
    // Logical operators
    FunTabEntry::new(
        b"&\0",
        Some(crate::main::logic::do_logic),
        1,
        1,
        2,
        PPinfo::new(PP_BINARY, PREC_AND, 0),
    ),
    FunTabEntry::new(
        b"|\0",
        Some(crate::main::logic::do_logic),
        2,
        1,
        2,
        PPinfo::new(PP_BINARY, PREC_OR, 0),
    ),
    FunTabEntry::new(
        b"!\0",
        Some(crate::main::logic::do_logic),
        3,
        1,
        1,
        PPinfo::new(PP_UNARY, PREC_NOT, 0),
    ),
    FunTabEntry::new(
        b"&&\0",
        Some(crate::main::logic::do_logic2),
        1,
        0,
        2,
        PPinfo::new(PP_BINARY, PREC_AND, 0),
    ),
    FunTabEntry::new(
        b"||\0",
        Some(crate::main::logic::do_logic2),
        2,
        0,
        2,
        PPinfo::new(PP_BINARY, PREC_OR, 0),
    ),
    // Colon and special operators
    FunTabEntry::new(
        b":\0",
        Some(crate::main::seq::do_colon),
        0,
        1,
        2,
        PPinfo::new(PP_BINARY2, PREC_COLON, 0),
    ),
    FunTabEntry::new(
        b"~\0",
        Some(crate::main::names::do_tilde),
        0,
        0,
        -1,
        PPinfo::new(PP_BINARY, PREC_TILDE, 0),
    ),
    FunTabEntry::new(
        b"::\0",
        Some(do_double_colon),
        0,
        200,
        2,
        PPinfo::new(PP_BINARY2, PREC_NS, 0),
    ),
    FunTabEntry::new(
        b":::\0",
        Some(do_triple_colon),
        0,
        200,
        2,
        PPinfo::new(PP_BINARY2, PREC_NS, 0),
    ),
    // ===== Logic Related Functions =====
    FunTabEntry::new(
        b"all\0",
        Some(crate::main::unique::do_all),
        1,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"any\0",
        Some(crate::main::unique::do_any),
        2,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // ===== Vectors, Matrices and Arrays =====
    // Primitives
    FunTabEntry::new(
        b"...elt\0",
        Some(do_dot_elt),
        0,
        201,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"...length\0",
        Some(do_dot_length),
        0,
        1,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"...names\0",
        Some(do_dot_names),
        0,
        1,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"length\0",
        Some(crate::main::inspect::do_length),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"length<-\0",
        Some(do_length_assign),
        0,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"c\0",
        Some(crate::main::bind::do_c),
        0,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"oldClass\0",
        Some(crate::main::objects::do_oldClass),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"oldClass<-\0",
        Some(crate::main::attrib::do_classgets),
        0,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"class\0",
        Some(crate::main::inspect::do_classname),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".cache_class\0",
        Some(do_cache_class),
        1,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".class2\0",
        Some(do_class2),
        2,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"class<-\0",
        Some(crate::main::attrib::do_classgets),
        0,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"unclass\0",
        Some(crate::main::print::do_unclass),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"names\0",
        Some(crate::main::inspect::do_names),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"names<-\0",
        Some(crate::main::attrib::do_namesgets),
        0,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"dimnames\0",
        Some(crate::main::inspect::do_dimnames),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dimnames<-\0",
        Some(crate::main::attrib::do_dimnamesgets),
        0,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"dim\0",
        Some(crate::main::inspect::do_dim),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dim<-\0",
        Some(crate::main::attrib::do_dimgets),
        0,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"attributes\0",
        Some(crate::main::inspect::do_attributes),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"attributes<-\0",
        Some(crate::main::attrib::do_attributesgets),
        0,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"attr\0",
        Some(crate::main::attrib::do_attr),
        0,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"attr<-\0",
        Some(crate::main::attrib::do_attrgets),
        0,
        1,
        3,
        PPinfo::new(PP_FUNCALL, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"@<-\0",
        Some(do_at_assign),
        1,
        0,
        3,
        PPinfo::new(PP_SUBASS, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"levels<-\0",
        Some(crate::main::attrib::do_levelsgets),
        0,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_LEFT, 1),
    ),
    // .Internals (vectors, matrices, arrays)
    FunTabEntry::new(
        b"vector\0",
        Some(do_vector),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"complex\0",
        Some(crate::main::complex_cmath::do_complex),
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"matrix\0",
        Some(crate::main::array::do_matrix),
        0,
        11,
        7,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"array\0",
        Some(crate::main::array::do_array),
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"diag\0",
        Some(crate::main::array::do_diag),
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"backsolve\0",
        Some(crate::main::array::do_backsolve),
        0,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"max.col\0",
        Some(crate::main::array::do_maxcol),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"row\0",
        Some(crate::main::array::do_rowscols),
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"col\0",
        Some(crate::main::array::do_rowscols),
        2,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"unlist\0",
        Some(crate::main::bind::do_unlist),
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"cbind\0",
        Some(crate::main::bind::do_cbind),
        1,
        10,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rbind\0",
        Some(crate::main::bind::do_rbind),
        2,
        10,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"drop\0",
        Some(crate::main::array::do_drop),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"all.names\0",
        Some(crate::main::list::do_allnames),
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"comment\0",
        Some(crate::main::attrib::do_comment),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"comment<-\0",
        Some(crate::main::attrib::do_commentgets),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"get\0",
        Some(crate::main::envir::do_get),
        1,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"get0\0",
        Some(do_get0),
        2,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"mget\0",
        Some(do_mget),
        1,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"exists\0",
        Some(crate::main::envir::do_exists),
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"assign\0",
        Some(crate::main::envir::do_assign),
        0,
        111,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"list2env\0",
        Some(do_list2env),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"remove\0",
        Some(crate::main::envir::do_remove),
        0,
        111,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"duplicated\0",
        Some(crate::main::unique::do_duplicated),
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"unique\0",
        Some(crate::main::unique::do_unique),
        1,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"anyDuplicated\0",
        Some(crate::main::unique::do_anyDuplicated),
        2,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"anyNA\0",
        Some(crate::main::unique::do_anyNA),
        0,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"which\0",
        Some(crate::main::unique::do_which),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"which.min\0",
        Some(crate::main::unique::do_which_min),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pmin\0",
        Some(crate::main::unique::do_pminmax),
        1,
        11,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pmax\0",
        Some(crate::main::unique::do_pminmax),
        0,
        11,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"which.max\0",
        Some(crate::main::unique::do_which_max),
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"match\0",
        Some(crate::main::match_mod::do_match),
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pmatch\0",
        Some(crate::main::match_mod::do_pmatch),
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"charmatch\0",
        Some(crate::main::match_mod::do_charmatch),
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"match.call\0",
        Some(do_match_call),
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"crossprod\0",
        Some(crate::main::array::do_matprod),
        1,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"tcrossprod\0",
        Some(crate::main::array::do_matprod),
        2,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"asplit\0",
        Some(crate::main::array::do_asplit),
        0,
        11,
        8,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"lengths\0",
        Some(crate::main::array::do_lengths),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sequence\0",
        Some(crate::main::seq::do_sequence),
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"vhash\0",
        Some(do_vhash),
        0,
        11,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"attach\0",
        Some(crate::main::envir::do_attach),
        0,
        111,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"detach\0",
        Some(crate::main::envir::do_detach),
        0,
        111,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"search\0",
        Some(crate::main::envir::do_search),
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"setFileTime\0",
        Some(do_setFileTime),
        0,
        111,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // ===== Mathematical Functions =====
    FunTabEntry::new(
        b"round\0",
        Some(crate::main::arithmetic::do_math2),
        10001,
        0,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"signif\0",
        Some(crate::main::arithmetic::do_math2),
        10004,
        0,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"log\0",
        Some(crate::main::arithmetic::do_math1),
        10003,
        0,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"log10\0",
        Some(crate::main::arithmetic::do_math1),
        10010,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"log2\0",
        Some(crate::main::arithmetic::do_math1),
        10002,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"abs\0",
        Some(crate::main::arithmetic::do_math1),
        6,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"floor\0",
        Some(crate::main::arithmetic::do_math1),
        1,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"ceiling\0",
        Some(crate::main::arithmetic::do_math1),
        2,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sqrt\0",
        Some(crate::main::arithmetic::do_math1),
        3,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sign\0",
        Some(crate::main::arithmetic::do_math1),
        4,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"trunc\0",
        Some(crate::main::arithmetic::do_math1),
        5,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"exp\0",
        Some(crate::main::arithmetic::do_math1),
        10,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"expm1\0",
        Some(crate::main::arithmetic::do_math1),
        11,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"log1p\0",
        Some(crate::main::arithmetic::do_math1),
        12,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"cos\0",
        Some(crate::main::arithmetic::do_math1),
        20,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sin\0",
        Some(crate::main::arithmetic::do_math1),
        21,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"tan\0",
        Some(crate::main::arithmetic::do_math1),
        22,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"acos\0",
        Some(crate::main::arithmetic::do_math1),
        23,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"asin\0",
        Some(crate::main::arithmetic::do_math1),
        24,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"atan\0",
        Some(crate::main::arithmetic::do_math1),
        25,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"cosh\0",
        Some(crate::main::arithmetic::do_math1),
        30,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sinh\0",
        Some(crate::main::arithmetic::do_math1),
        31,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"tanh\0",
        Some(crate::main::arithmetic::do_math1),
        32,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"acosh\0",
        Some(crate::main::arithmetic::do_math1),
        33,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"asinh\0",
        Some(crate::main::arithmetic::do_math1),
        34,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"atanh\0",
        Some(crate::main::arithmetic::do_math1),
        35,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"lgamma\0",
        Some(crate::main::arithmetic::do_math1),
        40,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"gamma\0",
        Some(crate::main::arithmetic::do_math1),
        41,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"digamma\0",
        Some(crate::main::arithmetic::do_math1),
        42,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"trigamma\0",
        Some(crate::main::arithmetic::do_math1),
        43,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"cospi\0",
        Some(crate::main::arithmetic::do_math1),
        47,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sinpi\0",
        Some(crate::main::arithmetic::do_math1),
        48,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"tanpi\0",
        Some(crate::main::arithmetic::do_math1),
        49,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Math2 functions
    FunTabEntry::new(
        b"atan2\0",
        Some(crate::main::arithmetic::do_math2),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"lbeta\0",
        Some(crate::main::arithmetic::do_math2),
        2,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"beta\0",
        Some(crate::main::arithmetic::do_math2),
        3,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"lchoose\0",
        Some(crate::main::arithmetic::do_math2),
        4,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"choose\0",
        Some(crate::main::arithmetic::do_math2),
        5,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dchisq\0",
        Some(crate::main::arithmetic::do_math2),
        6,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pchisq\0",
        Some(crate::main::arithmetic::do_math2),
        7,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qchisq\0",
        Some(crate::main::arithmetic::do_math2),
        8,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dexp\0",
        Some(crate::main::arithmetic::do_math2),
        9,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pexp\0",
        Some(crate::main::arithmetic::do_math2),
        10,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qexp\0",
        Some(crate::main::arithmetic::do_math2),
        11,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dgeom\0",
        Some(crate::main::arithmetic::do_math2),
        12,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pgeom\0",
        Some(crate::main::arithmetic::do_math2),
        13,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qgeom\0",
        Some(crate::main::arithmetic::do_math2),
        14,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dpois\0",
        Some(crate::main::arithmetic::do_math2),
        15,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"ppois\0",
        Some(crate::main::arithmetic::do_math2),
        16,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qpois\0",
        Some(crate::main::arithmetic::do_math2),
        17,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dt\0",
        Some(crate::main::arithmetic::do_math2),
        18,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pt\0",
        Some(crate::main::arithmetic::do_math2),
        19,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qt\0",
        Some(crate::main::arithmetic::do_math2),
        20,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dsignrank\0",
        Some(crate::main::arithmetic::do_math2),
        21,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"psignrank\0",
        Some(crate::main::arithmetic::do_math2),
        22,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qsignrank\0",
        Some(crate::main::arithmetic::do_math2),
        23,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"besselJ\0",
        Some(crate::main::arithmetic::do_math2),
        24,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"besselY\0",
        Some(crate::main::arithmetic::do_math2),
        25,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"psigamma\0",
        Some(crate::main::arithmetic::do_math2),
        26,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Complex functions
    FunTabEntry::new(
        b"Re\0",
        Some(crate::main::complex_cmath::do_cmathfuns),
        1,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Im\0",
        Some(crate::main::complex_cmath::do_cmathfuns),
        2,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Mod\0",
        Some(crate::main::complex_cmath::do_cmathfuns),
        3,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Arg\0",
        Some(crate::main::complex_cmath::do_cmathfuns),
        4,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Conj\0",
        Some(crate::main::complex_cmath::do_cmathfuns),
        5,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Math3 functions (d/p/q for beta, binom, cauchy, f, gamma, lnorm, logis, nbinom, norm, unif, weibull, nchisq, nt, wilcox, bessel)
    FunTabEntry::new(
        b"dbeta\0",
        Some(crate::main::arithmetic::do_math2),
        1,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pbeta\0",
        Some(crate::main::arithmetic::do_math2),
        2,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qbeta\0",
        Some(crate::main::arithmetic::do_math2),
        3,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dbinom\0",
        Some(crate::main::arithmetic::do_math2),
        4,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pbinom\0",
        Some(crate::main::arithmetic::do_math2),
        5,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qbinom\0",
        Some(crate::main::arithmetic::do_math2),
        6,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dcauchy\0",
        Some(crate::main::arithmetic::do_math2),
        7,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pcauchy\0",
        Some(crate::main::arithmetic::do_math2),
        8,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qcauchy\0",
        Some(crate::main::arithmetic::do_math2),
        9,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"df\0",
        Some(crate::main::arithmetic::do_math2),
        10,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pf\0",
        Some(crate::main::arithmetic::do_math2),
        11,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qf\0",
        Some(crate::main::arithmetic::do_math2),
        12,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dgamma\0",
        Some(crate::main::arithmetic::do_math2),
        13,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pgamma\0",
        Some(crate::main::arithmetic::do_math2),
        14,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qgamma\0",
        Some(crate::main::arithmetic::do_math2),
        15,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dlnorm\0",
        Some(crate::main::arithmetic::do_math2),
        16,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"plnorm\0",
        Some(crate::main::arithmetic::do_math2),
        17,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qlnorm\0",
        Some(crate::main::arithmetic::do_math2),
        18,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dlogis\0",
        Some(crate::main::arithmetic::do_math2),
        19,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"plogis\0",
        Some(crate::main::arithmetic::do_math2),
        20,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qlogis\0",
        Some(crate::main::arithmetic::do_math2),
        21,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dnbinom\0",
        Some(crate::main::arithmetic::do_math2),
        22,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pnbinom\0",
        Some(crate::main::arithmetic::do_math2),
        23,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qnbinom\0",
        Some(crate::main::arithmetic::do_math2),
        24,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dnorm\0",
        Some(crate::main::arithmetic::do_math2),
        25,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pnorm\0",
        Some(crate::main::arithmetic::do_math2),
        26,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qnorm\0",
        Some(crate::main::arithmetic::do_math2),
        27,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dunif\0",
        Some(crate::main::arithmetic::do_math2),
        28,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"punif\0",
        Some(crate::main::arithmetic::do_math2),
        29,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qunif\0",
        Some(crate::main::arithmetic::do_math2),
        30,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dweibull\0",
        Some(crate::main::arithmetic::do_math2),
        31,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pweibull\0",
        Some(crate::main::arithmetic::do_math2),
        32,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qweibull\0",
        Some(crate::main::arithmetic::do_math2),
        33,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dnchisq\0",
        Some(crate::main::arithmetic::do_math2),
        34,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pnchisq\0",
        Some(crate::main::arithmetic::do_math2),
        35,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qnchisq\0",
        Some(crate::main::arithmetic::do_math2),
        36,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dnt\0",
        Some(crate::main::arithmetic::do_math2),
        37,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pnt\0",
        Some(crate::main::arithmetic::do_math2),
        38,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qnt\0",
        Some(crate::main::arithmetic::do_math2),
        39,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dwilcox\0",
        Some(crate::main::arithmetic::do_math2),
        40,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pwilcox\0",
        Some(crate::main::arithmetic::do_math2),
        41,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qwilcox\0",
        Some(crate::main::arithmetic::do_math2),
        42,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"besselI\0",
        Some(crate::main::arithmetic::do_math2),
        43,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"besselK\0",
        Some(crate::main::arithmetic::do_math2),
        44,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dnbinom_mu\0",
        Some(crate::main::arithmetic::do_math2),
        45,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pnbinom_mu\0",
        Some(crate::main::arithmetic::do_math2),
        46,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qnbinom_mu\0",
        Some(crate::main::arithmetic::do_math2),
        47,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Math4 functions
    FunTabEntry::new(
        b"dhyper\0",
        Some(crate::main::arithmetic::do_math2),
        1,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"phyper\0",
        Some(crate::main::arithmetic::do_math2),
        2,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qhyper\0",
        Some(crate::main::arithmetic::do_math2),
        3,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dnbeta\0",
        Some(crate::main::arithmetic::do_math2),
        4,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pnbeta\0",
        Some(crate::main::arithmetic::do_math2),
        5,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qnbeta\0",
        Some(crate::main::arithmetic::do_math2),
        6,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dnf\0",
        Some(crate::main::arithmetic::do_math2),
        7,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pnf\0",
        Some(crate::main::arithmetic::do_math2),
        8,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qnf\0",
        Some(crate::main::arithmetic::do_math2),
        9,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dtukey\0",
        Some(crate::main::arithmetic::do_math2),
        10,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"ptukey\0",
        Some(crate::main::arithmetic::do_math2),
        11,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qtukey\0",
        Some(crate::main::arithmetic::do_math2),
        12,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Random number generators
    FunTabEntry::new(
        b"rchisq\0",
        Some(crate::main::random::do_random1),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rexp\0",
        Some(crate::main::random::do_random1),
        1,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rgeom\0",
        Some(crate::main::random::do_random1),
        2,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rpois\0",
        Some(crate::main::random::do_random1),
        3,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rt\0",
        Some(crate::main::random::do_random1),
        4,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rsignrank\0",
        Some(crate::main::random::do_random1),
        5,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rbeta\0",
        Some(crate::main::random::do_random2),
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rbinom\0",
        Some(crate::main::random::do_random2),
        1,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rcauchy\0",
        Some(crate::main::random::do_random2),
        2,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rf\0",
        Some(crate::main::random::do_random2),
        3,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rgamma\0",
        Some(crate::main::random::do_random2),
        4,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rlnorm\0",
        Some(crate::main::random::do_random2),
        5,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rlogis\0",
        Some(crate::main::random::do_random2),
        6,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rnbinom\0",
        Some(crate::main::random::do_random2),
        7,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rnchisq\0",
        Some(crate::main::random::do_random2),
        12,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rnorm\0",
        Some(crate::main::random::do_random2),
        8,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"runif\0",
        Some(crate::main::random::do_random2),
        9,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rweibull\0",
        Some(crate::main::random::do_random2),
        10,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rwilcox\0",
        Some(crate::main::random::do_random2),
        11,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rnbinom_mu\0",
        Some(crate::main::random::do_random2),
        13,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rhyper\0",
        Some(crate::main::random::do_random3),
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sample\0",
        Some(crate::main::random::do_sample),
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sample2\0",
        Some(do_sample2),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"RNGkind\0",
        Some(crate::main::random::do_RNGkind),
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"set.seed\0",
        Some(crate::main::random::do_setseed),
        0,
        111,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Data Summaries
    FunTabEntry::new(
        b"sum\0",
        Some(crate::main::summary::do_summary),
        0,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"min\0",
        Some(crate::main::summary::do_summary),
        2,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"max\0",
        Some(crate::main::summary::do_summary),
        3,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"prod\0",
        Some(crate::main::summary::do_summary),
        4,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"mean\0",
        Some(crate::main::summary::do_mean),
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"range\0",
        Some(crate::main::summary::do_range),
        0,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"cumsum\0",
        Some(crate::main::cum::do_cumsum),
        1,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"cumprod\0",
        Some(crate::main::cum::do_cumprod),
        2,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"cummax\0",
        Some(crate::main::cum::do_cummax),
        3,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"cummin\0",
        Some(crate::main::cum::do_cummin),
        4,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Type coercion
    FunTabEntry::new(
        b"as.character\0",
        Some(crate::main::coerce::do_ascoerce),
        0,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.integer\0",
        Some(crate::main::coerce::do_ascoerce),
        1,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.double\0",
        Some(crate::main::coerce::do_ascoerce),
        2,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.numeric\0",
        Some(crate::main::coerce::do_ascoerce),
        2,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.complex\0",
        Some(crate::main::coerce::do_ascoerce),
        3,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.logical\0",
        Some(crate::main::coerce::do_ascoerce),
        4,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.raw\0",
        Some(crate::main::coerce::do_ascoerce),
        5,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.call\0",
        Some(crate::main::inspect::do_as_call),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.environment\0",
        Some(crate::main::inspect::do_as_environment),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"storage.mode<-\0",
        Some(do_storage_mode_assign),
        0,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"asCharacterFactor\0",
        Some(crate::main::coerce::do_asCharacterFactor),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.vector\0",
        Some(crate::main::coerce::do_asvector),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"paste\0",
        Some(crate::main::paste_impl::do_paste),
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"paste0\0",
        Some(crate::main::paste_impl::do_paste),
        1,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.path\0",
        Some(crate::main::paste_impl::do_filepath),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"format\0",
        Some(crate::main::paste_impl::do_format),
        0,
        11,
        9,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"format.info\0",
        Some(crate::main::paste_impl::do_formatinfo),
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"cat\0",
        Some(do_cat),
        0,
        111,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"do.call\0",
        Some(do_do_call),
        0,
        211,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"str2lang\0",
        Some(do_str2lang),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"str2expression\0",
        Some(do_str2expression),
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // String Manipulation
    FunTabEntry::new(
        b"nchar\0",
        Some(crate::main::character::do_nchar),
        1,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"nzchar\0",
        Some(crate::main::character::do_nzchar),
        1,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"substr\0",
        Some(crate::main::character::do_substr),
        1,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"startsWith\0",
        Some(crate::main::character::do_startsWith),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"endsWith\0",
        Some(crate::main::character::do_endsWith),
        1,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"substr<-\0",
        Some(do_substr_assign),
        1,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"strsplit\0",
        Some(do_strsplit),
        1,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"abbreviate\0",
        Some(crate::main::character::do_abbreviate),
        1,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"make.names\0",
        Some(crate::main::character::do_make_names),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"pcre_config\0",
        Some(crate::main::grep::do_pcre_config),
        1,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"grep\0",
        Some(crate::main::grep::do_grep),
        0,
        11,
        8,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"grepl\0",
        Some(crate::main::grep::do_grepl),
        1,
        11,
        8,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"grepRaw\0",
        Some(crate::main::grep::do_grepraw),
        0,
        11,
        8,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sub\0",
        Some(crate::main::grep::do_sub),
        0,
        11,
        7,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"gsub\0",
        Some(crate::main::grep::do_gsub),
        1,
        11,
        7,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"regexpr\0",
        Some(crate::main::grep::do_regexpr),
        0,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"gregexpr\0",
        Some(crate::main::grep::do_gregexpr),
        1,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"regexec\0",
        Some(crate::main::grep::do_regexec),
        1,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"agrep\0",
        Some(crate::main::agrep::do_agrep),
        0,
        11,
        8,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"agrepl\0",
        Some(do_agrepl),
        1,
        11,
        8,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"adist\0",
        Some(crate::main::agrep::do_adist),
        1,
        11,
        8,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"aregexec\0",
        Some(crate::main::agrep::do_aregexec),
        1,
        11,
        7,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"tolower\0",
        Some(crate::main::character::do_tolower),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"toupper\0",
        Some(crate::main::character::do_toupper),
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"chartr\0",
        Some(crate::main::character::do_chartr),
        1,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sprintf\0",
        Some(crate::main::sprintf_main::do_sprintf),
        1,
        11,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"make.unique\0",
        Some(crate::main::character::do_make_unique),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"charToRaw\0",
        Some(crate::main::raw::do_charToRaw),
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rawToChar\0",
        Some(crate::main::raw::do_rawToChar),
        1,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rawShift\0",
        Some(crate::main::raw::do_rawShift),
        1,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"intToBits\0",
        Some(crate::main::raw::do_intToBits),
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"numToBits\0",
        Some(crate::main::raw::do_numToBits),
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"numToInts\0",
        Some(crate::main::raw::do_numToInts),
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rawToBits\0",
        Some(crate::main::raw::do_rawToBits),
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"packBits\0",
        Some(crate::main::raw::do_packBits),
        1,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"utf8ToInt\0",
        Some(crate::main::raw::do_utf8ToInt),
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"intToUtf8\0",
        Some(crate::main::raw::do_intToUtf8),
        1,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"validUTF8\0",
        Some(crate::main::character::do_validUTF8),
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"validEnc\0",
        Some(crate::main::character::do_validEnc),
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"encodeString\0",
        Some(crate::main::character::do_encodeString),
        1,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"iconv\0",
        Some(crate::main::sysutils::do_iconv),
        0,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"strtrim\0",
        Some(crate::main::character::do_strtrim),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"strtoi\0",
        Some(crate::main::character::do_strtoi),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"strrep\0",
        Some(crate::main::character::do_strrep),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Type Checking
    FunTabEntry::new(
        b"is.null\0",
        Some(crate::main::inspect::do_isnull),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.logical\0",
        Some(crate::main::coerce::do_is),
        10,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.integer\0",
        Some(crate::main::coerce::do_is),
        13,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.double\0",
        Some(crate::main::coerce::do_is),
        14,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.complex\0",
        Some(crate::main::coerce::do_is),
        15,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.character\0",
        Some(crate::main::coerce::do_is),
        16,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.symbol\0",
        Some(crate::main::coerce::do_is),
        1,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.name\0",
        Some(crate::main::coerce::do_is),
        1,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.environment\0",
        Some(crate::main::coerce::do_is),
        4,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.list\0",
        Some(crate::main::coerce::do_is),
        19,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.pairlist\0",
        Some(crate::main::coerce::do_is),
        2,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.expression\0",
        Some(crate::main::coerce::do_is),
        20,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.raw\0",
        Some(crate::main::coerce::do_is),
        24,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.object\0",
        Some(crate::main::coerce::do_is),
        50,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"isS4\0",
        Some(crate::main::coerce::do_is),
        51,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.numeric\0",
        Some(crate::main::coerce::do_is),
        100,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.matrix\0",
        Some(crate::main::coerce::do_is),
        101,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.array\0",
        Some(crate::main::coerce::do_is),
        102,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.atomic\0",
        Some(crate::main::coerce::do_is),
        200,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.recursive\0",
        Some(crate::main::coerce::do_is),
        201,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.call\0",
        Some(crate::main::coerce::do_is),
        6,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.language\0",
        Some(crate::main::coerce::do_is),
        300,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.function\0",
        Some(crate::main::coerce::do_is),
        302,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.single\0",
        Some(crate::main::coerce::do_is),
        999,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.na\0",
        Some(crate::main::coerce::do_isna),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.nan\0",
        Some(crate::main::coerce::do_isnan),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.finite\0",
        Some(crate::main::coerce::do_isfinite),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.infinite\0",
        Some(crate::main::coerce::do_isinfinite),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"is.vector\0",
        Some(crate::main::coerce::do_isvector),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Miscellaneous
    FunTabEntry::new(
        b"proc.time\0",
        Some(crate::main::platform::do_proc_time),
        0,
        1,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"gc.time\0",
        Some(crate::main::platform::do_gcinfo),
        0,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"withVisible\0",
        Some(crate::main::inspect::do_withVisible),
        1,
        10,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"expression\0",
        Some(crate::main::inspect::do_expression),
        1,
        0,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"interactive\0",
        Some(crate::main::sysutils::do_interactive),
        0,
        1,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"invisible\0",
        Some(crate::main::inspect::do_invisible),
        0,
        101,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rep\0",
        Some(crate::main::seq::do_rep),
        0,
        0,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rep.int\0",
        Some(crate::main::seq::do_rep),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"rep_len\0",
        Some(crate::main::seq::do_rep),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"seq.int\0",
        Some(crate::main::seq::do_seq),
        0,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"seq_len\0",
        Some(crate::main::seq::do_seq_len),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"seq_along\0",
        Some(crate::main::seq::do_seq_along),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"list\0",
        Some(crate::main::inspect::do_list),
        1,
        1,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"xtfrm\0",
        Some(do_xtfrm),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"enc2native\0",
        Some(crate::main::inspect::do_enc2native),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"enc2utf8\0",
        Some(crate::main::inspect::do_enc2utf8),
        1,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"emptyenv\0",
        Some(crate::main::inspect::do_emptyenv),
        0,
        1,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"baseenv\0",
        Some(crate::main::inspect::do_baseenv),
        0,
        1,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"globalenv\0",
        Some(crate::main::inspect::do_globalenv),
        0,
        1,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"environment<-\0",
        Some(do_environment_assign),
        0,
        1,
        2,
        PPinfo::new(PP_FUNCALL, PREC_LEFT, 1),
    ),
    FunTabEntry::new(
        b"pos.to.env\0",
        Some(do_pos_to_env),
        0,
        1,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".C\0",
        Some(do_foreign_C),
        0,
        1,
        -1,
        PPinfo::new(PP_FOREIGN, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".Fortran\0",
        Some(do_foreign_Fortran),
        1,
        1,
        -1,
        PPinfo::new(PP_FOREIGN, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".External\0",
        Some(do_foreign_External),
        0,
        1,
        -1,
        PPinfo::new(PP_FOREIGN, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".External2\0",
        Some(do_foreign_External2),
        1,
        201,
        -1,
        PPinfo::new(PP_FOREIGN, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".Call\0",
        Some(do_foreign_Call),
        0,
        1,
        -1,
        PPinfo::new(PP_FOREIGN, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".External.graphics\0",
        Some(do_foreign_External_graphics),
        0,
        1,
        -1,
        PPinfo::new(PP_FOREIGN, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b".Call.graphics\0",
        Some(do_foreign_Call_graphics),
        0,
        1,
        -1,
        PPinfo::new(PP_FOREIGN, PREC_FN, 0),
    ),
    // More .Internals
    FunTabEntry::new(
        b"eapply\0",
        Some(do_eapply),
        0,
        10,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"lapply\0",
        Some(crate::main::apply::do_lapply),
        0,
        10,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"vapply\0",
        Some(crate::main::apply::do_vapply),
        0,
        10,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"mapply\0",
        Some(crate::main::mapply::do_mapply),
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Version\0",
        Some(do_Version),
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"machine\0",
        Some(crate::unix::sys_unix::do_machine),
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"commandArgs\0",
        Some(crate::main::CommandLineArgs::do_commandArgs),
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"internalsID\0",
        Some(do_internalsID),
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"system\0",
        Some(do_system),
        0,
        211,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"parse\0",
        Some(do_parse_fn),
        0,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"save\0",
        Some(crate::main::saveload::do_save),
        0,
        111,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"load\0",
        Some(crate::main::saveload::do_load),
        0,
        111,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"deparse\0",
        Some(crate::main::deparse::do_deparse),
        0,
        11,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"quit\0",
        Some(do_quit),
        0,
        111,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"readline\0",
        Some(do_readline),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"print.default\0",
        Some(crate::main::print::do_printdefault),
        0,
        111,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"gc\0",
        Some(crate::main::platform::do_gc),
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"gcinfo\0",
        Some(do_gcinfo),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"split\0",
        Some(crate::main::split::do_split),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"ls\0",
        Some(crate::main::envir::do_ls),
        1,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"typeof\0",
        Some(crate::main::inspect::do_typeof),
        1,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"eval\0",
        Some(do_eval_fn),
        0,
        211,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"sort\0",
        Some(do_sort),
        1,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"radixsort\0",
        Some(crate::main::radixsort::do_radixsort),
        0,
        11,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"qsort\0",
        Some(crate::main::qsort::do_qsort),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"order\0",
        Some(do_order),
        0,
        11,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"scan\0",
        Some(crate::main::scan::do_scan),
        0,
        11,
        19,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"t.default\0",
        Some(crate::main::array::do_transpose),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"options\0",
        Some(crate::main::options::do_options),
        0,
        211,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"getOption\0",
        Some(crate::main::options::do_getOption),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"inspect\0",
        Some(do_inspect),
        0,
        111,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"capabilities\0",
        Some(crate::main::platform::do_capabilities),
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"new.env\0",
        Some(do_new_env),
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.time\0",
        Some(crate::main::datetime::do_Sys_time),
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.POSIXct\0",
        Some(crate::main::datetime::do_asPOSIXct),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"as.POSIXlt\0",
        Some(crate::main::datetime::do_asPOSIXlt),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"format.POSIXlt\0",
        Some(crate::main::datetime::do_formatPOSIXlt),
        0,
        11,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"strptime\0",
        Some(crate::main::datetime::do_strptime),
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Date2POSIXlt\0",
        Some(crate::main::datetime::do_D2POSIXlt),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"POSIXlt2Date\0",
        Some(crate::main::datetime::do_POSIXlt2D),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"balancePOSIXlt\0",
        Some(crate::main::datetime::do_balancePOSIXlt),
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"polyroot\0",
        Some(do_polyroot_fn),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"inherits\0",
        Some(crate::main::objects::do_inherits),
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"UseMethod\0",
        Some(crate::main::objects::do_usemethod),
        0,
        200,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"NextMethod\0",
        Some(crate::main::objects::do_nextmethod),
        0,
        210,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"standardGeneric\0",
        Some(crate::main::objects::do_standardGeneric),
        0,
        201,
        -1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"compareNumericVersion\0",
        Some(do_compareNumericVersion),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // OS interaction
    FunTabEntry::new(
        b"file.show\0",
        Some(crate::main::platform::do_fileshow),
        0,
        111,
        5,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.create\0",
        Some(crate::main::platform::do_filecreate),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.remove\0",
        Some(crate::main::platform::do_fileremove),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.rename\0",
        Some(crate::main::platform::do_filerename),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.append\0",
        Some(crate::main::platform::do_fileappend),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.symlink\0",
        Some(crate::main::platform::do_filesymlink),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.link\0",
        Some(crate::main::platform::do_filelink),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.copy\0",
        Some(crate::main::platform::do_filecopy),
        0,
        11,
        6,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"list.files\0",
        Some(crate::main::platform::do_listfiles),
        0,
        11,
        9,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"list.dirs\0",
        Some(crate::main::platform::do_listdirs),
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.exists\0",
        Some(crate::main::platform::do_fileexists),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.choose\0",
        Some(crate::main::platform::do_filechoose),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.info\0",
        Some(crate::main::platform::do_fileinfo),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"file.access\0",
        Some(crate::main::platform::do_fileaccess),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dir.exists\0",
        Some(crate::main::platform::do_direxists),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dir.create\0",
        Some(crate::main::platform::do_dircreate),
        0,
        111,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"R.home\0",
        Some(crate::main::platform::do_Rhome),
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"date\0",
        Some(crate::main::platform::do_date),
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.getenv\0",
        Some(crate::main::sysutils::do_getenv),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.getlocale\0",
        Some(crate::main::platform::do_getlocale),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.setlocale\0",
        Some(crate::main::platform::do_setlocale),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.localeconv\0",
        Some(crate::main::platform::do_localeconv),
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"path.expand\0",
        Some(crate::main::platform::do_pathexpand),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.getpid\0",
        Some(crate::main::platform::do_sysgetpid),
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"unlink\0",
        Some(crate::main::platform::do_unlink),
        0,
        111,
        4,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.sleep\0",
        Some(crate::main::platform::do_Sys_sleep),
        0,
        111,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.info\0",
        Some(crate::unix::sys_unix::do_sysinfo),
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.chmod\0",
        Some(crate::main::platform::do_syschmod),
        0,
        111,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.umask\0",
        Some(crate::main::platform::do_sysumask),
        0,
        211,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.readlink\0",
        Some(crate::main::platform::do_readlink),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"l10n_info\0",
        Some(crate::main::platform::do_l10n_info),
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Cstack_info\0",
        Some(crate::main::platform::do_Cstack_info),
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"eSoftVersion\0",
        Some(crate::main::platform::do_eSoftVersion),
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"mkjunction\0",
        Some(crate::main::platform::do_mkjunction),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    // Stub entries for remaining functions
    FunTabEntry::new(
        b"gctorture\0",
        Some(crate::main::memory_main::do_gctorture),
        0,
        111,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"gctorture2\0",
        Some(crate::main::memory_main::do_gctorture2),
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"memory.profile\0",
        Some(do_memory_profile),
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"mem.maxVSize\0",
        Some(crate::main::memory_main::do_maxVSize),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"mem.maxNSize\0",
        Some(crate::main::memory_main::do_maxNSize),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"builtins\0",
        Some(do_builtins),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"args\0",
        Some(crate::main::inspect::do_args),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"formals\0",
        Some(crate::main::inspect::do_formals),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"body\0",
        Some(crate::main::inspect::do_body),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"environment\0",
        Some(crate::main::inspect::do_environment),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"getenv\0",
        Some(crate::main::sysutils::do_getenv),
        0,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.setenv\0",
        Some(crate::main::sysutils::do_setenv),
        0,
        111,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"Sys.unsetenv\0",
        Some(crate::main::sysutils::do_unsetenv),
        0,
        111,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"getwd\0",
        Some(crate::main::platform::do_getwd),
        0,
        11,
        0,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"setwd\0",
        Some(crate::main::platform::do_setwd),
        0,
        111,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"basename\0",
        Some(crate::main::platform::do_basename),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"dirname\0",
        Some(crate::main::platform::do_dirname),
        0,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"readDCF\0",
        Some(crate::main::dcf::do_readDCF),
        0,
        11,
        3,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"bitwiseAnd\0",
        Some(crate::main::relop::do_bitwise),
        1,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"bitwiseNot\0",
        Some(crate::main::relop::do_bitwise),
        2,
        11,
        1,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"bitwiseOr\0",
        Some(crate::main::relop::do_bitwise),
        3,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"bitwiseXor\0",
        Some(crate::main::relop::do_bitwise),
        4,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"bitwiseShiftL\0",
        Some(crate::main::relop::do_bitwise),
        5,
        11,
        2,
        PPinfo::new(PP_FUNCALL, PREC_FN, 0),
    ),
    FunTabEntry::new(
        b"bitwiseShiftR\0",
        Some(crate::main::relop::do_bitwise),
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
        if TYPEOF(name) != SEXPTYPE::STRSXP.0 || LENGTH(name) != 1 {
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

        let name_cstr =
            std::ffi::CString::new(entry_name).expect("CString::new failed: contains null byte");
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

/// Pre-allocated DDVAL symbols (..0 through ..64).
///
/// SEXP (raw pointers) are not Send/Sync, so we wrap in a newtype
/// that asserts safety. All access is through unsafe installDDVAL anyway.
struct DDVALSymbolsInner(Vec<SEXP>);
unsafe impl Send for DDVALSymbolsInner {}
unsafe impl Sync for DDVALSymbolsInner {}

static DDVAL_SYMBOLS: std::sync::OnceLock<DDVALSymbolsInner> = std::sync::OnceLock::new();

/// Get or create a DDVAL symbol for index n.
pub unsafe fn installDDVAL(n: c_int) -> SEXP {
    unsafe {
        let symbols = DDVAL_SYMBOLS.get_or_init(|| {
            let mut v: Vec<SEXP> = Vec::with_capacity(N_DDVAL_SYMBOLS);
            for i in 0..N_DDVAL_SYMBOLS {
                let name = format!("..{}", i);
                let sym = Rf_install(
                    std::ffi::CString::new(name)
                        .expect("CString::new failed: contains null byte")
                        .as_ptr(),
                );
                v.push(sym);
            }
            DDVALSymbolsInner(v)
        });
        let n = n as usize;
        if n < symbols.0.len() {
            symbols.0[n]
        } else {
            let name = format!("..{}", n);
            Rf_install(
                std::ffi::CString::new(name)
                    .expect("CString::new failed: contains null byte")
                    .as_ptr(),
            )
        }
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

/// Flag indicating whether the symbol table has been initialized.
static INIT_NAMES_DONE: AtomicBool = AtomicBool::new(false);

/// Initialize the R symbol table.
/// This must be called once before any symbol lookup operations.
pub unsafe fn InitNames() {
    unsafe {
        if INIT_NAMES_DONE.load(Ordering::Relaxed) {
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

        // Initialize DDVAL symbols
        let _ = installDDVAL(0); // Force initialization of DDVAL_SYMBOLS

        INIT_NAMES_DONE.store(true, Ordering::Relaxed);
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

        let sig_cstr =
            std::ffi::CString::new(sig).expect("CString::new failed: contains null byte");
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
        // s must be a pairlist
        if s.is_null() || TYPEOF(s) != SEXPTYPE::LISTSXP.0 {
            panic_any(RError {
                message: "invalid .Internal() argument".to_string(),
            });
        }
        let fun = CAR(s);
        // fun must be a symbol
        if fun.is_null() || TYPEOF(fun) != SEXPTYPE::SYMSXP.0 {
            panic_any(RError {
                message: "invalid .Internal() argument".to_string(),
            });
        }
        let internal_val = crate::sexp::accessors::INTERNAL(fun);
        if internal_val.is_null() || internal_val == R_NilValue() {
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
            panic_any(RError {
                message: format!("there is no .Internal function '{}'", name_str),
            });
        }

        // Get the actual arguments (CDR of the pairlist)
        let actual_args = CDR(s);

        // For BUILTINSXP, evaluate the argument list; for SPECIALSXP, pass as-is
        let evaluated_args = if TYPEOF(internal_val) == SEXPTYPE::BUILTINSXP.0 {
            crate::eval::dispatch::evalList(actual_args, env, call, 0)
        } else {
            actual_args
        };
        Rf_protect(evaluated_args);

        // Get the PRIMPRINT flag (visibility hint)
        let flag = crate::eval::eval::PRIMPRINT(internal_val);
        // Set R_Visible: flag != 1 means visible
        crate::sexp::globals::set_R_Visible(if flag != 1 { 1 } else { 0 });

        // Get the function pointer from the FunTab via offset
        let offset = PRIMOFFSET(internal_val);
        let entry = &R_FunTab[offset as usize];
        let cfun = entry.cfun;

        let ans = if let Some(f) = cfun {
            f(s, internal_val, evaluated_args, env)
        } else {
            R_NilValue()
        };

        // Reset visibility if flag < 2
        if flag < 2 {
            crate::sexp::globals::set_R_Visible(if flag != 1 { 1 } else { 0 });
        }

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_tilde
// ---------------------------------------------------------------------------

/// Implementation of the ~ operator.
/// Creates a formula object with class "formula" and .Environment attribute.
pub unsafe fn do_tilde(call: SEXP, _op: SEXP, _args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if !call.is_null() && OBJECT(call) != 0 {
            duplicate(call)
        } else {
            let klass = Rf_mkString(b"formula\0".as_ptr() as *const c_char);
            Rf_protect(klass);

            let result = duplicate(call);
            Rf_protect(result);

            let class_sym = Rf_install(b"class\0".as_ptr() as *const c_char);
            setAttrib(result, class_sym, klass);

            let dot_env_sym = Rf_install(b".Environment\0".as_ptr() as *const c_char);
            setAttrib(result, dot_env_sym, rho);

            Rf_unprotect(2);
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
        if t != SEXPTYPE::SPECIALSXP.0 && t != SEXPTYPE::BUILTINSXP.0 {
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
// PRIMARITY -- get the arity of a primitive function
// ---------------------------------------------------------------------------

/// Get the arity (number of arguments) of a primitive function.
/// Returns -1 for variable arity.
/// Equivalent to R's `PRIMARITY()` macro.
pub unsafe fn PRIMARITY(op: SEXP) -> c_int {
    unsafe {
        if op.is_null() {
            return -1;
        }
        let t = TYPEOF(op);
        if t != SEXPTYPE::SPECIALSXP.0 && t != SEXPTYPE::BUILTINSXP.0 {
            return -1;
        }
        let offset = PRIMOFFSET(op) as usize;
        if offset >= R_FunTab.len() || R_FunTab[offset].is_sentinel() {
            return -1;
        }
        R_FunTab[offset].arity
    }
}

// ---------------------------------------------------------------------------
// PRIMINTERNAL -- check if a primitive is a .Internal function
// ---------------------------------------------------------------------------

/// Check if a primitive function is a .Internal (vs .Primitive).
/// Returns non-zero if the function is accessed via .Internal().
/// Equivalent to R's `PRIMINTERNAL()` macro.
pub unsafe fn PRIMINTERNAL(op: SEXP) -> c_int {
    unsafe {
        if op.is_null() {
            return 0;
        }
        let t = TYPEOF(op);
        if t != SEXPTYPE::SPECIALSXP.0 && t != SEXPTYPE::BUILTINSXP.0 {
            return 0;
        }
        // Check if PRIMOFFSET is negative (custom internal)
        let offset = PRIMOFFSET(op);
        if offset < 0 {
            return 1;
        }
        // Check the eval field's tens digit
        let offset_usize = offset as usize;
        if offset_usize >= R_FunTab.len() || R_FunTab[offset_usize].is_sentinel() {
            return 0;
        }
        if (R_FunTab[offset_usize].eval % 100) / 10 != 0 {
            1
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
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
        let last = R_FunTab.last().unwrap();
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
        unsafe {
            let result = R_Primitive(b"nonexistent\0".as_ptr() as *const c_char);
            assert!(Rf_isNull(result) != 0 || result.is_null());
        }
    }

    #[test]
    fn test_r_primitive_is_internal() {
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
    fn test_install_s3_signature() {
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
    fn test_mk_sym_marker() {
        unsafe {
            let pname = Rf_mkChar(b"test\0".as_ptr() as *const c_char);
            let sym = mkSymMarker(pname);
            assert!(!sym.is_null());
            // Value should point to itself
            assert_eq!(crate::sexp::accessors::SYMVALUE(sym), sym);
        }
    }

    #[test]
    fn test_primarity_null() {
        unsafe {
            assert_eq!(PRIMARITY(ptr::null_mut()), -1);
        }
    }

    #[test]
    fn test_priminternal_null() {
        unsafe {
            assert_eq!(PRIMINTERNAL(ptr::null_mut()), 0);
        }
    }
}
