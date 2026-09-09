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
#[derive(Clone)]
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
        if !op.clone().is_primitive() {
            return None;
        }

        let table_index = op.clone().try_primoffset().ok()?;
        let entry = fun_tab_descriptor(table_index)?;

        Some(Self {
            op: op.clone(),
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
/// installed. This includes evaluator-backed implementations of functions
/// that are `.Internal` closures in GNU R: while the port exposes those
/// implementations directly as primitives, they still need stable identity
/// when passed as values to higher-order functions. Kind mismatches and
/// Rust-only helper names receive a session-local negative `PRIMOFFSET` so descriptor-driven
/// dispatch can recognize them as noncanonical.
pub unsafe fn make_primitive_binding(name: &str, fallback_kind: SEXPTYPE) -> SEXP {
    unsafe {
        if let Some(index) = fun_tab_index_by_name(name) {
            let entry = &R_FunTab[index as usize];
            if primitive_kind_for_eval(entry.eval) == fallback_kind {
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
            let index = crate::sexp::instance::with_required_current_instance(|instance| {
                let names = &mut (*instance).eval_state.portable_primitive_names;
                if let Some(index) = names.iter().position(|existing| existing == name) {
                    index
                } else {
                    names.push(name.to_owned());
                    names.len() - 1
                }
            });
            let offset = i32::try_from(index)
                .ok()
                .and_then(|n| n.checked_add(2))
                .and_then(i32::checked_neg)
                .expect("portable primitive index exceeds interpreter range");
            SET_PRIMOFFSET(prim, offset);
        }
        prim
    }
}

/// Dispatch identity for helpers without an upstream function-table slot.
pub(crate) fn portable_primitive_name(op: Sexp<'_>) -> Option<String> {
    if !op.clone().is_primitive() {
        return None;
    }
    let offset = op.try_primoffset().ok()?;
    let index = offset.checked_neg()?.checked_sub(2)?;
    let index = usize::try_from(index).ok()?;
    crate::sexp::instance::with_required_current_instance(|instance| unsafe {
        let names = &(*instance).eval_state.portable_primitive_names;
        names.get(index).cloned()
    })
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

/// Primitives that leave R_Visible in the state their (last evaluated)
/// argument/body/handler expression produced. Stock R implements these as
/// closures or funtab `eval` flag-2 entries: the flag is untouched at return,
/// so `eval(parse(text="5"))` inherits the inner expression's visibility and
/// `tryCatch(x, err = function(e) cat(...))` stays invisible when the handler
/// ends in `cat()`. The interpreter must NOT restore a print flag after these
/// run.
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
            | "options"
            | "par"
            | "data"
            | "library"
            | "require"
            | "system"
            | "suppressWarnings"
            | "suppressMessages"
            | "set.seed"
            | "RNGkind"
            // Flow-through evaluation: the result's visibility is whatever
            // the wrapped expression / handler evaluation left behind.
            | "eval"
            | "evalq"
            | "try"
            | "tryCatch"
            | "withCallingHandlers"
            | "withRestarts"
            | "UseMethod"
            | "NextMethod"
            | "callNextMethod"
            // Preserve drawing handlers and S3 methods' own visibility.
            // Custom plotting methods may return visible values.
            | "plot"
            | "plot.default"
            | "lines"
            | "points"
            | "text"
            | "lines.default"
            | "points.default"
            | "text.default"
            | "segments"
            | "arrows"
            | "abline"
            | "rect"
            | "polygon"
            | "title"
            | "box"
            | "axis"
            | "plot.new"
            | "plot.window"
            // Portable grid wrappers return invisible grobs or NULL just as
            // their GNU closures do. Do not overwrite their result flag.
            | "grid.newpage"
            | "grid.draw"
            | "grid.rect"
            | "grid.circle"
            | "grid.lines"
            | "grid.segments"
            | "grid.polygon"
            | "grid.points"
            | "grid.text"
            | "pushViewport"
            | "popViewport"
            | "upViewport"
            | "downViewport"
            | "seekViewport"
    )
}

/// .Internal functions whose results stock R marks invisible (the funtab
/// eval column, or a trailing `invisible()` in the stock R closure wrapper).
/// The interpreter auto-prints every visible top-level expression, so these
/// must clear R_Visible after the handler runs — previously the flag was
/// left at the pre-call value and the leak only surfaced once per-statement
/// auto-printing matched Rscript.
pub fn internal_result_invisible(name: &str) -> bool {
    matches!(
        name,
        "assign"
            | "rm"
            | "save"
            | "saveRDS"
            | "load"
            | ".libPaths"
            | "pushBack"
            | "sys.source"
            | "writeLines"
            | "writeBin"
            | "writeChar"
            | "write.table"
            | "write.csv2"
            | "close"
            | "flush"
            | "sink"
            | "unlink"
            | "dir.create"
            | "Sys.setenv"
            | "Sys.unsetenv"
            | "setClass"
            | "setGeneric"
            | "setMethod"
            | "unlockBinding"
            | "lockBinding"
            | "lockEnvironment"
            | "makeActiveBinding"
            | "attach"
            | "detach"
            | "source"
            | "layout"
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
        assert!(unsafe { crate::sexp::accessors::PRIMOFFSET(primitive) } <= -2);
    }

    #[test]
    fn portable_identity_is_reused_without_adding_r_attributes() {
        let _session = RSession::new();
        let first = unsafe { make_primitive_binding("__rport_helper__", SEXPTYPE::BUILTINSXP) };
        let _first = crate::sexp::protect::protect(first);
        let second = unsafe { make_primitive_binding("__rport_helper__", SEXPTYPE::BUILTINSXP) };
        assert_eq!(
            unsafe { crate::sexp::accessors::PRIMOFFSET(first) },
            unsafe { crate::sexp::accessors::PRIMOFFSET(second) }
        );
        assert_eq!(
            portable_primitive_name(Sexp::try_from_raw(first).unwrap()).as_deref(),
            Some("__rport_helper__")
        );
        let attributes = unsafe { crate::sexp::accessors::ATTRIB(first) };
        assert!(
            attributes.is_null() || attributes == unsafe { crate::sexp::globals::R_NilValue() }
        );
    }

    #[test]
    fn evaluator_backed_internal_binding_keeps_function_identity() {
        let _session = RSession::new();
        let primitive = unsafe { make_primitive_binding("paste", SEXPTYPE::BUILTINSXP) };
        let descriptor =
            unsafe { PrimitiveDescriptor::from_raw(primitive) }.expect("paste descriptor");

        assert_eq!(descriptor.name, "paste");
        assert_eq!(descriptor.kind, SEXPTYPE::BUILTINSXP.as_c_int());
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
        assert!(unsafe { crate::sexp::accessors::PRIMOFFSET(primitive) } <= -2);
    }
}
