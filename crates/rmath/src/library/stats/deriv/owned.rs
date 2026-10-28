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

#[derive(Clone, Copy)]
enum ParenthesesOperator {
    Plus,
    Minus,
    Multiply,
    Divide,
    Power,
    Other,
}

struct ParenthesesFrame {
    call: Call,
    operator: ParenthesesOperator,
    values: Vec<Value>,
    // The values buffer is dropped before its reservation.
    _workspace: WorkspaceReservation,
}

enum DerivativeOperator {
    Transparent,
    Negate,
    Plus,
    Minus,
    Multiply,
    Divide,
    PowerNumeric {
        exponent: Value,
        lowered: f64,
    },
    PowerSymbolicBase {
        first_power: Value,
    },
    PowerSymbolicExponent {
        second_power: Value,
        log_base: Value,
    },
    Sin(Value),
    Cos(Value),
    Tan,
    Cosh(Value),
    Sinh(Value),
    Tanh(Value),
    Asin(Value),
    Acos(Value),
    Atan(Value),
    Exp,
    Log,
    Expm1(Value),
    Log1p,
    LogBase(f64),
    Pnorm(Value),
    Dnorm(Value),
    Sinpi(Value),
    Cospi(Value),
    Tanpi(Value),
    Gamma(Value),
    GammaDerivative(&'static str),
    Trigamma,
    Psigamma(Value),
}

struct DerivativeFrame {
    // Source and snapshot remain owned across callbacks in either child.
    expression: Value,
    call: Call,
    operator: DerivativeOperator,
    first: Option<Value>,
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

// Iterative traversals share the kernel's admitted path buffer.
// Truncation unwinds the whole traversal on success, error, or panic.
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

    pub(super) fn derive(&self, expression: &Value, variable: &Value) -> SexpResult<Value> {
        let _path = PathRange {
            active: &self.active,
            start: self.active.borrow().len(),
        };
        // Keep the old frame allocation charged until replacement succeeds.
        let mut _frame_workspace = None;
        let mut frames: Vec<DerivativeFrame> = Vec::new();
        let mut current = expression.clone();
        loop {
            self.access.require_active()?;
            self.access.domain().link(&current)?;
            self.access.domain().link(variable)?;
            let mut result = match current.typeof_() {
                SEXPTYPE::INTSXP | SEXPTYPE::REALSXP | SEXPTYPE::LGLSXP | SEXPTYPE::CPLXSXP => {
                    self.scalar(0.)?
                }
                SEXPTYPE::SYMSXP => self.scalar(if current.as_raw() == variable.as_raw() {
                    1.
                } else {
                    0.
                })?,
                SEXPTYPE::LANGSXP => {
                    self.push_active(&current)?;
                    let call = snapshot(self.access, &current)?;
                    let function = name(&call.head)?;
                    let first = call
                        .arguments
                        .first()
                        .ok_or_else(|| invalid("invalid derivative expression"))?
                        .clone();
                    // These factors are constructed before D(first), exactly
                    // where the recursive GNU rules suspend. The continuation
                    // owns them across callbacks while the child is traversed.
                    let operator = match (function.as_str(), call.arguments.len()) {
                        ("(" | "+", 1) => Some(DerivativeOperator::Transparent),
                        ("-", 1) => Some(DerivativeOperator::Negate),
                        ("+", _) => Some(DerivativeOperator::Plus),
                        ("-", _) => Some(DerivativeOperator::Minus),
                        ("*", _) => {
                            if call.arguments.get(1).is_none() {
                                return Err(invalid("invalid binary derivative expression"));
                            }
                            Some(DerivativeOperator::Multiply)
                        }
                        ("/", _) => {
                            if call.arguments.get(1).is_none() {
                                return Err(invalid("invalid binary derivative expression"));
                            }
                            Some(DerivativeOperator::Divide)
                        }
                        ("^", _) => {
                            let exponent = call
                                .arguments
                                .get(1)
                                .ok_or_else(|| invalid("invalid binary derivative expression"))?
                                .clone();
                            if matches!(
                                exponent.typeof_(),
                                SEXPTYPE::INTSXP | SEXPTYPE::REALSXP | SEXPTYPE::LGLSXP
                            ) {
                                let exponent_value = self
                                    .numeric(&exponent)?
                                    .unwrap_or(crate::sexp::ffi::NA_REAL);
                                let lowered = if crate::sexp::ffi::is_na_real(exponent_value) {
                                    crate::sexp::ffi::NA_REAL
                                } else {
                                    exponent_value - 1.
                                };
                                Some(DerivativeOperator::PowerNumeric { exponent, lowered })
                            } else {
                                // GNU constructs a^(b-1) before traversing D(a).
                                let lowered =
                                    self.simplify("-", exponent.clone(), Some(self.scalar(1.)?))?;
                                let first_power =
                                    self.simplify("^", first.clone(), Some(lowered))?;
                                Some(DerivativeOperator::PowerSymbolicBase { first_power })
                            }
                        }
                        ("sin", _) => Some(DerivativeOperator::Sin(
                            self.call("cos", std::slice::from_ref(&first))?,
                        )),
                        ("cos", _) => Some(DerivativeOperator::Cos(
                            self.call("sin", std::slice::from_ref(&first))?,
                        )),
                        ("tan", _) => Some(DerivativeOperator::Tan),
                        ("cosh", _) => Some(DerivativeOperator::Cosh(
                            self.call("sinh", std::slice::from_ref(&first))?,
                        )),
                        ("sinh", _) => Some(DerivativeOperator::Sinh(
                            self.call("cosh", std::slice::from_ref(&first))?,
                        )),
                        ("tanh", _) => {
                            let cosine = self.call("cosh", std::slice::from_ref(&first))?;
                            let denominator = self.simplify("^", cosine, Some(self.scalar(2.)?))?;
                            Some(DerivativeOperator::Tanh(denominator))
                        }
                        ("asin", _) | ("acos", _) => {
                            let square =
                                self.simplify("^", first.clone(), Some(self.scalar(2.)?))?;
                            let radicand = self.simplify("-", self.scalar(1.)?, Some(square))?;
                            let root = self.call("sqrt", &[radicand])?;
                            Some(if function == "asin" {
                                DerivativeOperator::Asin(root)
                            } else {
                                DerivativeOperator::Acos(root)
                            })
                        }
                        ("atan", _) => {
                            let square =
                                self.simplify("^", first.clone(), Some(self.scalar(2.)?))?;
                            let denominator = self.simplify("+", self.scalar(1.)?, Some(square))?;
                            Some(DerivativeOperator::Atan(denominator))
                        }
                        ("gamma", _) => {
                            let digamma = self.call("digamma", std::slice::from_ref(&first))?;
                            let product = self.simplify("*", current.clone(), Some(digamma))?;
                            Some(DerivativeOperator::Gamma(product))
                        }
                        ("lgamma", _) => Some(DerivativeOperator::GammaDerivative("digamma")),
                        ("digamma", _) => Some(DerivativeOperator::GammaDerivative("trigamma")),
                        ("trigamma", _) => Some(DerivativeOperator::Trigamma),
                        ("psigamma", _) => {
                            let order = match call.arguments.get(1) {
                                Some(order) => order.clone(),
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
                            Some(DerivativeOperator::Psigamma(next))
                        }
                        ("sinpi", _) => Some(DerivativeOperator::Sinpi(self.symbol("pi")?)),
                        ("cospi", _) => Some(DerivativeOperator::Cospi(self.symbol("pi")?)),
                        ("tanpi", _) => Some(DerivativeOperator::Tanpi(self.symbol("pi")?)),
                        ("expm1", _) => Some(DerivativeOperator::Expm1(
                            self.call("exp", std::slice::from_ref(&first))?,
                        )),
                        ("log1p", _) => Some(DerivativeOperator::Log1p),
                        ("log2", _) => Some(DerivativeOperator::LogBase(2.)),
                        ("log10", _) => Some(DerivativeOperator::LogBase(10.)),
                        ("pnorm", _) | ("dnorm", _) => {
                            let density = self.call("dnorm", std::slice::from_ref(&first))?;
                            Some(if function == "pnorm" {
                                DerivativeOperator::Pnorm(density)
                            } else {
                                DerivativeOperator::Dnorm(density)
                            })
                        }
                        ("exp", _) => Some(DerivativeOperator::Exp),
                        ("log", 1) => Some(DerivativeOperator::Log),
                        ("log", _) => {
                            return Err(invalid(
                                "only single-argument calls to log() are supported;\n  maybe use log(x,a) = log(x)/log(a)",
                            ));
                        }
                        _ => None,
                    };
                    if let Some(operator) = operator {
                        if frames.len() == frames.capacity() {
                            let capacity = frames
                                .len()
                                .checked_add(1)
                                .ok_or_else(|| invalid("cannot retain derivative frames"))?;
                            let bytes = capacity
                                .checked_mul(size_of::<DerivativeFrame>())
                                .ok_or_else(|| invalid("cannot retain derivative frames"))?;
                            let mut next = workspace(self.access, bytes, "derivative frames")?;
                            let mut replacement = Vec::new();
                            replacement
                                .try_reserve_exact(capacity)
                                .map_err(|_| invalid("cannot retain derivative frames"))?;
                            let actual = replacement
                                .capacity()
                                .checked_mul(size_of::<DerivativeFrame>())
                                .ok_or_else(|| invalid("cannot retain derivative frames"))?;
                            next.try_resize(actual)
                                .ok_or_else(|| invalid("cannot retain derivative frames"))?;
                            replacement.append(&mut frames);
                            drop(std::mem::replace(&mut frames, replacement));
                            _frame_workspace = Some(next);
                        }
                        frames.push(DerivativeFrame {
                            expression: current,
                            call,
                            operator,
                            first: None,
                        });
                        current = first;
                        continue;
                    }
                    let result = self.derive_call(&current, variable, call, function)?;
                    self.active.borrow_mut().pop();
                    result
                }
                _ => return Err(invalid("invalid derivative expression")),
            };
            loop {
                let Some(frame) = frames.last_mut() else {
                    return Ok(result);
                };
                if let DerivativeOperator::PowerSymbolicBase { first_power } = &frame.operator {
                    let first_power = first_power.clone();
                    let base = frame.call.arguments[0].clone();
                    let exponent = frame.call.arguments[1].clone();
                    // Match the recursive GNU sequence: finish the base term,
                    // then construct a^b and log(a), before visiting D(b).
                    let factor = self.simplify("*", exponent.clone(), Some(result))?;
                    let first_term = self.simplify("*", first_power, Some(factor))?;
                    let second_power = self.simplify("^", base.clone(), Some(exponent.clone()))?;
                    let log_base = self.call("log", &[base])?;
                    frame.operator = DerivativeOperator::PowerSymbolicExponent {
                        second_power,
                        log_base,
                    };
                    frame.first = Some(first_term);
                    current = exponent;
                    break;
                }
                if matches!(
                    &frame.operator,
                    DerivativeOperator::Plus
                        | DerivativeOperator::Minus
                        | DerivativeOperator::Multiply
                        | DerivativeOperator::Divide
                ) && frame.first.is_none()
                {
                    if let Some(second) = frame.call.arguments.get(1) {
                        // Suspend only after D(first) succeeds. Each occurrence
                        // is traversed independently and the left result stays rooted.
                        current = second.clone();
                        frame.first = Some(match &frame.operator {
                            DerivativeOperator::Multiply => {
                                self.simplify("*", result, Some(frame.call.arguments[1].clone()))?
                            }
                            DerivativeOperator::Divide => {
                                self.simplify("/", result, Some(frame.call.arguments[1].clone()))?
                            }
                            _ => result,
                        });
                        break;
                    }
                }
                let frame = frames.pop().expect("frame just observed");
                result = match frame.operator {
                    DerivativeOperator::Transparent => result,
                    DerivativeOperator::Negate => self.simplify("-", result, None)?,
                    DerivativeOperator::Sin(factor) => self.simplify("*", factor, Some(result))?,
                    DerivativeOperator::Cos(factor) => {
                        let negative = self.simplify("-", result, None)?;
                        self.simplify("*", factor, Some(negative))?
                    }
                    DerivativeOperator::Tan => {
                        let cosine =
                            self.call("cos", std::slice::from_ref(&frame.call.arguments[0]))?;
                        let denominator = self.simplify("^", cosine, Some(self.scalar(2.)?))?;
                        self.simplify("/", result, Some(denominator))?
                    }
                    DerivativeOperator::Cosh(factor) | DerivativeOperator::Sinh(factor) => {
                        self.simplify("*", factor, Some(result))?
                    }
                    DerivativeOperator::Tanh(denominator) => {
                        self.simplify("/", result, Some(denominator))?
                    }
                    DerivativeOperator::Asin(root) | DerivativeOperator::Atan(root) => {
                        self.simplify("/", result, Some(root))?
                    }
                    DerivativeOperator::Acos(root) => {
                        let quotient = self.simplify("/", result, Some(root))?;
                        self.simplify("-", quotient, None)?
                    }
                    DerivativeOperator::Expm1(factor) | DerivativeOperator::Pnorm(factor) => {
                        self.simplify("*", factor, Some(result))?
                    }
                    DerivativeOperator::Gamma(product) => {
                        self.simplify("*", result, Some(product))?
                    }
                    DerivativeOperator::GammaDerivative(outer) => {
                        let factor =
                            self.call(outer, std::slice::from_ref(&frame.call.arguments[0]))?;
                        self.simplify("*", result, Some(factor))?
                    }
                    DerivativeOperator::Trigamma => {
                        let factor = self.call(
                            "psigamma",
                            &[frame.call.arguments[0].clone(), self.integer(2)?],
                        )?;
                        self.simplify("*", result, Some(factor))?
                    }
                    DerivativeOperator::Psigamma(next) => {
                        let factor =
                            self.call("psigamma", &[frame.call.arguments[0].clone(), next])?;
                        self.simplify("*", result, Some(factor))?
                    }
                    DerivativeOperator::Sinpi(pi) => {
                        let factor = self.simplify("*", pi, Some(result))?;
                        self.simplify(
                            "*",
                            self.call("cospi", std::slice::from_ref(&frame.call.arguments[0]))?,
                            Some(factor),
                        )?
                    }
                    DerivativeOperator::Cospi(pi) => {
                        let factor = self.simplify("*", pi, Some(result))?;
                        let negated = self.simplify("-", factor, None)?;
                        self.simplify(
                            "*",
                            self.call("sinpi", std::slice::from_ref(&frame.call.arguments[0]))?,
                            Some(negated),
                        )?
                    }
                    DerivativeOperator::Tanpi(pi) => {
                        let numerator = self.simplify("*", pi, Some(result))?;
                        let cosine =
                            self.call("cospi", std::slice::from_ref(&frame.call.arguments[0]))?;
                        let denominator = self.simplify("^", cosine, Some(self.scalar(2.)?))?;
                        self.simplify("/", numerator, Some(denominator))?
                    }
                    DerivativeOperator::Dnorm(density) => {
                        let inner = self.simplify("*", density, Some(result))?;
                        let negated = self.simplify("-", frame.call.arguments[0].clone(), None)?;
                        self.simplify("*", negated, Some(inner))?
                    }
                    DerivativeOperator::Log1p => {
                        let denominator = self.simplify(
                            "+",
                            self.scalar(1.)?,
                            Some(frame.call.arguments[0].clone()),
                        )?;
                        self.simplify("/", result, Some(denominator))?
                    }
                    DerivativeOperator::LogBase(base) => {
                        let log_base =
                            self.call("log", std::slice::from_ref(&self.scalar(base)?))?;
                        let denominator =
                            self.simplify("*", frame.call.arguments[0].clone(), Some(log_base))?;
                        self.simplify("/", result, Some(denominator))?
                    }
                    DerivativeOperator::Exp => {
                        self.simplify("*", frame.expression, Some(result))?
                    }
                    DerivativeOperator::Log => {
                        self.simplify("/", result, Some(frame.call.arguments[0].clone()))?
                    }
                    operator @ (DerivativeOperator::Plus | DerivativeOperator::Minus) => {
                        let operator = if matches!(operator, DerivativeOperator::Plus) {
                            "+"
                        } else {
                            "-"
                        };
                        match frame.first {
                            Some(first) => self.simplify(operator, first, Some(result))?,
                            None => self.simplify(operator, result, None)?,
                        }
                    }
                    DerivativeOperator::Multiply => self.simplify(
                        "+",
                        frame.first.expect("product first term just computed"),
                        Some(self.simplify("*", frame.call.arguments[0].clone(), Some(result))?),
                    )?,
                    DerivativeOperator::Divide => {
                        let numerator =
                            self.simplify("*", frame.call.arguments[0].clone(), Some(result))?;
                        let denominator = self.simplify(
                            "^",
                            frame.call.arguments[1].clone(),
                            Some(self.scalar(2.)?),
                        )?;
                        let second = self.simplify("/", numerator, Some(denominator))?;
                        self.simplify(
                            "-",
                            frame.first.expect("quotient first term just computed"),
                            Some(second),
                        )?
                    }
                    DerivativeOperator::PowerNumeric { exponent, lowered } => {
                        let power = self.simplify(
                            "^",
                            frame.call.arguments[0].clone(),
                            Some(self.scalar(lowered)?),
                        )?;
                        let product = self.simplify("*", result, Some(power))?;
                        self.simplify("*", exponent, Some(product))?
                    }
                    DerivativeOperator::PowerSymbolicBase { .. } => {
                        unreachable!("symbolic power base must visit its exponent")
                    }
                    DerivativeOperator::PowerSymbolicExponent {
                        second_power,
                        log_base,
                    } => {
                        let factor = self.simplify("*", log_base, Some(result))?;
                        let second = self.simplify("*", second_power, Some(factor))?;
                        self.simplify(
                            "+",
                            frame
                                .first
                                .expect("symbolic power first term just computed"),
                            Some(second),
                        )?
                    }
                };
                self.active.borrow_mut().pop();
            }
        }
    }

    fn derive_call(
        &self,
        expression: &Value,
        variable: &Value,
        call: Call,
        function: String,
    ) -> SexpResult<Value> {
        let a = call
            .arguments
            .first()
            .ok_or_else(|| invalid("invalid derivative expression"))?
            .clone();
        match function.as_str() {
            "sqrt" => {
                let power = self.call("^", &[a, self.scalar(0.5)?])?;
                self.derive(&power, variable)
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

    fn form(&self, expression: &Value, operators: &[&str]) -> SexpResult<bool> {
        if expression.typeof_() != SEXPTYPE::LANGSXP {
            return Ok(false);
        }
        let call = snapshot(self.access, expression)?;
        Ok(call.arguments.len() == 2 && operators.contains(&name(&call.head)?.as_str()))
    }

    pub(super) fn parentheses(&self, expression: &Value) -> SexpResult<Value> {
        let _path = PathRange {
            active: &self.active,
            start: self.active.borrow().len(),
        };
        // Hold the old frame-buffer reservation until a new allocation has
        // been admitted and installed, including its reallocation peak.
        let mut _frame_workspace: Option<WorkspaceReservation> = None;
        let mut frames: Vec<ParenthesesFrame> = Vec::new();
        let mut current = expression.clone();
        loop {
            let mut result = if current.typeof_() != SEXPTYPE::LANGSXP {
                current
            } else {
                self.push_active(&current)?;
                let call = snapshot(self.access, &current)?;
                let operator = match name(&call.head)?.as_str() {
                    "+" => ParenthesesOperator::Plus,
                    "-" => ParenthesesOperator::Minus,
                    "*" => ParenthesesOperator::Multiply,
                    "/" => ParenthesesOperator::Divide,
                    "^" => ParenthesesOperator::Power,
                    _ => ParenthesesOperator::Other,
                };
                let bytes = call
                    .arguments
                    .len()
                    .checked_mul(size_of::<Value>())
                    .ok_or_else(|| invalid("cannot retain derivative operands"))?;
                let mut values_workspace = workspace(self.access, bytes, "derivative operands")?;
                let mut values = Vec::new();
                values
                    .try_reserve_exact(call.arguments.len())
                    .map_err(|_| invalid("cannot retain derivative operands"))?;
                let actual = values
                    .capacity()
                    .checked_mul(size_of::<Value>())
                    .ok_or_else(|| invalid("cannot retain derivative operands"))?;
                values_workspace
                    .try_resize(actual)
                    .ok_or_else(|| invalid("cannot retain derivative operands"))?;
                if call.arguments.is_empty() {
                    let result = self.finish_parentheses(call, operator, values)?;
                    self.active.borrow_mut().pop();
                    result
                } else {
                    if frames.len() == frames.capacity() {
                        let bytes = frames
                            .len()
                            .checked_add(1)
                            .and_then(|n| n.checked_mul(size_of::<ParenthesesFrame>()))
                            .ok_or_else(|| invalid("cannot retain derivative frames"))?;
                        let mut next = workspace(self.access, bytes, "derivative frames")?;
                        let mut replacement = Vec::new();
                        replacement
                            .try_reserve_exact(frames.len() + 1)
                            .map_err(|_| invalid("cannot retain derivative frames"))?;
                        let actual = replacement
                            .capacity()
                            .checked_mul(size_of::<ParenthesesFrame>())
                            .ok_or_else(|| invalid("cannot retain derivative frames"))?;
                        next.try_resize(actual)
                            .ok_or_else(|| invalid("cannot retain derivative frames"))?;
                        replacement.append(&mut frames);
                        drop(std::mem::replace(&mut frames, replacement));
                        _frame_workspace = Some(next);
                    }
                    current = call.arguments[0].clone();
                    frames.push(ParenthesesFrame {
                        call,
                        operator,
                        values,
                        _workspace: values_workspace,
                    });
                    continue;
                }
            };
            loop {
                let Some(frame) = frames.last_mut() else {
                    return Ok(result);
                };
                frame.values.push(result);
                if frame.values.len() < frame.call.arguments.len() {
                    current = frame.call.arguments[frame.values.len()].clone();
                    break;
                }
                let frame = frames.pop().expect("frame just observed");
                result = self.finish_parentheses(frame.call, frame.operator, frame.values)?;
                self.active.borrow_mut().pop();
            }
        }
    }

    fn finish_parentheses(
        &self,
        call: Call,
        operator: ParenthesesOperator,
        mut values: Vec<Value>,
    ) -> SexpResult<Value> {
        if values.len() == 1
            && matches!(operator, ParenthesesOperator::Minus)
            && self.form(&values[0], &["+", "-", "*", "/"])?
        {
            values[0] = self.call("(", std::slice::from_ref(&values[0]))?;
        }
        if values.len() == 2 {
            let wrap_first = match operator {
                ParenthesesOperator::Multiply | ParenthesesOperator::Divide => {
                    self.form(&values[0], &["+", "-"])?
                }
                ParenthesesOperator::Power => self.form(&values[0], &["^"])?,
                _ => false,
            };
            let wrap_second = match operator {
                ParenthesesOperator::Plus => self.form(&values[1], &["+"])?,
                ParenthesesOperator::Minus => self.form(&values[1], &["+", "-"])?,
                ParenthesesOperator::Multiply
                | ParenthesesOperator::Divide
                | ParenthesesOperator::Power => self.form(&values[1], &["+", "-", "*", "/"])?,
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
    fn parentheses_frame_denial_releases_workspace_and_allows_recovery() {
        let mut session = RSession::new_for_gc_tests();
        let expression = session
            .eval_code_with_output_capture("quote(f(x))")
            .0
            .unwrap()
            .into_owned()
            .unwrap();
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        crate::sexp::owner::with_runtime(&owner, |access| {
            let kernel = Kernel::new(access, |_, _| Err(invalid("unused installer")));
            let (allocated, held) = access
                .with_arena(|arena| (arena.total_bytes_allocated(), arena.workspace_bytes()))?;
            let allowance =
                size_of::<NodeLink>() + 3 * size_of::<Value>() + size_of::<ParenthesesFrame>() - 1;
            access.with_arena(|arena| {
                arena.set_budget(ArenaBudget::new(allocated + held + allowance, 0))
            })?;
            let error = kernel.parentheses(&expression).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("cannot retain derivative frames"),
                "{error}"
            );
            assert!(kernel.active.borrow().is_empty());
            let after = access.with_arena(|arena| arena.workspace_bytes())?;
            assert_eq!(after, held + size_of::<NodeLink>());
            access.with_arena(|arena| arena.set_budget(ArenaBudget::unlimited()))?;
            assert_eq!(
                kernel.parentheses(&expression)?.typeof_(),
                SEXPTYPE::LANGSXP
            );
            Ok::<_, SexpError>(())
        })
        .unwrap()
        .unwrap();
    }

    #[test]
    fn additive_frame_denial_releases_workspace_and_allows_recovery() {
        let mut session = RSession::new_for_gc_tests();
        let expression = session
            .eval_code_with_output_capture("quote(x + y)")
            .0
            .unwrap()
            .into_owned()
            .unwrap();
        let variable = expression
            .try_cdr()
            .unwrap()
            .try_car()
            .unwrap()
            .into_owned()
            .unwrap();
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        crate::sexp::owner::with_runtime(&owner, |access| {
            let kernel = Kernel::new(access, |_, _| Err(invalid("unused installer")));
            let (allocated, held) = access
                .with_arena(|arena| (arena.total_bytes_allocated(), arena.workspace_bytes()))?;
            let allowance =
                size_of::<NodeLink>() + 4 * size_of::<Value>() + size_of::<DerivativeFrame>() - 1;
            access.with_arena(|arena| {
                arena.set_budget(ArenaBudget::new(allocated + held + allowance, 0))
            })?;
            let error = kernel.derive(&expression, &variable).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("cannot retain derivative frames"),
                "{error}"
            );
            assert!(kernel.active.borrow().is_empty());
            assert_eq!(
                access.with_arena(|arena| arena.workspace_bytes())?,
                held + size_of::<NodeLink>()
            );
            access.with_arena(|arena| arena.set_budget(ArenaBudget::unlimited()))?;
            assert_eq!(kernel.derive(&expression, &variable)?.try_real_elt(0)?, 1.0);
            drop(kernel);
            assert_eq!(access.with_arena(|arena| arena.workspace_bytes())?, held);
            Ok::<_, SexpError>(())
        })
        .unwrap()
        .unwrap();
    }

    #[test]
    fn common_unary_frame_denial_releases_workspace_and_allows_recovery() {
        let mut session = RSession::new_for_gc_tests();
        let expression = session
            .eval_code_with_output_capture("quote(exp(y))")
            .0
            .unwrap()
            .into_owned()
            .unwrap();
        let variable = expression.try_car().unwrap().into_owned().unwrap();
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        crate::sexp::owner::with_runtime(&owner, |access| {
            let kernel = Kernel::new(access, |_, _| Err(invalid("unused installer")));
            let (allocated, held) = access
                .with_arena(|arena| (arena.total_bytes_allocated(), arena.workspace_bytes()))?;
            let allowance =
                size_of::<NodeLink>() + 2 * size_of::<Value>() + size_of::<DerivativeFrame>() - 1;
            access.with_arena(|arena| {
                arena.set_budget(ArenaBudget::new(allocated + held + allowance, 0))
            })?;
            let error = kernel.derive(&expression, &variable).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("cannot retain derivative frames"),
                "{error}"
            );
            assert!(kernel.active.borrow().is_empty());
            assert_eq!(
                access.with_arena(|arena| arena.workspace_bytes())?,
                held + size_of::<NodeLink>()
            );
            access.with_arena(|arena| arena.set_budget(ArenaBudget::unlimited()))?;
            assert_eq!(kernel.derive(&expression, &variable)?.try_real_elt(0)?, 0.);
            drop(kernel);
            assert_eq!(access.with_arena(|arena| arena.workspace_bytes())?, held);
            Ok::<_, SexpError>(())
        })
        .unwrap()
        .unwrap();
    }

    #[test]
    fn common_unary_preserves_construction_and_child_error_order() {
        let mut session = RSession::new_for_gc_tests();
        let variable = session
            .eval_code_with_output_capture("quote(x)")
            .0
            .unwrap()
            .into_owned()
            .unwrap();
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        for (source, expected) in [
            ("sin(foo(x))", vec!["cos"]),
            ("cos(foo(x))", vec!["sin"]),
            ("tan(foo(x))", vec![]),
            ("exp(foo(x))", vec![]),
            ("log(foo(x))", vec![]),
            ("expm1(foo(x))", vec!["exp"]),
            ("pnorm(foo(x))", vec!["dnorm"]),
            ("dnorm(foo(x))", vec!["dnorm"]),
            ("log1p(foo(x))", vec![]),
            ("log2(foo(x))", vec![]),
            ("log10(foo(x))", vec![]),
            ("sinpi(foo(x))", vec!["pi"]),
            ("cospi(foo(x))", vec!["pi"]),
            ("tanpi(foo(x))", vec!["pi"]),
            ("gamma(foo(x))", vec!["digamma", "*"]),
            ("lgamma(foo(x))", vec![]),
            ("digamma(foo(x))", vec![]),
            ("trigamma(foo(x))", vec![]),
            ("psigamma(foo(x))", vec![]),
            ("psigamma(foo(x), nu)", vec!["+"]),
        ] {
            let expression = session
                .eval_code_with_output_capture(&format!("quote({source})"))
                .0
                .unwrap()
                .into_owned()
                .unwrap();
            let events = RefCell::new(Vec::new());
            crate::sexp::owner::with_runtime(&owner, |access| {
                let kernel = Kernel::new(access, |_, name| {
                    events.borrow_mut().push(name.to_owned());
                    Ok(variable.clone())
                });
                let error = kernel.derive(&expression, &variable).unwrap_err();
                assert!(
                    error
                        .to_string()
                        .contains("Function 'foo' is not in the derivatives table"),
                    "{error}"
                );
                assert!(kernel.active.borrow().is_empty());
                Ok::<_, SexpError>(())
            })
            .unwrap()
            .unwrap();
            assert_eq!(*events.borrow(), expected, "{source}");
        }
    }

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
            let _outer = PathRange {
                active: &kernel.active,
                start: 0,
            };
            kernel.push_active(&expression)?;
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
            let denied = kernel.push_active(&child).err().unwrap();
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
            let _inner = PathRange {
                active: &kernel.active,
                start: 1,
            };
            kernel.push_active(&child)?;
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
