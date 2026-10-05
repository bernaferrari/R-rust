#![forbid(unsafe_code)]
//! Checked owning operands for the symbolic derivative table.
use crate::sexp::{
    ffi::SEXPTYPE,
    heap::NodeLink,
    object::{Sexp, SexpError, SexpResult},
    owner::RuntimeAccess,
};
use std::{cell::RefCell, collections::HashSet};

type Value = Sexp<'static>;

fn invalid(message: impl Into<String>) -> SexpError {
    SexpError::EvaluationFailed {
        message: message.into(),
    }
}

struct Call {
    head: Value,
    arguments: Vec<Value>,
    tags: Vec<Value>,
    attributes: Value,
    object: bool,
    gp: u16,
}

fn snapshot(access: &RuntimeAccess, expression: &Value) -> SexpResult<Call> {
    let domain = access.domain();
    domain.link(expression)?;
    let info = expression.header().sxpinfo;
    let head = expression.try_car()?.into_owned()?;
    let attributes = expression.try_attrib()?.into_owned()?;
    let mut arguments = Vec::new();
    let mut tags = Vec::new();
    let mut seen = HashSet::new();
    let mut cell = expression.try_cdr()?.into_owned()?;
    while !cell.is_nil() {
        seen.try_reserve(1)
            .map_err(|_| invalid("cannot inspect derivative arguments"))?;
        if !seen.insert(domain.link(&cell)?) {
            return Err(invalid("cyclic derivative argument list"));
        }
        arguments
            .try_reserve(1)
            .map_err(|_| invalid("cannot retain derivative arguments"))?;
        tags.try_reserve(1)
            .map_err(|_| invalid("cannot retain derivative tags"))?;
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
    })
}

fn name(value: &Value) -> SexpResult<String> {
    value.try_printname()?.try_as_string()
}

struct Path<'a> {
    active: &'a RefCell<HashSet<NodeLink>>,
    link: NodeLink,
}
impl Drop for Path<'_> {
    fn drop(&mut self) {
        self.active.borrow_mut().remove(&self.link);
    }
}

pub(super) struct Kernel<'a, F> {
    access: &'a RuntimeAccess,
    install: F,
    active: RefCell<HashSet<NodeLink>>,
}

impl<'a, F> Kernel<'a, F>
where
    F: Fn(&RuntimeAccess, &str) -> SexpResult<Value>,
{
    pub(super) fn new(access: &'a RuntimeAccess, install: F) -> Self {
        Self {
            access,
            install,
            active: RefCell::new(HashSet::new()),
        }
    }

    fn enter(&self, value: &Value) -> SexpResult<Path<'_>> {
        let link = self.access.domain().link(value)?;
        let mut active = self.active.borrow_mut();
        active
            .try_reserve(1)
            .map_err(|_| invalid("cannot inspect derivative expression"))?;
        if !active.insert(link) {
            return Err(invalid("cyclic derivative expression"));
        }
        drop(active);
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
                    "(" | "+" if call.arguments.len() == 1 => self.derive(&a, variable),
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
                        let exponent = self
                            .numeric(&b)?
                            .ok_or_else(|| invalid("Function is not in the derivatives table"))?;
                        let first = self.derive(&a, variable)?;
                        let power = self.simplify("^", a, Some(self.scalar(exponent - 1.)?))?;
                        let product = self.simplify("*", first, Some(power))?;
                        self.simplify("*", b, Some(product))
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
                            None => self.integer(1)?,
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
                    _ => Err(invalid("Function is not in the derivatives table")),
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
