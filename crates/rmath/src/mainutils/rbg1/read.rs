//! RBG1 graph reader: two-pass materialization.
//!
//! Pass 1 parses all node records. Pass 2a allocates every object (rooted in
//! a protected vector so GC cannot collect half-linked nodes). Pass 2b links
//! references and fills payloads. True cycles are handled by construction.

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::*;

use super::*;

pub struct GraphReader<'a> {
    payload: &'a [u8],
    pos: usize,
}

struct Parsed {
    tag: u8,
    meta: u32,
    refs: Vec<u32>,
    data: Vec<u8>,
    len: u32,
    extra: u8,
}

impl<'a> GraphReader<'a> {
    pub fn new(payload: &'a [u8]) -> Self {
        Self { payload, pos: 0 }
    }

    fn u8(&mut self) -> Result<u8, String> {
        let b = *self.payload.get(self.pos).ok_or("RBG1: truncated stream")?;
        self.pos += 1;
        Ok(b)
    }

    fn u32(&mut self) -> Result<u32, String> {
        let end = self.pos + 4;
        let bytes = self.payload.get(self.pos..end).ok_or("RBG1: truncated stream")?;
        self.pos = end;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }

    fn u64(&mut self) -> Result<u64, String> {
        let end = self.pos + 8;
        let bytes = self.payload.get(self.pos..end).ok_or("RBG1: truncated stream")?;
        self.pos = end;
        Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn bytes(&mut self, n: usize) -> Result<Vec<u8>, String> {
        let end = self.pos.checked_add(n).ok_or("RBG1: length overflow")?;
        let slice = self.payload.get(self.pos..end).ok_or("RBG1: truncated stream")?;
        self.pos = end;
        Ok(slice.to_vec())
    }

    pub fn read(mut self) -> Result<SEXP, String> {
        self.read_hooked(R_NilValue())
    }

    pub fn read_hooked(mut self, hook: SEXP) -> Result<SEXP, String> {
        let node_count = self.u32()? as usize;
        if node_count == 0 || node_count > (1 << 28) {
            return Err("RBG1: invalid node count".into());
        }
        // Pass 1: parse every record.
        let mut nodes: Vec<Parsed> = Vec::with_capacity(node_count);
        nodes.push(Parsed { tag: 0, meta: 0, refs: vec![], data: vec![], len: 0, extra: 0 });
        for idx in 1..node_count {
            nodes.push(self.parse_node(idx)?);
        }
        if self.pos != self.payload.len() {
            return Err("RBG1: trailing bytes after final node".into());
        }
        materialize(&nodes, hook)
    }

    fn parse_node(&mut self, _idx: usize) -> Result<Parsed, String> {
        let tag = self.u8()?;
        let meta = self.u32()?;
        let mut refs = Vec::new();
        let mut data = Vec::new();
        let mut len = 0u32;
        let mut extra = 0u8;
        match tag {
            T_SYM | T_BUILTIN | T_SPECIAL => {
                let n = self.u32()? as usize;
                data = self.bytes(n)?;
            }
            T_CHAR => {
                let n = self.i32()?;
                if n >= 0 {
                    len = n as u32;
                    data = self.bytes(n as usize)?;
                } else {
                    len = u32::MAX; // NA marker
                }
            }
            T_LGL | T_INT => {
                len = self.u32()?;
                refs.push(self.u32()?);
                data = self.bytes(len as usize * 4)?;
            }
            T_REAL => {
                len = self.u32()?;
                refs.push(self.u32()?);
                data = self.bytes(len as usize * 8)?;
            }
            T_CPLX => {
                len = self.u32()?;
                refs.push(self.u32()?);
                data = self.bytes(len as usize * 16)?;
            }
            T_STR | T_VEC | T_EXPR | T_S4 | T_BCODE => {
                len = self.u32()?;
                refs.push(self.u32()?); // attrib
                for _ in 0..len {
                    refs.push(self.u32()?);
                }
            }
            T_RAW => {
                len = self.u32()?;
                refs.push(self.u32()?);
                data = self.bytes(len as usize)?;
            }
            T_LIST | T_LANG | T_CLOS | T_PROM => {
                for _ in 0..4 {
                    refs.push(self.u32()?);
                }
            }
            T_ENV => {
                extra = self.u8()?;
                for _ in 0..3 {
                    refs.push(self.u32()?);
                }
            }
            T_PERSIST_NS => {
                let count = self.u32()?;
                len = count;
                for _ in 0..count {
                    let n = self.u32()? as usize;
                    let bytes = self.bytes(n)?;
                    data.extend_from_slice(&(n as u32).to_le_bytes());
                    data.extend_from_slice(&bytes);
                }
            }
            T_NAMESPACE => {
                let n = self.u32()? as usize;
                let bytes = self.bytes(n)?;
                data.extend_from_slice(&bytes);
                len = 1;
            }
            T_GLOBALENV | T_BASEENV | T_EMPTYENV | T_UNBOUND | T_MISSING | T_BASENAMESPACE => {}
            other => return Err(format!("RBG1: unknown node tag {other}")),
        }
        Ok(Parsed { tag, meta, refs, data, len, extra })
    }
}

/// Resolve a child reference, validating the index.
fn child(objects: &[SEXP], idx: u32) -> Result<SEXP, String> {
    objects
        .get(idx as usize)
        .copied()
        .ok_or_else(|| format!("RBG1: reference to missing node {idx}"))
}

fn materialize(nodes: &[Parsed], hook: SEXP) -> Result<SEXP, String> {
    let count = nodes.len();
    // Root table: a protected vector holding every materialized object so GC
    // cannot collect half-linked nodes during allocation.
    let roots = Rf_allocVector3(SEXPTYPE::VECSXP, count as _);
    let _roots_guard = crate::sexp::protect::protect(roots);
    // Pass 2a: allocate every object with its final type.
    let mut objects: Vec<SEXP> = vec![R_NilValue(); count];
    objects[0] = R_NilValue();
    for (idx, node) in nodes.iter().enumerate().skip(1) {
        let obj = allocate_node(node, hook)?;
        objects[idx] = obj;
        SET_VECTOR_ELT(roots, (idx - 1) as _, obj);
    }
    // Pass 2b: link references and fill payloads.
    for (idx, node) in nodes.iter().enumerate().skip(1) {
        link_node(node, objects[idx], &objects)?;
    }
    // Pass 2c: reconstruct environment frames now that every pairlist cell
    // is fully linked. Bindings go through defineVar so the host binding
    // representation (frame/hash) stays canonical.
    for (idx, node) in nodes.iter().enumerate().skip(1) {
        if node.tag != T_ENV {
            continue;
        }
        let obj = objects[idx];
        let mut cell = child(&objects, node.refs[1])?;
        let mut seen = std::collections::HashSet::new();
        while !cell.is_null() && cell != R_NilValue() {
            if TYPEOF(cell) != SEXPTYPE::LISTSXP || !seen.insert(cell as usize) {
                return Err("RBG1: invalid or cyclic environment binding list".into());
            }
            if LEVELS(cell) & ((1 << 15) | (1 << 14)) != 0 {
                return Err(
                    "restoration of active or individually locked bindings is not supported"
                        .into(),
                );
            }
            let symbol = TAG(cell);
            if symbol.is_null() || TYPEOF(symbol) != SEXPTYPE::SYMSXP {
                return Err("RBG1: invalid environment binding name".into());
            }
            crate::sexp::envir::defineVar(symbol, CAR(cell), obj);
            cell = CDR(cell);
        }
        if node.extra != 0 {
            crate::sexp::envir::lock_environment_raw(obj);
        }
    }
    // A nil root interned as index 0 produces no records: the object is nil.
    Ok(objects.get(1).copied().unwrap_or_else(R_NilValue))
}


/// Copy bytes into a fresh CHARSXP node.
fn mk_charsxp(bytes: &[u8]) -> SEXP {
    unsafe {
        crate::sexp::constructors::Rf_mkCharLen(bytes.as_ptr() as *const std::os::raw::c_char, bytes.len() as std::os::raw::c_int)
    }
}

fn allocate_node(node: &Parsed, hook: SEXP) -> Result<SEXP, String> {
    Ok(match node.tag {
        T_SYM => {
            let name = String::from_utf8_lossy(&node.data).into_owned();
            crate::sexp::symbol::install_str(&name)
        }
        T_CHAR => {
            if node.len == u32::MAX {
                R_NaString()
            } else if let Some(interned) = crate::sexp::symbol::interned_ascii_character(&node.data) {
                interned
            } else {
                mk_charsxp(&node.data)
            }
        }
        T_LGL => Rf_allocVector3(SEXPTYPE::LGLSXP, node.len as _),
        T_INT => Rf_allocVector3(SEXPTYPE::INTSXP, node.len as _),
        T_REAL => Rf_allocVector3(SEXPTYPE::REALSXP, node.len as _),
        T_CPLX => Rf_allocVector3(SEXPTYPE::CPLXSXP, node.len as _),
        T_STR => Rf_allocVector3(SEXPTYPE::STRSXP, node.len as _),
        T_RAW => Rf_allocVector3(SEXPTYPE::RAWSXP, node.len as _),
        T_VEC => Rf_allocVector3(SEXPTYPE::VECSXP, node.len as _),
        T_EXPR => Rf_allocVector3(SEXPTYPE::EXPRSXP, node.len as _),
        T_S4 => Rf_allocVector3(SEXPTYPE::S4SXP, node.len as _),
        T_BCODE => Rf_allocVector3(SEXPTYPE::BCODESXP, node.len as _),
        T_LIST => crate::sexp::memory_ext::allocSExp(SEXPTYPE::LISTSXP),
        T_LANG => crate::sexp::memory_ext::allocSExp(SEXPTYPE::LANGSXP),
        T_CLOS => crate::sexp::memory_ext::allocSExp(SEXPTYPE::CLOSXP),
        T_ENV => crate::sexp::memory_ext::NewEnvironment(R_NilValue(), R_BaseEnv(), R_NilValue()),
        T_PROM => crate::sexp::memory_ext::allocSExp(SEXPTYPE::PROMSXP),
        T_BUILTIN | T_SPECIAL => {
            let name = String::from_utf8_lossy(&node.data).into_owned();
            resolve_primitive(&name, node.tag)?
        }
        T_GLOBALENV => R_GlobalEnv(),
        T_BASEENV => R_BaseEnv(),
        T_EMPTYENV => R_EmptyEnv(),
        T_UNBOUND => R_UnboundValue(),
        T_MISSING => R_MissingArg(),
        T_BASENAMESPACE => crate::sexp::envir::R_BaseNamespace(),
        T_NAMESPACE => {
            let name = String::from_utf8_lossy(&node.data).to_string();
            if name.is_empty() || name == "base" {
                R_BaseEnv()
            } else {
                crate::mainutils::essentials::load_package_namespace_by_name(&name).unwrap_or_else(
                    |_| {
                        crate::sexp::memory_ext::NewEnvironment(
                            R_NilValue(),
                            R_BaseEnv(),
                            R_NilValue(),
                        )
                    },
                )
            }
        }
        T_PERSIST_NS => {
            if hook.is_null() || hook == R_NilValue() {
                return Err("RBG1: persistent reference requires a restore hook".into());
            }
            // Rebuild the persisted names as a STRSXP and invoke the hook,
            // exactly like GNU's persistent-restore path.
            let mut offset = 0usize;
            let mut count = 0usize;
            while offset + 4 <= node.data.len() {
                let n = u32::from_le_bytes(node.data[offset..offset + 4].try_into().unwrap()) as usize;
                offset += 4 + n;
                count += 1;
            }
            let names = Rf_allocVector3(SEXPTYPE::STRSXP, count as _);
            let _names_guard = crate::sexp::protect::protect(names);
            offset = 0;
            for i in 0..count {
                let n = u32::from_le_bytes(node.data[offset..offset + 4].try_into().unwrap()) as usize;
                offset += 4;
                let charsxp = mk_charsxp(&node.data[offset..offset + n]);
                offset += n;
                SET_STRING_ELT(names, i as _, charsxp);
            }
            crate::mainutils::serialize::lazyload::CallHook(names, hook)
        }
        other => return Err(format!("RBG1: cannot materialize tag {other}")),
    })
}

fn link_node(node: &Parsed, obj: SEXP, objects: &[SEXP]) -> Result<(), String> {
    match node.tag {
        T_SYM | T_BUILTIN | T_SPECIAL => {
            restore_meta(obj, node.meta);
        }
        T_CHAR => {
            restore_meta(obj, node.meta);
        }
        T_LGL | T_INT => {
            let attrib = child(objects, node.refs[0])?;
            SET_ATTRIB(obj, attrib);
            let is_lgl = node.tag == T_LGL;
            for i in 0..node.len as usize {
                let v = i32::from_le_bytes(node.data[i * 4..i * 4 + 4].try_into().unwrap());
                if is_lgl {
                    SET_LOGICAL_ELT(obj, i as _, v);
                } else {
                    SET_INTEGER_ELT(obj, i as _, v);
                }
            }
            restore_meta(obj, node.meta);
        }
        T_REAL => {
            let attrib = child(objects, node.refs[0])?;
            SET_ATTRIB(obj, attrib);
            for i in 0..node.len as usize {
                let bits = u64::from_le_bytes(node.data[i * 8..i * 8 + 8].try_into().unwrap());
                SET_REAL_ELT(obj, i as _, f64::from_bits(bits));
            }
            restore_meta(obj, node.meta);
        }
        T_CPLX => {
            let attrib = child(objects, node.refs[0])?;
            SET_ATTRIB(obj, attrib);
            for i in 0..node.len as usize {
                let r = u64::from_le_bytes(node.data[i * 16..i * 16 + 8].try_into().unwrap());
                let im = u64::from_le_bytes(node.data[i * 16 + 8..i * 16 + 16].try_into().unwrap());
                SET_COMPLEX_ELT(obj, i as _, Rcomplex { r: f64::from_bits(r), i: f64::from_bits(im) });
            }
            restore_meta(obj, node.meta);
        }
        T_STR => {
            let attrib = child(objects, node.refs[0])?;
            SET_ATTRIB(obj, attrib);
            for i in 0..node.len as usize {
                let elt = child(objects, node.refs[1 + i])?;
                SET_STRING_ELT(obj, i as _, elt);
            }
            restore_meta(obj, node.meta);
        }
        T_RAW => {
            let attrib = child(objects, node.refs[0])?;
            SET_ATTRIB(obj, attrib);
            for i in 0..node.len as usize {
                SET_RAW_ELT(obj, i as _, node.data[i]);
            }
            restore_meta(obj, node.meta);
        }
        T_VEC | T_EXPR | T_S4 | T_BCODE => {
            let attrib = child(objects, node.refs[0])?;
            SET_ATTRIB(obj, attrib);
            for i in 0..node.len as usize {
                let elt = child(objects, node.refs[1 + i])?;
                SET_VECTOR_ELT(obj, i as _, elt);
            }
            restore_meta(obj, node.meta);
        }
        T_LIST | T_LANG => {
            let car = child(objects, node.refs[0])?;
            let cdr = child(objects, node.refs[1])?;
            let tag = child(objects, node.refs[2])?;
            let attrib = child(objects, node.refs[3])?;
            SETCAR(obj, car);
            SETCDR(obj, cdr);
            SETTAG(obj, tag);
            SET_ATTRIB(obj, attrib);
            restore_meta(obj, node.meta);
        }
        T_CLOS => {
            let env = child(objects, node.refs[0])?;
            let formals = child(objects, node.refs[1])?;
            let body = child(objects, node.refs[2])?;
            let attrib = child(objects, node.refs[3])?;
            SET_CLOENV(obj, env);
            SET_FORMALS(obj, formals);
            SET_BODY(obj, body);
            SET_ATTRIB(obj, attrib);
            restore_meta(obj, node.meta);
        }
        T_ENV => {
            // Frame reconstruction is deferred to pass 2c: the binding
            // pairlist cells are validated structurally, so they must be
            // fully linked first — and cyclic environments can order the
            // frame nodes after the environment node itself.
            let enclos = child(objects, node.refs[0])?;
            let attrib = child(objects, node.refs[2])?;
            if !enclos.is_null() && enclos != R_NilValue() && TYPEOF(enclos) == SEXPTYPE::ENVSXP {
                SET_ENCLOS(obj, enclos);
            }
            SET_ATTRIB(obj, attrib);
            restore_meta(obj, node.meta);
        }
        T_PROM => {
            let env = child(objects, node.refs[0])?;
            let value = child(objects, node.refs[1])?;
            let expr = child(objects, node.refs[2])?;
            let attrib = child(objects, node.refs[3])?;
            fill_promise(obj, env, value, expr, attrib, node.meta)?;
        }
        T_GLOBALENV | T_BASEENV | T_EMPTYENV | T_UNBOUND | T_MISSING | T_BASENAMESPACE | T_PERSIST_NS | T_NAMESPACE => {}
        other => return Err(format!("RBG1: cannot link tag {other}")),
    }
    Ok(())
}

/// Fill promise fields through the arena node-body idiom: promise state is
/// not shaped like ordinary CAR/CDR nodes.
fn fill_promise(
    promise: SEXP,
    env: SEXP,
    value: SEXP,
    expr: SEXP,
    attrib: SEXP,
    meta: u32,
) -> Result<(), String> {
    let owner = crate::sexp::owner::OwnerToken::current().map_err(|e| e.to_string())?;
    let _pin = owner.pin().map_err(|e| e.to_string())?;
    owner.require_active().map_err(|e| e.to_string())?;
    let factory = owner.node_factory();
    let env_o = factory.wrap(env).map_err(|e| e.to_string())?;
    let value_o = factory.wrap(value).map_err(|e| e.to_string())?;
    let expr_o = factory.wrap(expr).map_err(|e| e.to_string())?;
    let attrib_o = factory.wrap(attrib).map_err(|e| e.to_string())?;
    let promise_o = factory.wrap(promise).map_err(|e| e.to_string())?;
    let attributes_link = factory.link(&attrib_o).map_err(|e| e.to_string())?;
    let body = crate::sexp::ffi::NodeBody::Promise(crate::sexp::ffi::Promsxp {
        value: factory.link(&value_o).map_err(|e| e.to_string())?,
        expr: factory.link(&expr_o).map_err(|e| e.to_string())?,
        env: factory.link(&env_o).map_err(|e| e.to_string())?,
    });
    factory
        .allocate(|arena| {
            let node = arena.node_token(promise_o.as_raw())?;
            let heap = arena.heap_identity();
            let mut header = heap.node_snapshot(&node)?;
            header.data = body;
            header.attrib = attributes_link;
            header.sxpinfo.set_gp((meta & 0xFFFF) as u16);
            header.sxpinfo.set_obj((meta >> 16) != 0);
            heap.replace_node(&node, header)?;
            Some(promise_o.as_raw())
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn resolve_primitive(name: &str, tag: u8) -> Result<SEXP, String> {
    let name_cstr = std::ffi::CString::new(name).map_err(|_| "invalid primitive name".to_string())?;
    let idx = unsafe { crate::mainutils::names::StrToInternal(name_cstr.as_ptr()) };
    let kind = if tag == T_BUILTIN {
        SEXPTYPE::BUILTINSXP
    } else {
        SEXPTYPE::SPECIALSXP
    };
    if idx == crate::sexp::ffi::NA_INTEGER {
        // Rust-only portable primitives serialize by registry name; recreate
        // them through the same portable registry the writer consulted.
        let prim = unsafe { crate::eval::primitive::make_primitive_binding(name, kind) };
        if prim.is_null() {
            return Err(format!("RBG1: unknown primitive '{name}'"));
        }
        return Ok(prim);
    }
    let entry = &crate::mainutils::names::R_FunTab[idx as usize];
    let eval_type = entry.eval % 10;
    Ok(unsafe { crate::mainutils::dstruct::mkPRIMSXP(idx, eval_type) })
}
