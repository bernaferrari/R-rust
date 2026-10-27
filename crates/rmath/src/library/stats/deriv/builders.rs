#![forbid(unsafe_code)]
//! Owning construction of the auxiliary syntax emitted by GNU deriv.c.

use crate::sexp::{
    ffi::SEXPTYPE,
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::RuntimeAccess,
};
type Value = Sexp<'static>;

pub(super) struct Builder<'a> {
    access: &'a RuntimeAccess,
}
impl<'a> Builder<'a> {
    pub(super) fn new(access: &'a RuntimeAccess) -> Self {
        Self { access }
    }
    pub(super) fn symbol(&self, name: &str) -> SexpResult<Value> {
        super::derivative_symbol(self.access, name)
    }
    pub(super) fn call(&self, name: &str, arguments: &[Value]) -> SexpResult<Value> {
        let function = self.symbol(name)?;
        let domain = self.access.domain();
        let allocator = self.access.allocator(&domain)?;
        let nil = domain.nil();
        let mut tail = nil.clone();
        for value in arguments.iter().rev() {
            tail = allocator.pairlist_cell(value, &tail, &nil)?;
        }
        allocator.call(&function, &tail)
    }
    fn integer(&self, value: i32) -> SexpResult<Value> {
        let domain = self.access.domain();
        self.access.allocator(&domain)?.allocate(|arena| {
            let raw = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
            let node = arena.node_token(raw)?;
            arena
                .heap_identity()
                .payload_lease(&node)?
                .set_integer_elt(0, value)?;
            Some(raw)
        })
    }
    fn zero(&self) -> SexpResult<Value> {
        let domain = self.access.domain();
        self.access.allocator(&domain)?.allocate(|arena| {
            let raw = arena.alloc_vector(SEXPTYPE::REALSXP, 1);
            let node = arena.node_token(raw)?;
            arena
                .heap_identity()
                .payload_lease(&node)?
                .set_real_elt(0, 0.)?;
            Some(raw)
        })
    }
    fn string(&self, character: &Value) -> SexpResult<Value> {
        let domain = self.access.domain();
        let value = self
            .access
            .allocator(&domain)?
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, 1)))?;
        let mut value = SexpMut::try_from_checked(value)?;
        value.try_set_string_elt(0, character.clone())?;
        Ok(value.freeze())
    }
    fn text(&self, text: &str) -> SexpResult<Value> {
        let domain = self.access.domain();
        self.access.allocator(&domain)?.strings(&[text])
    }
    fn names_call(&self, names: &Value) -> SexpResult<Value> {
        let domain = self.access.domain();
        let nil = domain.nil();
        let allocator = self.access.allocator(&domain)?;
        let mut head = nil.clone();
        let mut last = nil.clone();
        // Read providers in GNU's increasing index order. Owning head/last
        // retain all constructed cells without a native Vec of raw SEXPs.
        for index in 0..names.len() {
            let character = names.try_string_elt(index)?.into_owned()?;
            self.access.require_active()?;
            let value = self.string(&character)?;
            let cell = allocator.pairlist_cell(&value, &nil, &nil)?;
            if head.is_nil() {
                head = cell.clone();
            } else {
                SexpMut::try_from_checked(last)?.try_set_pairlist_cdr(&cell)?;
            }
            last = cell;
        }
        allocator.call(&self.symbol("c")?, &head)
    }
    pub(super) fn initialize(&self, names: &Value, hessian: bool) -> SexpResult<Value> {
        self.access.domain().link(names)?;
        let count = i32::try_from(names.len()).map_err(|_| SexpError::EvaluationFailed {
            message: "too many derivative variables".into(),
        })?;
        let names_call = self.names_call(names)?;
        let dimnames = if hessian {
            let copied = super::duplicate_builder_value(self.access, &names_call)?;
            self.call("list", &[self.access.domain().nil(), names_call, copied])?
        } else {
            self.call("list", &[self.access.domain().nil(), names_call])?
        };
        let length = self.call("length", &[self.symbol(".value")?])?;
        let dimensions = if hessian {
            self.call("c", &[length, self.integer(count)?, self.integer(count)?])?
        } else {
            self.call("c", &[length, self.integer(count)?])?
        };
        let array = self.call("array", &[self.zero()?, dimensions, dimnames])?;
        self.call(
            "<-",
            &[
                self.symbol(if hessian { ".hessian" } else { ".grad" })?,
                array,
            ],
        )
    }
    pub(super) fn assign_gradient(&self, name: &Value, expression: &Value) -> SexpResult<Value> {
        let target = self.call(
            "[",
            &[
                self.symbol(".grad")?,
                self.access.domain().missing(),
                self.string(name)?,
            ],
        )?;
        self.call("<-", &[target, expression.clone()])
    }
    pub(super) fn assign_hessian(
        &self,
        first_name: &Value,
        second_name: Option<&Value>,
        expression: &Value,
    ) -> SexpResult<Value> {
        let first = self.string(first_name)?;
        let second = self.string(second_name.unwrap_or(first_name))?;
        let target = self.call(
            "[",
            &[
                self.symbol(".hessian")?,
                self.access.domain().missing(),
                first.clone(),
                second.clone(),
            ],
        )?;
        let expression = if second_name.is_some() {
            let reflected = self.call(
                "[",
                &[
                    self.symbol(".hessian")?,
                    self.access.domain().missing(),
                    second,
                    first,
                ],
            )?;
            self.call("<-", &[reflected, expression.clone()])?
        } else {
            expression.clone()
        };
        self.call("<-", &[target, expression])
    }
    pub(super) fn formal_symbol(&self, character: &Value) -> SexpResult<Value> {
        self.access.domain().link(character)?;
        let failure = || SexpError::AllocationFailed {
            object: "derivative formal name",
        };
        let length = usize::try_from(character.try_char_len()?).map_err(|_| failure())?;
        let capacity = length.checked_add(1).ok_or_else(failure)?;
        // The exact byte buffer and Rf_install's temporary UTF-8 name copy
        // coexist. No growing String/CString conversion precedes admission.
        let peak = length.checked_add(capacity).ok_or_else(failure)?;
        let _workspace = self
            .access
            .with_arena(|a| a.reserve_workspace(peak))?
            .ok_or_else(failure)?;
        let mut name = Vec::new();
        name.try_reserve_exact(capacity).map_err(|_| failure())?;
        {
            let header = character.header();
            let lease = header.payload_lease().ok_or_else(failure)?;
            for index in 0..length {
                let byte = lease.byte_elt(index).ok_or_else(failure)?;
                if byte == 0 {
                    break;
                }
                name.push(byte);
            }
        }
        // Match string_at's C-string truncation and invalid-UTF8 fallback.
        if std::str::from_utf8(&name).is_err() {
            name.clear();
        }
        name.push(0);
        super::install_builder_formal(self.access, &name)
    }
    pub(super) fn closure(&self, names: &Value, body: &Value) -> SexpResult<Value> {
        let domain = self.access.domain();
        domain.link(names)?;
        let mut formals = domain.nil();
        let allocator = self.access.allocator(&domain)?;
        for index in (0..names.len()).rev() {
            let character = names.try_string_elt(index)?.into_owned()?;
            self.access.require_active()?;
            let tag = self.formal_symbol(&character)?;
            formals = allocator.pairlist_cell(&domain.missing(), &formals, &tag)?;
        }
        let environment = super::builder_environment(self.access)?;
        self.closure_with_parts(&formals, body, &environment)
    }
    pub(super) fn closure_with_parts(
        &self,
        formals: &Value,
        body: &Value,
        environment: &Value,
    ) -> SexpResult<Value> {
        let domain = self.access.domain();
        let formals_link = domain.link(formals)?;
        let body_link = domain.link(body)?;
        let environment_link = domain.link(environment)?;
        self.access.allocator(&domain)?.allocate(|arena| {
            let raw = arena.alloc_node(SEXPTYPE::CLOSXP);
            let node = arena.node_token(raw)?;
            let heap = arena.heap_identity();
            let mut header = heap.node_snapshot(&node)?;
            header.data.closure_mut().formals = formals_link;
            header.data.closure_mut().body = body_link;
            header.data.closure_mut().env = environment_link;
            heap.replace_node(&node, header)?;
            Some(raw)
        })
    }
    pub(super) fn attach(&self, hessian: bool) -> SexpResult<Value> {
        let name = if hessian { "hessian" } else { "gradient" };
        let target = self.call("attr", &[self.symbol(".value")?, self.text(name)?])?;
        self.call(
            "<-",
            &[
                target,
                self.symbol(if hessian { ".hessian" } else { ".grad" })?,
            ],
        )
    }
}
