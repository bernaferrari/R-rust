#![forbid(unsafe_code)]
//! Checked owning operands for the symbolic derivative table.
use crate::sexp::{
    ffi::SEXPTYPE,
    heap::NodeLink,
    memory::WorkspaceReservation,
    object::{Sexp, SexpError, SexpResult},
    owner::RuntimeAccess,
};
use std::{cell::RefCell, mem::size_of};

type Value = Sexp<'static>;

fn invalid(message: impl Into<String>) -> SexpError {
    SexpError::EvaluationFailed {
        message: message.into(),
    }
}

// GNU prints the deparsed callable in derivative-table diagnostics.  Symbols
// that are not R identifiers need backticks and R's character escapes.
fn derivative_function_name(name: &str) -> String {
    let valid = if name.is_ascii() {
        crate::mainutils::deparse::is_valid_r_name_bytes(name.as_bytes())
    } else {
        let mut chars = name.chars();
        match chars.next() {
            Some(first) if first.is_alphabetic() || first == '.' => {
                let rest = chars.as_str();
                !(first == '.' && rest.chars().next().is_some_and(|c| c.is_ascii_digit()))
                    && rest
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '.' || c == '_')
            }
            _ => false,
        }
    };
    if valid {
        return name.to_owned();
    }
    let mut result = String::from("`");
    for c in name.chars() {
        match c {
            '`' => result.push_str("\\`"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '\u{0007}' => result.push_str("\\a"),
            '\u{0008}' => result.push_str("\\b"),
            '\u{000b}' => result.push_str("\\v"),
            '\u{000c}' => result.push_str("\\f"),
            c if c.is_ascii_control() => {
                use std::fmt::Write;
                let _ = write!(result, "\\{:03o}", c as u32);
            }
            c => result.push(c),
        }
    }
    result.push('`');
    result
}

struct Call {
    head: Value,
    arguments: Vec<Value>,
    tags: Vec<Value>,
    attributes: Value,
    object: bool,
    gp: u16,
    // Declared after the buffers so the buffers are freed before their charge.
    _workspace: WorkspaceReservation,
}

fn workspace(access: &RuntimeAccess, bytes: usize, what: &str) -> SexpResult<WorkspaceReservation> {
    access
        .with_arena(|arena| arena.reserve_workspace(bytes))?
        .ok_or_else(|| invalid(format!("cannot retain {what}")))
}

fn snapshot(access: &RuntimeAccess, expression: &Value) -> SexpResult<Call> {
    let domain = access.domain();
    domain.link(expression)?;
    let info = expression.header().sxpinfo;
    let head = expression.try_car()?.into_owned()?;
    let attributes = expression.try_attrib()?.into_owned()?;

    // Count and cycle-check before allocating native buffers. Floyd's walk
    // uses only rooted SEXP handles, so no unadmitted identity table is needed.
    let mut count = 0usize;
    let mut cell = expression.try_cdr()?.into_owned()?;
    let mut slow = cell.clone();
    let mut fast = cell.clone();
    while !cell.is_nil() {
        let _ = cell.try_car()?;
        count = count
            .checked_add(1)
            .ok_or_else(|| invalid("cannot retain derivative arguments"))?;
        cell = cell.try_cdr()?.into_owned()?;

        if !slow.is_nil() {
            slow = slow.try_cdr()?.into_owned()?;
        }
        for _ in 0..2 {
            if !fast.is_nil() {
                fast = fast.try_cdr()?.into_owned()?;
            }
        }
        if !slow.is_nil() && !fast.is_nil() && domain.link(&slow)? == domain.link(&fast)? {
            return Err(invalid("cyclic derivative argument list"));
        }
    }

    let slots = count
        .checked_mul(2)
        .and_then(|n| n.checked_mul(size_of::<Value>()))
        .ok_or_else(|| invalid("cannot retain derivative arguments"))?;
    let _workspace = workspace(access, slots, "derivative arguments")?;
    let mut arguments = Vec::new();
    let mut tags = Vec::new();
    arguments
        .try_reserve_exact(count)
        .map_err(|_| invalid("cannot retain derivative arguments"))?;
    tags.try_reserve_exact(count)
        .map_err(|_| invalid("cannot retain derivative tags"))?;
    let mut cell = expression.try_cdr()?.into_owned()?;
    while !cell.is_nil() {
        arguments.push(cell.try_car()?.into_owned()?);
        tags.push(cell.try_tag()?.into_owned()?);
        cell = cell.try_cdr()?.into_owned()?;
    }
    Ok(Call {
        head,
        arguments,
        tags,
        attributes,
        object: info.obj(),
        gp: info.gp(),
        _workspace,
    })
}

fn name(value: &Value) -> SexpResult<String> {
    value.try_printname()?.try_as_string()
}

struct Path<'a> {
    active: &'a RefCell<Vec<NodeLink>>,
    link: NodeLink,
}
impl Drop for Path<'_> {
    fn drop(&mut self) {
        let removed = self.active.borrow_mut().pop();
        debug_assert_eq!(removed.as_ref(), Some(&self.link));
    }
}

// A run of transparent unary calls shares the kernel's admitted path buffer.
// Truncation unwinds the whole run on success, error, or panic.
struct PathRange<'a> {
    active: &'a RefCell<Vec<NodeLink>>,
    start: usize,
}
impl Drop for PathRange<'_> {
    fn drop(&mut self) {
        self.active.borrow_mut().truncate(self.start);
    }
}

pub(super) struct Kernel<'a, F> {
    access: &'a RuntimeAccess,
    install: F,
    active: RefCell<Vec<NodeLink>>,
    active_workspace: RefCell<Option<WorkspaceReservation>>,
}

impl<'a, F> Kernel<'a, F>
where
    F: Fn(&RuntimeAccess, &str) -> SexpResult<Value>,
{
    pub(super) fn new(access: &'a RuntimeAccess, install: F) -> Self {
        Self {
            access,
            install,
            active: RefCell::new(Vec::new()),
            active_workspace: RefCell::new(None),
        }
    }

    fn push_active(&self, value: &Value) -> SexpResult<NodeLink> {
        let link = self.access.domain().link(value)?;
        let mut active = self.active.borrow_mut();
        if active.contains(&link) {
            return Err(invalid("cyclic derivative expression"));
        }
        if active.len() == active.capacity() {
            let next = active
                .len()
                .checked_add(1)
                .and_then(|n| n.checked_mul(size_of::<NodeLink>()))
                .ok_or_else(|| invalid("cannot inspect derivative expression"))?;
            // The old ticket remains live while Vec reallocates, admitting the
            // old+new peak before the allocator can move its backing storage.
            let ticket = workspace(self.access, next, "derivative expression path")?;
            active
                .try_reserve_exact(1)
                .map_err(|_| invalid("cannot inspect derivative expression"))?;
            *self.active_workspace.borrow_mut() = Some(ticket);
        }
        active.push(link.clone());
        Ok(link)
    }

    fn enter(&self, value: &Value) -> SexpResult<Path<'_>> {
        let link = self.push_active(value)?;
        Ok(Path {
            active: &self.active,
            link,
        })
    }

    pub(super) fn scalar(&self, value: f64) -> SexpResult<Value> {
        let domain = self.access.domain();
        self.access.allocator(&domain)?.allocate(|arena| {
            let pointer = arena.alloc_vector(SEXPTYPE::REALSXP, 1);
            let node = arena.node_token(pointer)?;
            arena
                .heap_identity()
                .payload_lease(&node)?
                .set_real_elt(0, value)?;
            Some(pointer)
        })
    }

    fn integer(&self, value: i32) -> SexpResult<Value> {
        let domain = self.access.domain();
        self.access.allocator(&domain)?.allocate(|arena| {
            let pointer = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
            let node = arena.node_token(pointer)?;
            arena
                .heap_identity()
                .payload_lease(&node)?
                .set_integer_elt(0, value)?;
            Some(pointer)
        })
    }

    fn build(&self, head: &Value, values: &[Value], tags: Option<&[Value]>) -> SexpResult<Value> {
        let domain = self.access.domain();
        let allocator = self.access.allocator(&domain)?;
        let nil = domain.nil();
        let mut tail = nil.clone();
        for (index, value) in values.iter().enumerate().rev() {
            tail = allocator.pairlist_cell(value, &tail, tags.map_or(&nil, |tags| &tags[index]))?;
        }
        allocator.call(head, &tail)
    }

    fn call(&self, function: &str, values: &[Value]) -> SexpResult<Value> {
        let head = (self.install)(self.access, function)?;
        self.access.require_active()?;
        self.build(&head, values, None)
    }

    fn symbol(&self, name: &str) -> SexpResult<Value> {
        (self.install)(self.access, name)
    }

    fn numeric(&self, value: &Value) -> SexpResult<Option<f64>> {
        if value.is_empty()
            || !matches!(
                value.typeof_(),
                SEXPTYPE::INTSXP | SEXPTYPE::REALSXP | SEXPTYPE::LGLSXP
            )
        {
            return Ok(None);
        }
        let number = match value.typeof_() {
            SEXPTYPE::REALSXP => value.try_real_elt(0)?,
            SEXPTYPE::INTSXP => match value.try_integer_elt(0)? {
                crate::sexp::ffi::NA_INTEGER => crate::sexp::ffi::NA_REAL,
                integer => f64::from(integer),
            },
            SEXPTYPE::LGLSXP => match value.try_logical_elt(0)? {
                crate::sexp::ffi::NA_LOGICAL => crate::sexp::ffi::NA_REAL,
                integer => f64::from(integer),
            },
            _ => unreachable!(),
        };
        self.access.require_active()?;
        Ok(Some(number))
    }

    fn unary_minus(&self, value: &Value) -> SexpResult<Option<Value>> {
        if value.typeof_() != SEXPTYPE::LANGSXP {
            return Ok(None);
        }
        let call = snapshot(self.access, value)?;
        if call.head.typeof_() != SEXPTYPE::SYMSXP || name(&call.head)? != "-" {
            return Ok(None);
        }
        if call.arguments.len() == 1
            || (call.arguments.len() == 2
                && call.arguments[1].as_raw() == self.access.domain().missing().as_raw())
        {
            Ok(Some(call.arguments[0].clone()))
        } else {
            Ok(None)
        }
    }

    fn simplify(&self, function: &str, first: Value, second: Option<Value>) -> SexpResult<Value> {
        match function {
            "+" => {
                let second = second.ok_or_else(|| invalid("invalid binary derivative"))?;
                if self.numeric(&first)? == Some(0.0) {
                    return Ok(second);
                }
                if self.numeric(&second)? == Some(0.0) {
                    return Ok(first);
                }
                if let Some(positive) = self.unary_minus(&first)? {
                    return self.simplify("-", second, Some(positive));
                }
                if let Some(positive) = self.unary_minus(&second)? {
                    return self.simplify("-", first, Some(positive));
                }
                self.call("+", &[first, second])
            }
            "-" => match second {
                None => {
                    if self.numeric(&first)? == Some(0.0) {
                        self.scalar(0.)
                    } else if let Some(positive) = self.unary_minus(&first)? {
                        Ok(positive)
                    } else {
                        self.call("-", &[first])
                    }
                }
                Some(second) => {
                    if self.numeric(&second)? == Some(0.0) {
                        return Ok(first);
                    }
                    if self.numeric(&first)? == Some(0.0) {
                        return self.simplify("-", second, None);
                    }
                    if let Some(positive) = self.unary_minus(&first)? {
                        return self.simplify(
                            "-",
                            self.simplify("+", positive, Some(second))?,
                            None,
                        );
                    }
                    if let Some(positive) = self.unary_minus(&second)? {
                        return self.simplify("+", first, Some(positive));
                    }
                    self.call("-", &[first, second])
                }
            },
            "*" => {
                let second = second.ok_or_else(|| invalid("invalid binary derivative"))?;
                if self.numeric(&first)? == Some(0.0) || self.numeric(&second)? == Some(0.0) {
                    return self.scalar(0.);
                }
                if self.numeric(&first)? == Some(1.0) {
                    return Ok(second);
                }
                if self.numeric(&second)? == Some(1.0) {
                    return Ok(first);
                }
                if let Some(positive) = self.unary_minus(&first)? {
                    return self.simplify("-", self.simplify("*", positive, Some(second))?, None);
                }
                if let Some(positive) = self.unary_minus(&second)? {
                    return self.simplify("-", self.simplify("*", first, Some(positive))?, None);
                }
                self.call("*", &[first, second])
            }
            "/" => {
                let second = second.ok_or_else(|| invalid("invalid binary derivative"))?;
                if self.numeric(&first)? == Some(0.0) {
                    return self.scalar(0.);
                }
                if self.numeric(&second)? == Some(0.0) {
                    return self.scalar(crate::sexp::ffi::NA_REAL);
                }
                if self.numeric(&second)? == Some(1.0) {
                    return Ok(first);
                }
                if let Some(positive) = self.unary_minus(&first)? {
                    return self.simplify("-", self.simplify("/", positive, Some(second))?, None);
                }
                if let Some(positive) = self.unary_minus(&second)? {
                    return self.simplify("-", self.simplify("/", first, Some(positive))?, None);
                }
                self.call("/", &[first, second])
            }
            "^" => {
                let second = second.ok_or_else(|| invalid("invalid binary derivative"))?;
                if self.numeric(&second)? == Some(0.0) || self.numeric(&first)? == Some(1.0) {
                    return self.scalar(1.);
                }
                if self.numeric(&first)? == Some(0.0) {
                    return self.scalar(0.);
                }
                if self.numeric(&second)? == Some(1.0) {
                    return Ok(first);
                }
                self.call("^", &[first, second])
            }
            _ => {
                if let Some(second) = second {
                    self.call(function, &[first, second])
                } else {
                    self.call(function, &[first])
                }
            }
        }
    }

    fn derive_transparent(&self, expression: &Value, variable: &Value) -> SexpResult<Value> {
        let _path = PathRange {
            active: &self.active,
            start: self.active.borrow().len(),
        };
        let mut current = expression.clone();
        while current.typeof_() == SEXPTYPE::LANGSXP {
            let call = snapshot(self.access, &current)?;
            let function = name(&call.head)?;
            if !matches!(function.as_str(), "(" | "+") || call.arguments.len() != 1 {
                break;
            }
            self.push_active(&current)?;
            current = call.arguments[0].clone();
        }
        self.derive(&current, variable)
    }

    pub(super) fn derive(&self, expression: &Value, variable: &Value) -> SexpResult<Value> {
        self.access.require_active()?;
        self.access.domain().link(expression)?;
        self.access.domain().link(variable)?;
        match expression.typeof_() {
            SEXPTYPE::INTSXP | SEXPTYPE::REALSXP | SEXPTYPE::LGLSXP | SEXPTYPE::CPLXSXP => {
                self.scalar(0.)
            }
            SEXPTYPE::SYMSXP => self.scalar(if expression.as_raw() == variable.as_raw() {
                1.
            } else {
                0.
            }),
            SEXPTYPE::LANGSXP => {
                let _path = self.enter(expression)?;
                let call = snapshot(self.access, expression)?;
                let function = name(&call.head)?;
                let a = call
                    .arguments
                    .first()
                    .ok_or_else(|| invalid("invalid derivative expression"))?
                    .clone();
                let b = call.arguments.get(1).cloned();
                let binary = || {
                    b.clone()
                        .ok_or_else(|| invalid("invalid binary derivative expression"))
                };
                match function.as_str() {
                    "(" | "+" if call.arguments.len() == 1 => self.derive_transparent(&a, variable),
                    "+" | "-" => {
                        let first = self.derive(&a, variable)?;
                        let second = match b {
                            Some(value) => Some(self.derive(&value, variable)?),
                            None => None,
                        };
                        self.simplify(&function, first, second)
                    }
                    "*" => {
                        let b = binary()?;
                        let first =
                            self.simplify("*", self.derive(&a, variable)?, Some(b.clone()))?;
                        let second = self.simplify("*", a, Some(self.derive(&b, variable)?))?;
                        self.simplify("+", first, Some(second))
                    }
                    "/" => {
                        let b = binary()?;
                        let first =
                            self.simplify("/", self.derive(&a, variable)?, Some(b.clone()))?;
                        let numerator = self.simplify("*", a, Some(self.derive(&b, variable)?))?;
                        let denominator = self.simplify("^", b, Some(self.scalar(2.)?))?;
                        let second = self.simplify("/", numerator, Some(denominator))?;
                        self.simplify("-", first, Some(second))
                    }
                    "^" => {
                        let b = binary()?;
                        if matches!(
                            b.typeof_(),
                            SEXPTYPE::INTSXP | SEXPTYPE::REALSXP | SEXPTYPE::LGLSXP
                        ) {
                            let exponent = self.numeric(&b)?.unwrap_or(crate::sexp::ffi::NA_REAL);
                            let lowered = if crate::sexp::ffi::is_na_real(exponent) {
                                crate::sexp::ffi::NA_REAL
                            } else {
                                exponent - 1.
                            };
                            let first = self.derive(&a, variable)?;
                            let power = self.simplify("^", a, Some(self.scalar(lowered)?))?;
                            let product = self.simplify("*", first, Some(power))?;
                            self.simplify("*", b, Some(product))
                        } else {
                            // Preserve GNU's construction order and simplification rules:
                            // a^(b-1) * (b * D(a)) + a^b * (log(a) * D(b)).
                            let exponent = self.simplify("-", b.clone(), Some(self.scalar(1.)?))?;
                            let power = self.simplify("^", a.clone(), Some(exponent))?;
                            let factor =
                                self.simplify("*", b.clone(), Some(self.derive(&a, variable)?))?;
                            let first = self.simplify("*", power, Some(factor))?;
                            let power = self.simplify("^", a.clone(), Some(b.clone()))?;
                            let log = self.call("log", &[a])?;
                            let factor =
                                self.simplify("*", log, Some(self.derive(&b, variable)?))?;
                            let second = self.simplify("*", power, Some(factor))?;
                            self.simplify("+", first, Some(second))
                        }
                    }
                    "sin" => self.simplify(
                        "*",
                        self.call("cos", std::slice::from_ref(&a))?,
                        Some(self.derive(&a, variable)?),
                    ),
                    "cos" => self.simplify(
                        "*",
                        self.call("sin", std::slice::from_ref(&a))?,
                        Some(self.simplify("-", self.derive(&a, variable)?, None)?),
                    ),
                    "tan" => {
                        let numerator = self.derive(&a, variable)?;
                        let cosine = self.call("cos", &[a])?;
                        let denominator = self.simplify("^", cosine, Some(self.scalar(2.)?))?;
                        self.simplify("/", numerator, Some(denominator))
                    }
                    "exp" => {
                        self.simplify("*", expression.clone(), Some(self.derive(&a, variable)?))
                    }
                    "log" => {
                        if call.arguments.len() != 1 {
                            return Err(invalid(
                                "only single-argument calls to log() are supported;\n  maybe use log(x,a) = log(x)/log(a)",
                            ));
                        }
                        self.simplify("/", self.derive(&a, variable)?, Some(a))
                    }
                    "sqrt" => {
                        let power = self.call("^", &[a, self.scalar(0.5)?])?;
                        self.derive(&power, variable)
                    }
                    "gamma" => {
                        let digamma = self.call("digamma", std::slice::from_ref(&a))?;
                        let product = self.simplify("*", expression.clone(), Some(digamma))?;
                        self.simplify("*", self.derive(&a, variable)?, Some(product))
                    }
                    "lgamma" | "digamma" => {
                        let outer = if function == "lgamma" {
                            "digamma"
                        } else {
                            "trigamma"
                        };
                        self.simplify(
                            "*",
                            self.derive(&a, variable)?,
                            Some(self.call(outer, &[a])?),
                        )
                    }
                    "trigamma" => self.simplify(
                        "*",
                        self.derive(&a, variable)?,
                        Some(self.call("psigamma", &[a, self.integer(2)?])?),
                    ),
                    "psigamma" => {
                        let order = match b {
                            Some(order) => order,
                            // psigamma(x)'s second argument defaults to 0;
                            // d/dx psigamma(x, 0) is psigamma(x, 1L).
                            None => self.integer(0)?,
                        };
                        let next = match self.numeric(&order)? {
                            Some(order) => self.integer(
                                (order as i32)
                                    .checked_add(1)
                                    .ok_or_else(|| invalid("derivative order overflow"))?,
                            )?,
                            None => self.call("+", &[order, self.integer(1)?])?,
                        };
                        self.simplify(
                            "*",
                            self.derive(&a, variable)?,
                            Some(self.call("psigamma", &[a, next])?),
                        )
                    }
                    "cosh" => self.simplify(
                        "*",
                        self.call("sinh", std::slice::from_ref(&a))?,
                        Some(self.derive(&a, variable)?),
                    ),
                    "sinh" => self.simplify(
                        "*",
                        self.call("cosh", std::slice::from_ref(&a))?,
                        Some(self.derive(&a, variable)?),
                    ),
                    "tanh" => {
                        let cosine = self.call("cosh", std::slice::from_ref(&a))?;
                        let denominator = self.simplify("^", cosine, Some(self.scalar(2.)?))?;
                        self.simplify("/", self.derive(&a, variable)?, Some(denominator))
                    }
                    "pnorm" => self.simplify(
                        "*",
                        self.call("dnorm", std::slice::from_ref(&a))?,
                        Some(self.derive(&a, variable)?),
                    ),
                    "dnorm" => {
                        let inner = self.simplify(
                            "*",
                            self.call("dnorm", std::slice::from_ref(&a))?,
                            Some(self.derive(&a, variable)?),
                        )?;
                        let negated = self.simplify("-", a, None)?;
                        self.simplify("*", negated, Some(inner))
                    }
                    "asin" => {
                        let square = self.simplify("^", a.clone(), Some(self.scalar(2.)?))?;
                        let radicand = self.simplify("-", self.scalar(1.)?, Some(square))?;
                        let root = self.call("sqrt", &[radicand])?;
                        self.simplify("/", self.derive(&a, variable)?, Some(root))
                    }
                    "acos" => {
                        let square = self.simplify("^", a.clone(), Some(self.scalar(2.)?))?;
                        let radicand = self.simplify("-", self.scalar(1.)?, Some(square))?;
                        let root = self.call("sqrt", &[radicand])?;
                        let quotient =
                            self.simplify("/", self.derive(&a, variable)?, Some(root))?;
                        self.simplify("-", quotient, None)
                    }
                    "atan" => {
                        let square = self.simplify("^", a.clone(), Some(self.scalar(2.)?))?;
                        let denominator = self.simplify("+", self.scalar(1.)?, Some(square))?;
                        self.simplify("/", self.derive(&a, variable)?, Some(denominator))
                    }
                    "expm1" => self.simplify(
                        "*",
                        self.call("exp", std::slice::from_ref(&a))?,
                        Some(self.derive(&a, variable)?),
                    ),
                    "log1p" => {
                        let derived = self.derive(&a, variable)?;
                        let denominator = self.simplify("+", self.scalar(1.)?, Some(a))?;
                        self.simplify("/", derived, Some(denominator))
                    }
                    "log2" | "log10" => {
                        let base = if function == "log2" { 2. } else { 10. };
                        let derived = self.derive(&a, variable)?;
                        let log_base =
                            self.call("log", std::slice::from_ref(&self.scalar(base)?))?;
                        let denominator = self.simplify("*", a, Some(log_base))?;
                        self.simplify("/", derived, Some(denominator))
                    }
                    "cospi" => {
                        let factor = self.simplify(
                            "*",
                            self.symbol("pi")?,
                            Some(self.derive(&a, variable)?),
                        )?;
                        let negated = self.simplify("-", factor, None)?;
                        self.simplify(
                            "*",
                            self.call("sinpi", std::slice::from_ref(&a))?,
                            Some(negated),
                        )
                    }
                    "sinpi" => {
                        let factor = self.simplify(
                            "*",
                            self.symbol("pi")?,
                            Some(self.derive(&a, variable)?),
                        )?;
                        self.simplify(
                            "*",
                            self.call("cospi", std::slice::from_ref(&a))?,
                            Some(factor),
                        )
                    }
                    "tanpi" => {
                        let numerator = self.simplify(
                            "*",
                            self.symbol("pi")?,
                            Some(self.derive(&a, variable)?),
                        )?;
                        let cosine = self.call("cospi", std::slice::from_ref(&a))?;
                        let denominator = self.simplify("^", cosine, Some(self.scalar(2.)?))?;
                        self.simplify("/", numerator, Some(denominator))
                    }
                    "lfactorial" => {
                        let shifted = self.simplify("+", a.clone(), Some(self.integer(1)?))?;
                        let digamma = self.call("digamma", &[shifted])?;
                        self.simplify("*", self.derive(&a, variable)?, Some(digamma))
                    }
                    "factorial" => {
                        let shifted = self.simplify("+", a.clone(), Some(self.integer(1)?))?;
                        let digamma = self.call("digamma", &[shifted])?;
                        let product = self.simplify("*", expression.clone(), Some(digamma))?;
                        self.simplify("*", self.derive(&a, variable)?, Some(product))
                    }
                    _ => Err(invalid(format!(
                        "Function '{}' is not in the derivatives table",
                        derivative_function_name(&function)
                    ))),
                }
            }
            _ => Err(invalid("invalid derivative expression")),
        }
    }

    fn form(&self, expression: &Value, operators: &[&str]) -> SexpResult<bool> {
        if expression.typeof_() != SEXPTYPE::LANGSXP {
            return Ok(false);
        }
        let call = snapshot(self.access, expression)?;
        Ok(call.arguments.len() == 2 && operators.contains(&name(&call.head)?.as_str()))
    }

    pub(super) fn parentheses(&self, expression: &Value) -> SexpResult<Value> {
        if expression.typeof_() != SEXPTYPE::LANGSXP {
            return Ok(expression.clone());
        }
        let _path = self.enter(expression)?;
        let call = snapshot(self.access, expression)?;
        let function = name(&call.head)?;
        let bytes = call
            .arguments
            .len()
            .checked_mul(size_of::<Value>())
            .ok_or_else(|| invalid("cannot retain derivative operands"))?;
        let _workspace = workspace(self.access, bytes, "derivative operands")?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(call.arguments.len())
            .map_err(|_| invalid("cannot retain derivative operands"))?;
        for value in call.arguments {
            values.push(self.parentheses(&value)?);
        }
        if values.len() == 1 && function == "-" && self.form(&values[0], &["+", "-", "*", "/"])? {
            values[0] = self.call("(", std::slice::from_ref(&values[0]))?;
        }
        if values.len() == 2 {
            let wrap_first = match function.as_str() {
                "*" | "/" => self.form(&values[0], &["+", "-"])?,
                "^" => self.form(&values[0], &["^"])?,
                _ => false,
            };
            let wrap_second = match function.as_str() {
                "+" => self.form(&values[1], &["+"])?,
                "-" => self.form(&values[1], &["+", "-"])?,
                "*" | "/" | "^" => self.form(&values[1], &["+", "-", "*", "/"])?,
                _ => false,
            };
            if wrap_first {
                values[0] = self.call("(", std::slice::from_ref(&values[0]))?;
            }
            if wrap_second {
                values[1] = self.call("(", std::slice::from_ref(&values[1]))?;
            }
        }
        let result = self.build(&call.head, &values, Some(&call.tags))?;
        let mut result = crate::sexp::object::SexpMut::try_from_checked(result)?;
        result.try_set_attribute(&call.attributes)?;
        self.access.require_active()?;
        let result = result.freeze();
        let node = result.allocation()?.clone();
        let heap = node.heap_identity();
        let mut header = heap
            .node_snapshot(&node)
            .ok_or(SexpError::RootUnavailable)?;
        header.sxpinfo.set_obj(call.object);
        header.sxpinfo.set_gp(call.gp);
        heap.replace_node(&node, header)
            .ok_or(SexpError::RootUnavailable)?;
        Ok(result)
    }
}

pub(super) fn direct<F, W>(
    access: &RuntimeAccess,
    arguments: &Value,
    install: F,
    warning: W,
) -> SexpResult<Value>
where
    F: Fn(&RuntimeAccess, &str) -> SexpResult<Value>,
    W: FnOnce(&RuntimeAccess) -> SexpResult<()>,
{
    access.domain().link(arguments)?;
    let arguments = arguments.try_cdr()?.into_owned()?;
    let given = arguments.clone().try_pairlist_arg(0)?.into_owned()?;
    let names = arguments.try_pairlist_arg(1)?.into_owned()?;
    let expression = if given.typeof_() == SEXPTYPE::EXPRSXP {
        given.try_vector_elt(0)?.into_owned()?
    } else {
        given
    };
    access.require_active()?;
    if !matches!(
        expression.typeof_(),
        SEXPTYPE::LANGSXP
            | SEXPTYPE::SYMSXP
            | SEXPTYPE::INTSXP
            | SEXPTYPE::REALSXP
            | SEXPTYPE::CPLXSXP
    ) {
        return Err(invalid(format!(
            "expression must not be type '{}'",
            match expression.typeof_() {
                SEXPTYPE::LGLSXP => "logical",
                SEXPTYPE::STRSXP => "character",
                SEXPTYPE::VECSXP => "list",
                SEXPTYPE::NILSXP => "NULL",
                _ => "unsupported",
            }
        )));
    }
    if names.typeof_() != SEXPTYPE::STRSXP || names.is_empty() {
        return Err(invalid("variable must be a character string"));
    }
    let character = names.try_string_elt(0)?.into_owned()?;
    access.require_active()?;
    if names.len() > 1 {
        warning(access)?;
    }
    let variable = install(access, &character.try_as_string()?)?;
    let kernel = Kernel::new(access, install);
    let result = kernel.derive(&expression, &variable)?;
    kernel.parentheses(&result)
}

#[cfg(test)]
mod scratch_admission_tests {
    use super::*;
    use crate::sexp::{RSession, memory::ArenaBudget};

    #[test]
    fn call_snapshot_admits_native_scratch_and_releases_its_ticket() {
        let mut session = RSession::new_for_gc_tests();
        let expression = session
            .eval_code_with_output_capture(&format!(
                "quote(f({}))",
                (0..512)
                    .map(|i| format!("arg{i}=x"))
                    .collect::<Vec<_>>()
                    .join(",")
            ))
            .0
            .unwrap()
            .into_owned()
            .unwrap();
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        let before = crate::sexp::owner::with_runtime(&owner, |access| {
            access.with_arena(|arena| (arena.total_bytes_allocated(), arena.workspace_bytes()))
        })
        .unwrap()
        .unwrap();
        session.set_arena_budget(ArenaBudget::new(before.0, 0));

        let pin = owner.pin().unwrap();
        let denied = crate::sexp::owner::with_runtime(&owner, |access| {
            let call = super::snapshot(access, &expression)?;
            Ok::<_, SexpError>((
                call.arguments.len(),
                call.arguments.capacity(),
                call.tags.capacity(),
            ))
        })
        .unwrap()
        .unwrap_err();
        assert!(
            denied
                .to_string()
                .contains("cannot retain derivative arguments")
        );
        let after_denial = crate::sexp::owner::with_runtime(&owner, |access| {
            access.with_arena(|arena| (arena.total_bytes_allocated(), arena.workspace_bytes()))
        })
        .unwrap()
        .unwrap();
        assert_eq!(
            after_denial, before,
            "denial must not leak a partial ticket"
        );

        session.set_arena_budget(ArenaBudget::unlimited());
        let (call, result, held) = crate::sexp::owner::with_runtime(&owner, |access| {
            let call = super::snapshot(access, &expression)?;
            let result = (
                call.arguments.len(),
                call.arguments.capacity(),
                call.tags.capacity(),
            );
            let held = call._workspace.bytes();
            Ok::<_, SexpError>((call, result, held))
        })
        .unwrap()
        .unwrap();

        assert_eq!(result.0, 512);
        assert_eq!(result.1, 512);
        assert_eq!(result.2, 512);
        assert_eq!(held, (result.1 + result.2) * size_of::<Value>());
        drop(call);
        let after = crate::sexp::owner::with_runtime(&owner, |access| {
            access.with_arena(|arena| (arena.total_bytes_allocated(), arena.workspace_bytes()))
        })
        .unwrap()
        .unwrap();
        assert_eq!(
            after, before,
            "snapshot ticket must release with its buffers"
        );
        drop(pin);
    }

    #[test]
    fn active_path_growth_admits_old_and_new_backings_before_reallocation() {
        let mut session = RSession::new_for_gc_tests();
        let expression = session
            .eval_code_with_output_capture("quote(f(f(x)))")
            .0
            .unwrap()
            .into_owned()
            .unwrap();
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        let pin = owner.pin().unwrap();
        crate::sexp::owner::with_runtime(&owner, |access| {
            let kernel = Kernel::new(access, |_, _| Err(invalid("unused test installer")));
            let child = expression.try_cdr()?.try_car()?.into_owned()?;
            let _outer = kernel.enter(&expression)?;
            assert_eq!(kernel.active.borrow().capacity(), 1);
            let old_bytes = size_of::<NodeLink>();
            assert_eq!(
                kernel.active_workspace.borrow().as_ref().unwrap().bytes(),
                old_bytes
            );

            let (allocated, held) = access
                .with_arena(|arena| (arena.total_bytes_allocated(), arena.workspace_bytes()))?;
            assert_eq!(held, old_bytes);
            access.with_arena(|arena| {
                arena.set_budget(ArenaBudget::new(allocated + held + 2 * old_bytes - 1, 0));
            })?;
            let denied = kernel.enter(&child).err().unwrap();
            assert!(
                denied
                    .to_string()
                    .contains("cannot retain derivative expression path"),
                "unexpected admission error: {denied}"
            );
            assert_eq!(kernel.active.borrow().len(), 1);
            assert_eq!(kernel.active.borrow().capacity(), 1);
            assert_eq!(
                access.with_arena(|arena| arena.workspace_bytes())?,
                old_bytes,
                "failed growth must retain only the old backing's ticket"
            );

            access.with_arena(|arena| arena.set_budget(ArenaBudget::unlimited()))?;
            let _inner = kernel.enter(&child)?;
            assert_eq!(kernel.active.borrow().len(), 2);
            assert_eq!(kernel.active.borrow().capacity(), 2);
            let new_bytes = 2 * size_of::<NodeLink>();
            assert_eq!(
                kernel.active_workspace.borrow().as_ref().unwrap().bytes(),
                new_bytes
            );
            assert_eq!(
                access.with_arena(|arena| arena.workspace_bytes())?,
                new_bytes
            );
            drop(_inner);
            assert_eq!(kernel.active.borrow().len(), 1);
            drop(_outer);
            assert_eq!(kernel.active.borrow().len(), 0);
            drop(kernel);
            assert_eq!(access.with_arena(|arena| arena.workspace_bytes())?, 0);
            Ok::<_, SexpError>(())
        })
        .unwrap()
        .unwrap();
        drop(pin);
    }
}
