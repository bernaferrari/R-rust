//! RBG1 graph writer: iterative flat-graph emission.
//!
//! Objects are interned on first sight (assigning the next node index) and
//! emitted in index order. Child references are u32 indices — possibly
//! forward references — which the flat format supports by construction.

use std::collections::HashMap;

use crate::sexp::accessors::*;
use crate::sexp::ffi::{Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::*;

use super::*;

pub struct GraphWriter {
    memo: HashMap<usize, u32>,
    /// Node index → object pointer; index 0 is the canonical nil object.
    objects: Vec<SEXP>,
    /// Node index → emitted record bytes (None until emitted).
    records: Vec<Option<Vec<u8>>>,
    /// Protection guards rooting ALTREP materialized copies for the whole
    /// emission: copies are temporaries outside the original object graph,
    /// so GC could otherwise collect them before their node is written.
    copy_guards: Vec<crate::sexp::protect::ProtectGuard<'static>>,
    /// GNU `refhook`: consulted for persistent (namespace/package) references.
    persist_hook: SEXP,
}

impl GraphWriter {
    pub fn new() -> Self {
        Self {
            memo: HashMap::new(),
            objects: vec![R_NilValue()],
            records: vec![None],
            copy_guards: Vec::new(),
            persist_hook: R_NilValue(),
        }
    }

    pub fn intern_root(&mut self, s: SEXP) -> Result<(), String> {
        self.intern(s);
        Ok(())
    }

    pub fn set_persist_hook(&mut self, hook: SEXP) {
        self.persist_hook = hook;
    }

    /// Assign a node index to `s`; index 0 is the canonical nil object.
    /// ALTREP objects intern their materialized copy instead, exactly like
    /// the GNU writer.
    fn intern(&mut self, s: SEXP) -> u32 {
        if s.is_null() || s == R_NilValue() {
            return 0;
        }
        let s = if TYPEOF(s) != SEXPTYPE::ENVSXP && unsafe { crate::sexp::altrep::has_extension_raw(s) } {
            let object = unsafe { crate::sexp::altrep::rooted_raw(s) }
                .unwrap_or_else(|e| error(&e.to_string()));
            let _operation = crate::sexp::altrep::serialization_guard(&object)
                .unwrap_or_else(|e| error(&e.to_string()));
            let copy = crate::sexp::altrep::materialized_copy(&object)
                .unwrap_or_else(|e| error(&e.to_string()))
                .as_raw();
            self.copy_guards.push(crate::sexp::protect::protect(copy));
            copy
        } else {
            s
        };
        let key = s as usize;
        if let Some(&idx) = self.memo.get(&key) {
            return idx;
        }
        let idx = self.objects.len() as u32;
        self.memo.insert(key, idx);
        self.objects.push(s);
        self.records.push(None);
        idx
    }

    /// Emit every pending node in index order and produce the payload.
    pub fn finish_payload(&mut self) -> Result<Vec<u8>, String> {
        let mut cursor = 1usize;
        while cursor < self.objects.len() {
            if self.records[cursor].is_none() {
                let s = self.objects[cursor];
                let record = self.emit_node(s)?;
                self.records[cursor] = Some(record);
            }
            cursor += 1;
        }
        let count = self.objects.len() as u32;
        let mut out = Vec::new();
        out.extend_from_slice(&count.to_le_bytes());
        for idx in 1..self.records.len() {
            if let Some(record) = self.records[idx].take() {
                out.extend_from_slice(&record);
            }
        }
        Ok(out)
    }

    fn emit_node(&mut self, s: SEXP) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        let stype = SEXPTYPE(TYPEOF(s));
        // Persistent references first (GNU consults the refhook before any
        // other check): namespace/package environments serialize by name.
        if stype == SEXPTYPE::ENVSXP {
            let name = crate::mainutils::serialize::core::persistent_name(s, self.persist_hook);
            if !name.is_null() && name != R_NilValue() {
                out.push(T_PERSIST_NS);
                push_meta(&mut out, s);
                let count = XLENGTH(name) as u32;
                out.extend_from_slice(&count.to_le_bytes());
                for i in 0..count {
                    let bytes = charsxp_as_utf8(STRING_ELT(name, i as _));
                    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                    out.extend_from_slice(&bytes);
                }
                return Ok(out);
            }
        }
        // Special singletons: no payload, resolved at materialization.
        if s == R_GlobalEnv() {
            return Ok(singleton_record(T_GLOBALENV, s));
        }
        if s == R_BaseEnv() {
            return Ok(singleton_record(T_BASEENV, s));
        }
        if s == R_EmptyEnv() {
            return Ok(singleton_record(T_EMPTYENV, s));
        }
        if s == R_UnboundValue() {
            return Ok(singleton_record(T_UNBOUND, s));
        }
        if s == R_MissingArg() {
            return Ok(singleton_record(T_MISSING, s));
        }
        if s == crate::sexp::envir::R_BaseNamespace() {
            return Ok(singleton_record(T_BASENAMESPACE, s));
        }

        match stype {
            SEXPTYPE::SYMSXP => {
                out.push(T_SYM);
                push_meta(&mut out, s);
                let pname = PRINTNAME(s);
                let bytes = charsxp_as_utf8(pname);
                out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                out.extend_from_slice(&bytes);
            }
            SEXPTYPE::CHARSXP => {
                out.push(T_CHAR);
                push_meta(&mut out, s);
                if s == R_NaString() {
                    out.extend_from_slice(&(-1i32).to_le_bytes());
                } else {
                    let bytes = charsxp_as_utf8(s);
                    out.extend_from_slice(&(bytes.len() as i32).to_le_bytes());
                    out.extend_from_slice(&bytes);
                }
            }
            SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP => {
                out.push(if stype == SEXPTYPE::LGLSXP { T_LGL } else { T_INT });
                push_meta(&mut out, s);
                let len = XLENGTH(s);
                out.extend_from_slice(&(len as u32).to_le_bytes());
                let attrib = self.intern(ATTRIB(s));
                out.extend_from_slice(&attrib.to_le_bytes());
                for i in 0..len {
                    let v = if stype == SEXPTYPE::LGLSXP {
                        LOGICAL_ELT(s, i as _)
                    } else {
                        INTEGER_ELT(s, i as _)
                    };
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
            SEXPTYPE::REALSXP => {
                out.push(T_REAL);
                push_meta(&mut out, s);
                let len = XLENGTH(s);
                out.extend_from_slice(&(len as u32).to_le_bytes());
                let attrib = self.intern(ATTRIB(s));
                out.extend_from_slice(&attrib.to_le_bytes());
                for i in 0..len {
                    out.extend_from_slice(&REAL_ELT(s, i as _).to_bits().to_le_bytes());
                }
            }
            SEXPTYPE::CPLXSXP => {
                out.push(T_CPLX);
                push_meta(&mut out, s);
                let len = XLENGTH(s);
                out.extend_from_slice(&(len as u32).to_le_bytes());
                let attrib = self.intern(ATTRIB(s));
                out.extend_from_slice(&attrib.to_le_bytes());
                for i in 0..len {
                    let v: Rcomplex = COMPLEX_ELT(s, i as _);
                    out.extend_from_slice(&v.r.to_bits().to_le_bytes());
                    out.extend_from_slice(&v.i.to_bits().to_le_bytes());
                }
            }
            SEXPTYPE::STRSXP => {
                out.push(T_STR);
                push_meta(&mut out, s);
                let len = XLENGTH(s);
                out.extend_from_slice(&(len as u32).to_le_bytes());
                let attrib = self.intern(ATTRIB(s));
                out.extend_from_slice(&attrib.to_le_bytes());
                for i in 0..len {
                    let elt = self.intern(STRING_ELT(s, i));
                    out.extend_from_slice(&elt.to_le_bytes());
                }
            }
            SEXPTYPE::RAWSXP => {
                out.push(T_RAW);
                push_meta(&mut out, s);
                let len = XLENGTH(s);
                out.extend_from_slice(&(len as u32).to_le_bytes());
                let attrib = self.intern(ATTRIB(s));
                out.extend_from_slice(&attrib.to_le_bytes());
                for i in 0..len {
                    out.push(RAW_ELT(s, i as _));
                }
            }
            SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP | SEXPTYPE::S4SXP => {
                out.push(match stype {
                    SEXPTYPE::VECSXP => T_VEC,
                    SEXPTYPE::EXPRSXP => T_EXPR,
                    _ => T_S4,
                });
                push_meta(&mut out, s);
                let len = XLENGTH(s);
                out.extend_from_slice(&(len as u32).to_le_bytes());
                let attrib = self.intern(ATTRIB(s));
                out.extend_from_slice(&attrib.to_le_bytes());
                for i in 0..len {
                    let elt = self.intern(VECTOR_ELT(s, i));
                    out.extend_from_slice(&elt.to_le_bytes());
                }
            }
            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => {
                out.push(if stype == SEXPTYPE::LISTSXP { T_LIST } else { T_LANG });
                push_meta(&mut out, s);
                for part in [CAR(s), CDR(s), TAG(s), ATTRIB(s)] {
                    let r = self.intern(part);
                    out.extend_from_slice(&r.to_le_bytes());
                }
            }
            SEXPTYPE::CLOSXP => {
                out.push(T_CLOS);
                push_meta(&mut out, s);
                for part in [CLOENV(s), FORMALS(s), BODY(s), ATTRIB(s)] {
                    let r = self.intern(part);
                    out.extend_from_slice(&r.to_le_bytes());
                }
            }
            SEXPTYPE::ENVSXP => {
                // Reject active/locked bindings exactly like the GNU writer.
                let mut binding = FRAME(s);
                while !binding.is_null() && binding != R_NilValue() {
                    let symbol = TAG(binding);
                    if crate::sexp::envir::binding_is_active_raw(s, symbol)
                        || crate::sexp::envir::binding_is_locked_raw(s, symbol)
                    {
                        return Err(
                            "serialization of active or individually locked bindings is not supported"
                                .into(),
                        );
                    }
                    binding = CDR(binding);
                }
                out.push(T_ENV);
                push_meta(&mut out, s);
                out.push(u8::from(crate::sexp::envir::environment_is_locked_raw(s)));
                // Rust bindings are canonical in FRAME; the hash cache is not
                // serialized.
                for part in [ENCLOS(s), FRAME(s), ATTRIB(s)] {
                    let r = self.intern(part);
                    out.extend_from_slice(&r.to_le_bytes());
                }
            }
            SEXPTYPE::PROMSXP => {
                // Never force the promise: snapshot fields through the owner.
                let owner = crate::sexp::owner::OwnerToken::current()
                    .map_err(|e| e.to_string())?;
                let _pin = owner.pin().map_err(|e| e.to_string())?;
                owner.require_active().map_err(|e| e.to_string())?;
                let object = owner.sexp(s).map_err(|e| e.to_string())?;
                let attributes = object.try_attrib().map_err(|e| e.to_string())?;
                let environment = object.try_prenv().map_err(|e| e.to_string())?;
                let value = object.try_prvalue().map_err(|e| e.to_string())?;
                let expression = object.try_prcode().map_err(|e| e.to_string())?;
                out.push(T_PROM);
                push_meta(&mut out, s);
                for part in [
                    environment.as_raw(),
                    value.as_raw(),
                    expression.as_raw(),
                    attributes.as_raw(),
                ] {
                    let r = self.intern(part);
                    out.extend_from_slice(&r.to_le_bytes());
                }
            }
            SEXPTYPE::BUILTINSXP | SEXPTYPE::SPECIALSXP => {
                out.push(if stype == SEXPTYPE::BUILTINSXP {
                    T_BUILTIN
                } else {
                    T_SPECIAL
                });
                push_meta(&mut out, s);
                let offset = PRIMOFFSET(s) as usize;
                let entry = crate::mainutils::names::R_FunTab.get(offset).ok_or("invalid primitive offset")?;
                let mut end = entry.name.len();
                if end > 0 && entry.name[end - 1] == 0 {
                    end -= 1;
                }
                let name = &entry.name[..end];
                out.extend_from_slice(&(name.len() as u32).to_le_bytes());
                out.extend_from_slice(name);
            }
            SEXPTYPE::BCODESXP => {
                return Err(
                    "bytecode serialization is not yet supported in RBG1".into(),
                );
            }
            other => {
                return Err(format!(
                    "RBG1: cannot serialize object of type {}",
                    other.as_c_int()
                ));
            }
        }
        Ok(out)
    }
}

fn error(msg: &str) -> ! {
    crate::sexp::context::r_error(msg.to_string())
}

fn singleton_record(tag: u8, s: SEXP) -> Vec<u8> {
    let mut out = vec![tag];
    push_meta(&mut out, s);
    out
}

fn push_meta(out: &mut Vec<u8>, s: SEXP) {
    out.extend_from_slice(&meta_of(s).to_le_bytes());
}
