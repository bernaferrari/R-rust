//! Primitive metadata for R builtins and special forms.
#![deny(unsafe_op_in_unsafe_fn)]

use std::os::raw::c_int;

use crate::mainutils::names::{FunTabEntry, R_FunTab};
use crate::sexp::accessors::SET_PRIMOFFSET;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::memory_ext::allocSExp;
use crate::sexp::object::Sexp;

/// Function pointer type for primitive functions (SPECIAL and BUILTIN).
pub type PrimFun = unsafe extern "C" fn(
    SEXP, // call
    SEXP, // op (the function)
    SEXP, // args
    SEXP, // rho (environment)
) -> SEXP;

/// Rust-shaped view over an R primitive descriptor.
#[derive(Clone, Copy)]
pub struct PrimitiveDescriptor<'a> {
    pub op: Sexp<'a>,
    pub table_index: c_int,
    pub operation_code: c_int,
    pub kind: c_int,
    pub name: &'static str,
    pub print_flag: c_int,
    pub fun: Option<PrimFun>,
}

impl<'a> PrimitiveDescriptor<'a> {
    pub fn from_sexp(op: Sexp<'a>) -> Option<Self> {
        if !op.is_primitive() {
            return None;
        }

        let table_index = op.try_primoffset().ok()?;
        let entry = fun_tab_descriptor(table_index)?;

        Some(Self {
            op,
            table_index,
            operation_code: entry.offset,
            kind: op.typeof_().as_c_int(),
            name: fun_tab_name(entry.name),
            print_flag: primitive_print_flag(entry),
            fun: entry.cfun,
        })
    }

    /// Wrap a raw primitive pointer for internal C-port boundaries.
    ///
    /// Rust evaluator code that already has an owner-scoped `Sexp` should use
    /// [`from_sexp`](Self::from_sexp) instead.
    pub unsafe fn from_raw(op: SEXP) -> Option<Self> {
        let op = Sexp::try_from_raw(op).ok()?;
        Self::from_sexp(op)
    }
}

/// Get the primitive function pointer for a SPECIAL or BUILTIN.
pub unsafe fn get_primfun(op: SEXP) -> Option<PrimFun> {
    unsafe { PrimitiveDescriptor::from_raw(op) }.and_then(|primitive| primitive.fun)
}

/// Get a function table entry by primitive table index.
pub fn get_fun_tab_entry(table_index: c_int) -> Option<PrimFun> {
    fun_tab_descriptor(table_index).and_then(|entry| entry.cfun)
}

pub fn fun_tab_descriptor(table_index: c_int) -> Option<&'static FunTabEntry> {
    if table_index < 0 {
        return None;
    }
    let idx = table_index as usize;
    R_FunTab.get(idx).filter(|entry| !entry.is_sentinel())
}

pub fn fun_tab_len() -> usize {
    R_FunTab.len()
}

pub fn fun_tab_name(name: &'static [u8]) -> &'static str {
    let bytes = name.strip_suffix(&[0]).unwrap_or(name);
    std::str::from_utf8(bytes).unwrap_or("unknown")
}

pub fn fun_tab_index_by_name(name: &str) -> Option<c_int> {
    R_FunTab
        .iter()
        .take_while(|entry| !entry.is_sentinel())
        .position(|entry| fun_tab_name(entry.name) == name)
        .map(|idx| idx as c_int)
}

/// Create a primitive binding with canonical `R_FunTab` identity when possible.
///
/// Primitive names present in `R_FunTab` are allocated with their real table
/// offset when the table's argument-evaluation kind matches the binding being
/// installed. `.Internal` entries, kind mismatches, and Rust-only helper names
/// are still exposed as primitives for the current evaluator surface, but
/// receive `PRIMOFFSET = -1` so descriptor-driven dispatch can recognize them
/// as noncanonical.
pub unsafe fn make_primitive_binding(name: &str, fallback_kind: SEXPTYPE) -> SEXP {
    unsafe {
        if let Some(index) = fun_tab_index_by_name(name) {
            let entry = &R_FunTab[index as usize];
            if (entry.eval % 100) / 10 == 0 && primitive_kind_for_eval(entry.eval) == fallback_kind
            {
                let prim = crate::mainutils::dstruct::mkPRIMSXP(index, entry.eval % 10);
                if !prim.is_null() {
                    (*prim).sxpinfo.set_gp(1);
                }
                return prim;
            }
        }

        let prim = allocSExp(fallback_kind);
        if !prim.is_null() {
            (*prim).sxpinfo.set_gp(1);
            SET_PRIMOFFSET(prim, -1);
        }
        prim
    }
}

fn primitive_print_flag(entry: &FunTabEntry) -> c_int {
    (entry.eval / 100) % 10
}

/// Check the PRIMPRINT flag (visibility hint for primitives).
pub unsafe fn PRIMPRINT(op: SEXP) -> c_int {
    unsafe { PrimitiveDescriptor::from_raw(op) }
        .map(|primitive| primitive.print_flag)
        .unwrap_or(0)
}

/// Get the PRIMNAME for a primitive.
pub unsafe fn PRIMNAME(op: SEXP) -> &'static str {
    unsafe { PrimitiveDescriptor::from_raw(op) }
        .map(|primitive| primitive.name)
        .unwrap_or("unknown")
}

pub fn primitive_controls_visibility(name: &str) -> bool {
    matches!(
        name,
        "<-" | "<<-"
            | "="
            | "{"
            | "("
            | "if"
            | "for"
            | "while"
            | "repeat"
            | "return"
            | "invisible"
            | "on.exit"
            | "withVisible"
            | "cat"
            | "print"
            | "warning"
            | "message"
            | "stopifnot"
            | "library"
            | "require"
            | "system"
            | "suppressWarnings"
            | "suppressMessages"
            | "set.seed"
            | "RNGkind"
    )
}

pub fn primitive_kind_for_eval(eval: c_int) -> SEXPTYPE {
    if eval % 10 == 0 {
        SEXPTYPE::SPECIALSXP
    } else {
        SEXPTYPE::BUILTINSXP
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;

    #[test]
    fn descriptor_exposes_funtab_metadata() {
        let _session = RSession::new();
        let primitive = unsafe { crate::mainutils::names::R_Primitive(c"+".as_ptr()) };
        let descriptor =
            unsafe { PrimitiveDescriptor::from_raw(primitive) }.expect("primitive descriptor");

        assert_eq!(descriptor.name, "+");
        assert_eq!(descriptor.kind, SEXPTYPE::BUILTINSXP.as_c_int());
        assert!(descriptor.table_index >= 0);
        assert_eq!(unsafe { PRIMNAME(primitive) }, "+");
        assert_eq!(unsafe { PRIMPRINT(primitive) }, descriptor.print_flag);
    }

    #[test]
    fn print_flag_matches_upstream_macro_shape() {
        let if_entry = R_FunTab
            .iter()
            .find(|entry| fun_tab_name(entry.name) == "if")
            .expect("if primitive exists");
        assert_eq!(primitive_print_flag(if_entry), 2);

        let plus_entry = R_FunTab
            .iter()
            .find(|entry| fun_tab_name(entry.name) == "+")
            .expect("+ primitive exists");
        assert_eq!(primitive_print_flag(plus_entry), 0);
    }

    #[test]
    fn primitive_binding_uses_canonical_funtab_identity() {
        let _session = RSession::new();
        let primitive = unsafe { make_primitive_binding("+", SEXPTYPE::BUILTINSXP) };
        let descriptor =
            unsafe { PrimitiveDescriptor::from_raw(primitive) }.expect("canonical descriptor");

        assert_eq!(descriptor.name, "+");
        assert_eq!(descriptor.table_index, fun_tab_index_by_name("+").unwrap());
        assert_eq!(descriptor.kind, SEXPTYPE::BUILTINSXP.as_c_int());
    }

    #[test]
    fn primitive_binding_preserves_funtab_kind() {
        let _session = RSession::new();
        let primitive = unsafe { make_primitive_binding("if", SEXPTYPE::SPECIALSXP) };
        let descriptor =
            unsafe { PrimitiveDescriptor::from_raw(primitive) }.expect("canonical descriptor");

        assert_eq!(descriptor.name, "if");
        assert_eq!(descriptor.kind, SEXPTYPE::SPECIALSXP.as_c_int());
    }

    #[test]
    fn rust_only_primitive_binding_is_explicitly_noncanonical() {
        let _session = RSession::new();
        let primitive = unsafe { make_primitive_binding("__rport_helper__", SEXPTYPE::BUILTINSXP) };

        assert!(unsafe { PrimitiveDescriptor::from_raw(primitive) }.is_none());
        assert_eq!(unsafe { crate::sexp::accessors::PRIMOFFSET(primitive) }, -1);
    }

    #[test]
    fn mismatched_funtab_binding_is_explicitly_noncanonical() {
        let _session = RSession::new();
        let primitive = unsafe { make_primitive_binding("log", SEXPTYPE::BUILTINSXP) };

        assert!(unsafe { PrimitiveDescriptor::from_raw(primitive) }.is_none());
        assert_eq!(
            Sexp::try_from_raw(primitive).unwrap().typeof_(),
            SEXPTYPE::BUILTINSXP
        );
        assert_eq!(unsafe { crate::sexp::accessors::PRIMOFFSET(primitive) }, -1);
    }
}
