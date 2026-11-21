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
    CAR, CDR, CHAR, LENGTH, OBJECT, PRIMOFFSET, PRINTNAME, SET_ATTRIB, SET_INTERNAL, SET_PRINTNAME,
    SET_SYMVALUE, TYPEOF,
};
use crate::sexp::context::RError;
use crate::sexp::ffi::{FALSE, NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance;
use crate::sexp::memory::with_arena;
use crate::sexp::protect::protect;

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

mod arrays;
mod distributions;
mod internals;
mod language;
mod math;
mod strings;
mod summaries;
mod types;
mod unimplemented;

macro_rules! concat_funtab {
    ($($slice:expr),+ $(,)?) => {{
        const __TOTAL: usize = 0 $(+ $slice.len())+;
        let mut __out = [FunTabEntry::new(
            NULL_NAME,
            None,
            0,
            0,
            0,
            PPinfo::new(PP_INVALID, PREC_FN, 0),
        ); __TOTAL];
        let mut __i = 0usize;
        $(
            let mut __j = 0usize;
            while __j < $slice.len() {
                __out[__i] = $slice[__j];
                __i += 1;
                __j += 1;
            }
        )+
        __out
    }};
}

const FUNTAB_ENTRIES: &[FunTabEntry] = &concat_funtab!(
    language::LANGUAGE_FUNTAB,
    arrays::ARRAYS_FUNTAB,
    math::MATH_FUNTAB,
    distributions::DISTRIBUTIONS_FUNTAB,
    summaries::SUMMARIES_FUNTAB,
    strings::STRINGS_FUNTAB,
    types::TYPES_FUNTAB,
    internals::INTERNALS_FUNTAB,
    unimplemented::UNIMPLEMENTED_FUNTAB,
);

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
pub fn do_primitive(call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let name = CAR(args);
    // Check that name is a single string
    if TYPEOF(name) != SEXPTYPE::STRSXP || LENGTH(name) != 1 {
        panic_any(RError {
            message: "string argument required".to_string(),
        });
    }
    let factory = crate::sexp::object::SessionNodeFactory::new(
        // SAFETY: the do_* boundary retains the active runtime.
        unsafe { crate::sexp::owner::OwnerToken::current() }
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
    );
    let name_str = factory
        .wrap(name)
        .ok()
        .and_then(|name| name.try_string_value_elt(0).ok());
    let Some(name_str) = name_str else {
        panic_any(RError {
            message: "string argument required".to_string(),
        });
    };
    // NA_character_ looks up the name "NA", matching CHAR(NA_STRING).
    let name_str = name_str.unwrap_or_else(|| "NA".to_string());
    let name_cstr = std::ffi::CString::new(name_str.as_str()).unwrap_or_default();
    // SAFETY: name_cstr is a live NUL-terminated buffer for the call.
    let prim = unsafe { R_Primitive(name_cstr.as_ptr()) };
    if prim.is_null() || prim == R_NilValue() {
        panic_any(RError {
            message: format!("no such primitive function '{}'", name_str),
        });
    }
    prim
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
fn installFunTab(i: usize) {
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
        let sym = crate::sexp::symbol::install_cstr(&name_cstr);

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
pub fn SymbolShortcuts() {
    // These use Rf_install which is our HashMap-based implementation.
    // The results are cached in the symbol table.
    let _ = crate::sexp::symbol::install_str("[[");
    let _ = crate::sexp::symbol::install_str("[");
    let _ = crate::sexp::symbol::install_str("{");
    let _ = crate::sexp::symbol::install_str("class");
    let _ = crate::sexp::symbol::install_str(".Device");
    let _ = crate::sexp::symbol::install_str("dimnames");
    let _ = crate::sexp::symbol::install_str("dim");
    let _ = crate::sexp::symbol::install_str("$");
    let _ = crate::sexp::symbol::install_str("@");
    let _ = crate::sexp::symbol::install_str("...");
    let _ = crate::sexp::symbol::install_str("drop");
    let _ = crate::sexp::symbol::install_str("eval");
    let _ = crate::sexp::symbol::install_str(".Last.value");
    let _ = crate::sexp::symbol::install_str("levels");
    let _ = crate::sexp::symbol::install_str("mode");
    let _ = crate::sexp::symbol::install_str("name");
    let _ = crate::sexp::symbol::install_str("names");
    let _ = crate::sexp::symbol::install_str("na.rm");
    let _ = crate::sexp::symbol::install_str("package");
    let _ = crate::sexp::symbol::install_str("previous");
    let _ = crate::sexp::symbol::install_str("quote");
    let _ = crate::sexp::symbol::install_str("row.names");
    let _ = crate::sexp::symbol::install_str(".Random.seed");
    let _ = crate::sexp::symbol::install_str("sort.list");
    let _ = crate::sexp::symbol::install_str("source");
    let _ = crate::sexp::symbol::install_str("tsp");
    let _ = crate::sexp::symbol::install_str("comment");
    let _ = crate::sexp::symbol::install_str(".Environment");
    let _ = crate::sexp::symbol::install_str("exact");
    let _ = crate::sexp::symbol::install_str("recursive");
    let _ = crate::sexp::symbol::install_str("srcfile");
    let _ = crate::sexp::symbol::install_str("srcref");
    let _ = crate::sexp::symbol::install_str("wholeSrcref");
    let _ = crate::sexp::symbol::install_str("*tmp*");
    let _ = crate::sexp::symbol::install_str("use.names");
    let _ = crate::sexp::symbol::install_str(":");
    let _ = crate::sexp::symbol::install_str("::");
    let _ = crate::sexp::symbol::install_str(":::");
    let _ = crate::sexp::symbol::install_str("conn_id");
    let _ = crate::sexp::symbol::install_str(".Devices");
    let _ = crate::sexp::symbol::install_str("base");
    let _ = crate::sexp::symbol::install_str("spec");
    let _ = crate::sexp::symbol::install_str(".__NAMESPACE__.");
    let _ = crate::sexp::symbol::install_str("as.character");
    let _ = crate::sexp::symbol::install_str("function");
    let _ = crate::sexp::symbol::install_str(".Generic");
    let _ = crate::sexp::symbol::install_str(".Method");
    let _ = crate::sexp::symbol::install_str(".Methods");
    let _ = crate::sexp::symbol::install_str(".defined");
    let _ = crate::sexp::symbol::install_str(".target");
    let _ = crate::sexp::symbol::install_str(".Group");
    let _ = crate::sexp::symbol::install_str(".Class");
    let _ = crate::sexp::symbol::install_str(".GenericCallEnv");
    let _ = crate::sexp::symbol::install_str(".GenericDefEnv");
    let _ = crate::sexp::symbol::install_str(".packageName");
}

// ---------------------------------------------------------------------------
// DDVALSymbols
// ---------------------------------------------------------------------------

const N_DDVAL_SYMBOLS: usize = 65;

/// Get or create a DDVAL symbol for index n.
pub fn installDDVAL(n: c_int) -> SEXP {
    unsafe {
        if n >= 0 {
            let n = n as usize;
            let cached = instance::with_required_current_instance(|inst| {
                // P2: short-lived shared borrow; strictly-local read, no ambient writes.
                let symbols = &(*inst).names_state.ddval_symbols;
                symbols.get(n).copied()
            });
            if let Some(sym) = cached {
                return sym;
            }
        }

        if n >= 0 && (n as usize) < N_DDVAL_SYMBOLS {
            let mut symbols = Vec::with_capacity(N_DDVAL_SYMBOLS);
            for i in 0..N_DDVAL_SYMBOLS {
                let name = format!("..{}", i);
                let sym = crate::sexp::symbol::install_cstr(
                    &std::ffi::CString::new(name).unwrap_or_default(),
                );
                symbols.push(sym);
            }
            let sym = symbols[n as usize];
            instance::with_required_current_instance(|inst| {
                if (*inst).names_state.ddval_symbols.len() < N_DDVAL_SYMBOLS {
                    (*inst).names_state.ddval_symbols = symbols;
                }
            });
            return sym;
        }

        let name = format!("..{}", n);
        crate::sexp::symbol::install_cstr(&std::ffi::CString::new(name).unwrap_or_default())
    }
}

// ---------------------------------------------------------------------------
// mkSymMarker
// ---------------------------------------------------------------------------

/// Create a symbol marker (SYMSXP whose value points to itself).
fn mkSymMarker(pname: SEXP) -> SEXP {
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
pub fn InitNames() {
    unsafe {
        if instance::with_required_current_instance(|inst| (*inst).names_state.init_names_done) {
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
            (*inst).names_state.init_names_done = true;
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
        crate::sexp::symbol::install_cstr(&sig_cstr)
    }
}

// ---------------------------------------------------------------------------
// do_internal
// ---------------------------------------------------------------------------

/// Implementation of .Internal()
/// Looks up an internal function and dispatches to its C implementation.
pub fn do_internal(call: SEXP, _op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        crate::sexp::owner::OwnerToken::with_current(|token_owner| {
            let factory = crate::sexp::object::SessionNodeFactory::new(token_owner);
            let args_owner = factory
                .wrap(args)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let env_owner = factory
                .wrap(env)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let call_owner = factory
                .wrap(call)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let s = CAR(args_owner.as_raw());
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
                if let Some(handler) = internal_builtin_handler(&name_str) {
                    let actual_args = CDR(s);
                    let evaluated_args = crate::eval::dispatch::evalList(
                        factory.wrap(actual_args).unwrap_or_else(|error| {
                            crate::sexp::context::r_error(error.to_string())
                        }),
                        env_owner.clone(),
                        Some(call_owner.clone()),
                        -1,
                    );
                    let ans = handler(s, R_NilValue(), evaluated_args.as_raw(), env);
                    // No funtab entry: honor the stock-invisible list (GNU marks
                    // these via the eval column we cannot read here).
                    resolve_internal_visibility(&name_str, 2);
                    return ans;
                }
                // GNU .Internal resolves strictly through R_FunTab; the port
                // also accepts handlers registered in the evaluated-builtin
                // table (topenv, bindingIsLocked, makeActiveBinding,
                // normalizePath, ...) when no INTERNAL slot or funtab entry
                // exists.
                if let Some(handler) = crate::eval::builtin::evaluated_builtin_handler(&name_str) {
                    let actual_args = CDR(s);
                    let evaluated_args = crate::eval::dispatch::evalList(
                        factory.wrap(actual_args).unwrap_or_else(|error| {
                            crate::sexp::context::r_error(error.to_string())
                        }),
                        env_owner.clone(),
                        Some(call_owner.clone()),
                        -1,
                    );
                    let ans = handler(s, R_NilValue(), evaluated_args.as_raw(), env);
                    resolve_internal_visibility(&name_str, 2);
                    return ans;
                }
                panic_any(RError {
                    message: format!("there is no .Internal function '{}'", name_str),
                });
            }

            // On-demand primitives are not necessarily installed in a symbol's
            // INTERNAL slot. Argument evaluation can collect, so root this one.
            let _internal_owner = factory
                .wrap(internal_val)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            // Get the actual arguments (CDR of the pairlist)
            let actual_args = CDR(s);

            // For BUILTINSXP, evaluate the argument list; for SPECIALSXP, pass as-is
            let evaluated_args = if TYPEOF(internal_val) == SEXPTYPE::BUILTINSXP {
                crate::eval::dispatch::evalList(
                    factory
                        .wrap(actual_args)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
                    env_owner.clone(),
                    Some(call_owner.clone()),
                    -1,
                )
            } else {
                factory
                    .wrap(actual_args)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            };

            // Get the PRIMPRINT flag (visibility hint)
            let flag = crate::eval::eval::PRIMPRINT(internal_val);
            // Set R_Visible: flag != 1 means visible
            crate::sexp::globals::set_R_Visible(if flag != 1 { 1 } else { 0 });

            let offset = PRIMOFFSET(internal_val);
            let entry = R_FunTab.get(offset as usize).unwrap_or_else(|| {
                panic_any(RError {
                    message: "invalid internal primitive offset".into(),
                })
            });
            let mut end = entry.name.len();
            if end > 0 && entry.name[end - 1] == 0 {
                end -= 1;
            }
            let name = std::str::from_utf8(&entry.name[..end]).unwrap_or("<invalid>");

            if let Some(handler) = internal_builtin_handler(name) {
                let ans = crate::mainutils::errors::attribute_handler_errors(s, || {
                    handler(s, internal_val, evaluated_args.as_raw(), env)
                });
                resolve_internal_visibility(name, flag);
                return ans;
            }

            if let Some(handler) = crate::eval::builtin::evaluated_builtin_handler(name) {
                let ans = crate::mainutils::errors::attribute_handler_errors(s, || {
                    handler(s, internal_val, evaluated_args.as_raw(), env)
                });
                resolve_internal_visibility(name, flag);
                return ans;
            }

            // Get the function pointer from the FunTab via offset.
            let cfun = entry.cfun;

            let ans = if let Some(f) = cfun {
                crate::mainutils::errors::attribute_handler_errors(s, || {
                    f(s, internal_val, evaluated_args.as_raw(), env)
                })
            } else if name == "inspect" {
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                R_NilValue()
            } else {
                panic_any(RError {
                    message: format!("internal function '{name}' is not implemented"),
                });
            };

            resolve_internal_visibility(name, flag);

            ans
        })
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
    }
}

/// Post-call visibility for .Internal dispatch. Handlers may set R_Visible
/// explicitly (options/par mode-dependent, message/warning); those escape
/// the flag restore. Internals whose results stock marks invisible (funtab
/// eval column) are forced invisible so top-level auto-printing matches
/// Rscript.
fn resolve_internal_visibility(name: &str, flag: c_int) {
    if crate::eval::primitive::internal_result_invisible(name) {
        crate::sexp::globals::set_R_Visible(0);
        return;
    }
    if crate::eval::primitive::primitive_controls_visibility(name) {
        return;
    }
    if flag < 2 {
        crate::sexp::globals::set_R_Visible(if flag != 1 { 1 } else { 0 });
    }
}

type InternalBuiltinHandler = unsafe fn(SEXP, SEXP, SEXP, SEXP) -> SEXP;

fn internal_builtin_handler(name: &str) -> Option<InternalBuiltinHandler> {
    match name {
        "La_chol" => Some(crate::mainutils::essentials::do_chol),
        "La_chol2inv" => Some(crate::mainutils::essentials::do_chol2inv),
        "builtins" => Some(do_builtins),
        "besselK" => Some(crate::mainutils::essentials::do_besselK_internal),
        "besselI" => Some(crate::mainutils::essentials::do_besselI_internal),
        "getLoadedDLLs" => Some(crate::mainutils::rdynload::do_getDllTable),
        "getSymbolInfo" => Some(crate::mainutils::rdynload::do_getSymbolInfo),
        "serialize" => Some(crate::mainutils::serialize::do_serialize_internal),
        "dput" => Some(crate::mainutils::deparse::do_dput),
        // GNU internal contract `(expr, functions, max.names, unique)`;
        // the R-level all.vars/all.names wrappers pass distinct flags.
        "all.names" => Some(crate::mainutils::list::do_allnames),
        "encodeString" => Some(crate::mainutils::essentials::do_encodeString_internal),
        "pmax" => Some(crate::mainutils::essentials::do_pmax_internal),
        "pmin" => Some(crate::mainutils::essentials::do_pmin_internal),
        "col" => Some(crate::mainutils::essentials::do_col_internal),
        "row" => Some(crate::mainutils::essentials::do_row_internal),
        "make.names" => Some(crate::mainutils::essentials::do_make_names_internal),
        "regexec" => Some(crate::mainutils::essentials::do_regexec_internal),
        "do.call" => Some(crate::mainutils::essentials::do_do_call_internal),
        "prmatrix" => Some(crate::mainutils::print::do_prmatrix),
        "sample" => Some(crate::mainutils::rng_dispatch::do_sample_internal),
        "parent.env" => Some(crate::mainutils::essentials::do_parent_env),
        "parent.env<-" => Some(crate::mainutils::essentials::do_set_parent_env),
        "bcVersion" => Some(crate::eval::compiler::do_bcversion),
        "growconst" => Some(crate::eval::compiler::do_growconst),
        "putconst" => Some(crate::eval::compiler::do_putconst),
        "getconst" => Some(crate::eval::compiler::do_getconst),
        "format" => Some(crate::mainutils::essentials::do_format),
        "readDCF" => Some(crate::mainutils::dcf::do_readDCF),
        "compareNumericVersion" => Some(crate::mainutils::dcf::do_compareNumericVersion),

        "bodyCode" => Some(crate::mainutils::builtin::do_bodyCode),
        "refcnt" => Some(do_refcnt),
        "address" => Some(do_address),
        "named" => Some(do_named),
        "addTaskCallback" => Some(do_add_task_callback),
        "removeTaskCallback" => Some(do_remove_task_callback),
        "Recall" => Some(crate::eval::eval::do_recall),
        "file.show" => Some(crate::mainutils::platform::do_fileshow),
        "stop" => Some(crate::mainutils::errors::do_stop_internal),
        "warning" => Some(crate::mainutils::errors::do_warning),
        "gettext" => Some(crate::mainutils::errors::do_gettext),
        "ngettext" => Some(crate::mainutils::errors::do_ngettext),
        "bindtextdomain" => Some(crate::mainutils::errors::do_bindtextdomain),
        ".dfltStop" => Some(crate::mainutils::errors::do_dfltStop),
        ".dfltWarn" => Some(crate::mainutils::errors::do_dfltWarn),
        "geterrmessage" => Some(crate::mainutils::errors::do_geterrmessage),
        "traceback" => Some(crate::mainutils::errors::do_traceback),
        "seterrmessage" => Some(crate::mainutils::errors::do_seterrmessage),
        "printDeferredWarnings" => Some(crate::mainutils::errors::do_printDeferredWarnings),
        "interruptsSuspended" => Some(crate::mainutils::errors::do_interruptsSuspended),
        "debug" | "undebug" | "isdebugged" | "debugonce" => Some(crate::mainutils::debug::do_debug),
        "delayedAssign" => Some(crate::mainutils::builtin::do_delayed),
        "makeLazy" => Some(crate::mainutils::builtin::do_makelazy),
        "radixsort" => Some(crate::mainutils::radixsort::do_radixsort),
        "order" => Some(crate::mainutils::essentials::do_order),
        "sort" => Some(crate::mainutils::essentials::do_sort),
        "mean" => Some(crate::mainutils::summary::do_mean),
        "bincode" => Some(crate::mainutils::bincode::do_bincode),
        "close" => Some(crate::mainutils::connections::do_close),
        "grepRaw" => Some(crate::mainutils::grep::do_grepraw),

        "save" => Some(crate::mainutils::saveload::do_save),
        "islistfactor" => Some(crate::mainutils::apply::do_islistfactor),
        "lapply" => Some(crate::mainutils::apply::do_lapply),
        "vapply" => Some(crate::mainutils::apply::do_vapply),
        "rapply" => Some(crate::mainutils::apply::do_rapply),
        "array" => Some(crate::mainutils::array::do_array),
        "paste" | "paste0" => Some(crate::mainutils::paste_impl::do_paste),
        "file.path" => Some(crate::mainutils::paste_impl::do_filepath),
        "unique" => Some(crate::mainutils::essentials::do_unique),
        "duplicated" => Some(crate::mainutils::essentials::do_duplicated),
        "anyDuplicated" => Some(crate::mainutils::essentials::do_anyDuplicated),
        "detach" => Some(crate::mainutils::envir::do_detach),
        "data" => Some(crate::mainutils::essentials::do_data),
        "merge" => Some(crate::mainutils::essentials::do_merge),
        "attach" => Some(crate::mainutils::envir::do_attach),
        "as.vector" => Some(crate::mainutils::essentials::do_as_vector),
        "seq" => Some(crate::mainutils::seq::do_seq),
        "split" => Some(crate::mainutils::split::do_split),

        "getRegisteredNamespace" => Some(crate::mainutils::essentials::do_get_registered_namespace),
        "isRegisteredNamespace" => Some(crate::mainutils::essentials::do_is_registered_namespace),

        "load" => Some(crate::mainutils::saveload::do_load),
        "strptime" => Some(crate::mainutils::datetime::do_strptime),
        "isNamespaceEnv" => Some(crate::mainutils::essentials::s4::do_isNamespace),
        "mkUnbound" => Some(do_mkunbound),
        _ => None,
    }
}

/// GNU `do_mkunbound` (envir.c): `.Internal(mkUnbound(name))` sets the
/// symbol's value cell to R_UnboundValue.
pub fn do_mkunbound(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let sym = CAR(args);
    if !sym.is_null() && TYPEOF(sym) == SEXPTYPE::SYMSXP {
        crate::sexp::accessors::SET_SYMVALUE(sym, crate::sexp::globals::R_UnboundValue());
    }
    R_NilValue()
}
pub fn do_add_task_callback(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let fun = CAR(args);
    let data = crate::sexp::accessors::CADR(args);
    let id = crate::mainutils::main::Rf_addTaskCallback(fun, data);
    crate::sexp::constructors::Rf_ScalarInteger(id)
}
pub fn do_remove_task_callback(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let which = CAR(args);
        let ok = crate::mainutils::main::Rf_removeTaskCallback(which);
        crate::sexp::constructors::Rf_ScalarLogical(ok)
    }
}

/// R's `.Internal(builtins(internal))` — sorted builtin/internal name listing.
pub fn do_refcnt(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = if args.is_null() || args == R_NilValue() {
        R_NilValue()
    } else {
        CAR(args)
    };
    // SAFETY: audited boundary — scalar allocation under the active runtime.
    crate::sexp::constructors::Rf_ScalarInteger(crate::sexp::accessors::NAMED(x))
}

/// `.Internal(address(x))` — external pointer whose address is `x`.
pub fn do_address(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = if args.is_null() || args == crate::sexp::globals::R_NilValue() {
        crate::sexp::globals::R_NilValue()
    } else {
        CAR(args)
    };
    // SAFETY: audited boundary — stores the object's address verbatim.
    unsafe {
        crate::mainutils::memory_main::R_MakeExternalPtr(
            x as *mut std::ffi::c_void,
            crate::sexp::globals::R_NilValue(),
            crate::sexp::globals::R_NilValue(),
        )
    }
}

/// `.Internal(named(x))` reports the same NAMED level as `refcnt`.
pub fn do_named(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    do_refcnt(call, op, args, rho)
}

pub fn do_builtins(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let internal = if args.is_null() || args == R_NilValue() {
        false
    } else {
        let first = CAR(args);
        if first.is_null()
            || !matches!(SEXPTYPE(TYPEOF(first)), SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP)
            || LENGTH(first) == 0
        {
            false
        } else {
            let factory = crate::sexp::object::SessionNodeFactory::new(
                // SAFETY: the do_* boundary retains the active runtime.
                unsafe { crate::sexp::owner::OwnerToken::current() }
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
            );
            let value = factory
                .wrap(first)
                .ok()
                .and_then(|first| {
                    if TYPEOF(first.as_raw()) == SEXPTYPE::LGLSXP {
                        first.try_logical_elt(0).ok()
                    } else {
                        first.try_integer_elt(0).ok()
                    }
                })
                .unwrap_or(NA_INTEGER);
            value != FALSE && value != NA_INTEGER
        }
    };

    let mut names = builtin_names_from_funtab(internal);
    names.sort();
    names.dedup();
    string_vector_from_bytes(&names)
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

fn string_vector_from_bytes(values: &[Vec<u8>]) -> SEXP {
    let factory = crate::sexp::object::SessionNodeFactory::new(
        // SAFETY: the do_* boundary retains the active runtime.
        unsafe { crate::sexp::owner::OwnerToken::current() }
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
    );
    let result = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, values.len() as R_xlen_t)));
    let Ok(result) = result else {
        return R_NilValue();
    };
    let Ok(mut result) = crate::sexp::object::SexpMut::try_from_checked(result) else {
        return R_NilValue();
    };
    for (i, value) in values.iter().enumerate() {
        // CString::new rejected interior NUL bytes, yielding an empty string.
        let bytes: &[u8] = if value.contains(&0) { &[] } else { value };
        if let Ok(chars) = factory.allocate(|arena| Some(arena.alloc_charsxp(bytes)))
            && result.try_set_string_elt(i as R_xlen_t, chars).is_ok()
        {}
    }
    result.freeze().as_raw()
}

// ---------------------------------------------------------------------------
// do_tilde (stub)
// ---------------------------------------------------------------------------

/// Implementation of the ~ operator.
/// Creates a formula object with class "formula" and .Environment attribute.
pub fn do_tilde(call: SEXP, _op: SEXP, _args: SEXP, rho: SEXP) -> SEXP {
    if !call.is_null() && OBJECT(call) != 0 {
        duplicate(call)
    } else {
        let klass = crate::sexp::constructors::mk_string_str("formula");
        let _klass_guard = protect(klass);

        let result = duplicate(call);
        let _result_guard = protect(result);

        let class_sym = crate::sexp::symbol::install_str("class");
        setAttrib(result, class_sym, klass);

        let dot_env_sym = crate::sexp::symbol::install_str(".Environment");
        setAttrib(result, dot_env_sym, rho);

        result
    }
}

// ---------------------------------------------------------------------------
// getPRIMNAME
// ---------------------------------------------------------------------------

/// Get the name of a primitive function.
/// For use in packages.
pub fn getPRIMNAME(object: SEXP) -> *const c_char {
    {
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
    use crate::sexp::constructors::Rf_mkChar;
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
            let sym_if = crate::sexp::symbol::install_str("if");
            assert!(!sym_if.is_null());
            let sym_colon = crate::sexp::symbol::install_str(":");
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
        let left = RSession::new();
        let right = RSession::new();

        let mut left_sym0 = ptr::null_mut();
        left.with_active(|| unsafe {
            InitNames();
            left_sym0 = installDDVAL(0);
            assert_eq!(installDDVAL(0), left_sym0);
        });

        right.with_active(|| unsafe {
            InitNames();
            let right_sym0 = installDDVAL(0);
            assert_eq!(installDDVAL(0), right_sym0);
            assert_ne!(right_sym0, left_sym0);
        });

        left.with_active(|| unsafe {
            assert_eq!(installDDVAL(0), left_sym0);
        });
    }

    #[test]
    fn test_mk_sym_marker() {
        let _session = RSession::new();

        unsafe {
            let pname = crate::sexp::constructors::mk_char_str("test");
            let sym = mkSymMarker(pname);
            assert!(!sym.is_null());
            // Value should point to itself
            assert_eq!(crate::sexp::accessors::SYMVALUE(sym), sym);
        }
    }
}
