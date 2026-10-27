#![forbid(unsafe_code)]
//! Original-runtime ownership for the outer deriv.c program builder.
use super::{builders::Builder, owned::Kernel};
use crate::sexp::{
    ffi::SEXPTYPE,
    heap::NodeLink,
    memory::WorkspaceReservation,
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::RuntimeAccess,
};
use std::io::Write;
type Value = Sexp<'static>;
fn invalid(message: &str) -> SexpError {
    SexpError::EvaluationFailed {
        message: message.into(),
    }
}
fn car(value: &Value) -> SexpResult<Value> {
    value.try_car()?.into_owned()
}
fn cdr(value: &Value) -> SexpResult<Value> {
    if value.is_nil() {
        Ok(value.clone())
    } else {
        value.try_cdr()?.into_owned()
    }
}

struct Indices {
    first: Vec<i32>,
    second: Vec<i32>,
    // Buffers drop before the original ledger ticket.
    _reservation: WorkspaceReservation,
}
impl Indices {
    fn new(access: &RuntimeAccess, count: usize, hessian: bool) -> SexpResult<Self> {
        let failure = || SexpError::AllocationFailed {
            object: "derivative indices",
        };
        let second = if hessian {
            count
                .checked_add(1)
                .and_then(|n| count.checked_mul(n))
                .map(|n| n / 2)
                .ok_or_else(failure)?
        } else {
            0
        };
        let bytes = count
            .checked_add(second)
            .and_then(|n| n.checked_mul(std::mem::size_of::<i32>()))
            .ok_or_else(failure)?;
        let reservation = access
            .with_arena(|a| a.reserve_workspace(bytes))?
            .ok_or_else(failure)?;
        let mut first = Vec::new();
        first.try_reserve_exact(count).map_err(|_| failure())?;
        first.resize(count, 0);
        let mut last = Vec::new();
        last.try_reserve_exact(second).map_err(|_| failure())?;
        last.resize(second, 0);
        Ok(Self {
            first,
            second: last,
            _reservation: reservation,
        })
    }
}

struct Path<'a> {
    node: NodeLink,
    previous: Option<&'a Path<'a>>,
}
fn enter<'a>(
    access: &RuntimeAccess,
    value: &Value,
    previous: Option<&'a Path<'a>>,
) -> SexpResult<Path<'a>> {
    let node = access.domain().link(value)?;
    let mut cursor = previous;
    while let Some(path) = cursor {
        if path.node == node {
            return Err(invalid("cyclic derivative expression"));
        }
        cursor = path.previous;
    }
    Ok(Path { node, previous })
}
// Brent's checkpoint follows only visited cells. A generation identity avoids
// retaining an already processed prefix after a callback detaches it.
struct ListPath {
    checkpoint: Option<NodeLink>,
    power: usize,
    distance: usize,
}
impl ListPath {
    fn new(access: &RuntimeAccess, first: &Value) -> SexpResult<Self> {
        Ok(Self {
            checkpoint: if first.is_nil() {
                None
            } else {
                Some(access.domain().link(first)?)
            },
            power: 1,
            distance: 0,
        })
    }
    fn advance(&mut self, access: &RuntimeAccess, next: &Value) -> SexpResult<()> {
        if next.is_nil() {
            return Ok(());
        }
        let next = access.domain().link(next)?;
        if Some(next) == self.checkpoint {
            return Err(invalid("cyclic derivative argument list"));
        }
        self.distance = self
            .distance
            .checked_add(1)
            .ok_or_else(|| invalid("derivative argument list is too large"))?;
        if self.distance == self.power {
            self.checkpoint = Some(next);
            self.distance = 0;
            self.power = self.power.saturating_mul(2);
        }
        Ok(())
    }
}

fn length(access: &RuntimeAccess, value: &Value) -> SexpResult<usize> {
    let too_long = || invalid("long vectors not supported by LENGTH");
    if matches!(
        value.typeof_(),
        SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP | SEXPTYPE::DOTSXP
    ) {
        let mut cell = value.clone();
        let mut seen = ListPath::new(access, &cell)?;
        let mut result = 0usize;
        while !cell.is_nil() {
            access.with_native(|_| {
                crate::eval::limits::poll_computation();
                Ok(())
            })?;
            result = result.checked_add(1).ok_or_else(too_long)?;
            if result > i32::MAX as usize {
                return Err(too_long());
            }
            let next = cdr(&cell)?;
            seen.advance(access, &next)?;
            cell = next;
        }
        Ok(result)
    } else {
        let result = usize::try_from(value.len()).map_err(|_| invalid("invalid variable names"))?;
        if result > i32::MAX as usize {
            Err(too_long())
        } else {
            Ok(result)
        }
    }
}

struct Program<'a> {
    access: &'a RuntimeAccess,
    builder: Builder<'a>,
    expressions: Value,
    tag: Value,
    tag_length: usize,
}
impl<'a> Program<'a> {
    fn checkpoint(&self) -> SexpResult<()> {
        self.access.with_native(|_| {
            crate::eval::limits::poll_computation();
            Ok(())
        })
    }
    fn same(&self, first: &Value, second: &Value) -> SexpResult<bool> {
        Ok(self.access.domain().link(first)? == self.access.domain().link(second)?)
    }
    fn set_car(&self, cell: &Value, value: &Value) -> SexpResult<()> {
        self.access.require_active()?;
        SexpMut::try_from_checked(cell.clone())?.try_set_pairlist_car(value)
    }
    fn set_cdr(&self, cell: &Value, value: &Value) -> SexpResult<()> {
        self.access.require_active()?;
        SexpMut::try_from_checked(cell.clone())?.try_set_pairlist_cdr(value)
    }
    fn variable(&self, number: i32) -> SexpResult<Value> {
        let failure = || SexpError::AllocationFailed {
            object: "derivative temporary name",
        };
        let mut digits = [0u8; 11];
        let length = {
            let mut out = std::io::Cursor::new(digits.as_mut_slice());
            write!(&mut out, "{number}").map_err(|_| failure())?;
            out.position() as usize
        };
        let length_total = self.tag_length.checked_add(length).ok_or_else(failure)?;
        let capacity = length_total.checked_add(1).ok_or_else(failure)?;
        let peak = length_total.checked_add(capacity).ok_or_else(failure)?;
        let _reservation = self
            .access
            .with_arena(|a| a.reserve_workspace(peak))?
            .ok_or_else(failure)?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(capacity).map_err(|_| failure())?;
        if self.tag_length != 0 {
            let header = self.tag.header();
            let lease = header.payload_lease().ok_or_else(failure)?;
            for index in 0..self.tag_length {
                bytes.push(lease.byte_elt(index).ok_or_else(failure)?);
            }
        }
        bytes.extend_from_slice(&digits[..length]);
        bytes.push(0);
        super::install_builder_formal(self.access, &bytes)
    }
    fn equal(
        &self,
        first: &Value,
        second: &Value,
        path_a: Option<&Path<'_>>,
        path_b: Option<&Path<'_>>,
    ) -> SexpResult<bool> {
        self.checkpoint()?;
        if first.typeof_() != second.typeof_() {
            return Ok(false);
        }
        match first.typeof_() {
            SEXPTYPE::NILSXP => Ok(true),
            SEXPTYPE::SYMSXP => self.same(first, second),
            SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP | SEXPTYPE::REALSXP => {
                super::equal_derivative_scalars(self.access, first, second)
            }
            SEXPTYPE::LANGSXP | SEXPTYPE::LISTSXP => {
                let a = enter(self.access, first, path_a)?;
                let b = enter(self.access, second, path_b)?;
                if !self.equal(&car(first)?, &car(second)?, Some(&a), Some(&b))? {
                    return Ok(false);
                }
                self.equal(&cdr(first)?, &cdr(second)?, Some(&a), Some(&b))
            }
            // Preserve the existing port's comparisons; additional GNU
            // expression kinds have separate oracle-driven fidelity work.
            _ => Ok(false),
        }
    }
    fn append(&self, expression: &Value, compare: bool) -> SexpResult<i32> {
        let mut cell = self.expressions.clone();
        let mut count = 0i32;
        let mut seen = ListPath::new(self.access, &cell)?;
        loop {
            self.checkpoint()?;
            let next = cdr(&cell)?;
            if next.is_nil() {
                break;
            }
            seen.advance(self.access, &next)?;
            cell = next;
            count = count
                .checked_add(1)
                .ok_or_else(|| invalid("too many derivative expressions"))?;
            if compare && self.equal(expression, &car(&cell)?, None, None)? {
                return Ok(count);
            }
        }
        let domain = self.access.domain();
        let tail = self.access.allocator(&domain)?.pairlist_cell(
            expression,
            &domain.nil(),
            &domain.nil(),
        )?;
        self.set_cdr(&cell, &tail)?;
        count
            .checked_add(1)
            .ok_or_else(|| invalid("too many derivative expressions"))
    }
    fn find(&self, expression: &Value, previous: Option<&Path<'_>>) -> SexpResult<i32> {
        self.checkpoint()?;
        if expression.typeof_() != SEXPTYPE::LANGSXP {
            return Ok(0);
        }
        let path = enter(self.access, expression, previous)?;
        let parenthesis = self.builder.symbol("(")?;
        if self.same(&car(expression)?, &parenthesis)? {
            return self.find(&car(&cdr(expression)?)?, Some(&path));
        }
        let mut cell = cdr(expression)?;
        let mut seen = ListPath::new(self.access, &cell)?;
        while !cell.is_nil() {
            self.checkpoint()?;
            let selected = car(&cell)?;
            let index = self.find(&selected, Some(&path))?;
            if index != 0 {
                self.set_car(&cell, &self.variable(index)?)?;
            }
            let next = cdr(&cell)?;
            seen.advance(self.access, &next)?;
            cell = next;
        }
        self.append(expression, true)
    }
    fn count(
        &self,
        symbol: &Value,
        expression: &Value,
        previous: Option<&Path<'_>>,
    ) -> SexpResult<usize> {
        self.checkpoint()?;
        match expression.typeof_() {
            SEXPTYPE::SYMSXP => Ok(usize::from(self.same(symbol, expression)?)),
            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => {
                let path = enter(self.access, expression, previous)?;
                self.count(symbol, &car(expression)?, Some(&path))?
                    .checked_add(self.count(symbol, &cdr(expression)?, Some(&path))?)
                    .ok_or_else(|| invalid("too many derivative occurrences"))
            }
            _ => Ok(0),
        }
    }
    fn replace(
        &self,
        symbol: &Value,
        value: &Value,
        expression: &Value,
        previous: Option<&Path<'_>>,
    ) -> SexpResult<Value> {
        self.checkpoint()?;
        match expression.typeof_() {
            SEXPTYPE::SYMSXP if self.same(symbol, expression)? => Ok(value.clone()),
            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => {
                let path = enter(self.access, expression, previous)?;
                let next = self.replace(symbol, value, &car(expression)?, Some(&path))?;
                self.set_car(expression, &next)?;
                let next = self.replace(symbol, value, &cdr(expression)?, Some(&path))?;
                self.set_cdr(expression, &next)?;
                Ok(expression.clone())
            }
            _ => Ok(expression.clone()),
        }
    }
    fn binary_form(&self, expression: &Value, operator: &str) -> SexpResult<bool> {
        if expression.typeof_() != SEXPTYPE::LANGSXP {
            return Ok(false);
        }
        if !self.same(&car(expression)?, &self.builder.symbol(operator)?)? {
            return Ok(false);
        }
        let first = cdr(expression)?;
        if first.is_nil() {
            return Ok(false);
        }
        let second = cdr(&first)?;
        Ok(!second.is_nil() && cdr(&second)?.is_nil())
    }
    fn parentheses(&self, expression: &Value, previous: Option<&Path<'_>>) -> SexpResult<Value> {
        self.checkpoint()?;
        if expression.typeof_() != SEXPTYPE::LANGSXP {
            return Ok(expression.clone());
        }
        let path = enter(self.access, expression, previous)?;
        let mut cell = cdr(expression)?;
        let mut seen = ListPath::new(self.access, &cell)?;
        while !cell.is_nil() {
            let selected = car(&cell)?;
            let nested = self.parentheses(&selected, Some(&path))?;
            self.set_car(&cell, &nested)?;
            let next = cdr(&cell)?;
            seen.advance(self.access, &next)?;
            cell = next;
        }
        let first_cell = cdr(expression)?;
        if first_cell.is_nil() {
            return Ok(expression.clone());
        }
        let second_cell = cdr(&first_cell)?;
        if second_cell.is_nil() {
            return Ok(expression.clone());
        }
        let first = car(&first_cell)?;
        let second = car(&second_cell)?;
        let plus = self.binary_form(expression, "+")?;
        let minus = self.binary_form(expression, "-")?;
        let times = self.binary_form(expression, "*")?;
        let divide = self.binary_form(expression, "/")?;
        if plus && self.binary_form(&second, "+")? {
            self.set_car(&second_cell, &self.builder.call("(", &[second])?)?;
        } else if minus && (self.binary_form(&second, "+")? || self.binary_form(&second, "-")?) {
            self.set_car(&second_cell, &self.builder.call("(", &[second])?)?;
        } else if times || divide {
            if self.binary_form(&first, "+")? || self.binary_form(&first, "-")? {
                self.set_car(&first_cell, &self.builder.call("(", &[first])?)?;
            }
            if self.binary_form(&second, "+")?
                || self.binary_form(&second, "-")?
                || self.binary_form(&second, "*")?
                || self.binary_form(&second, "/")?
            {
                self.set_car(&second_cell, &self.builder.call("(", &[second])?)?;
            }
        } else if self.binary_form(expression, "^")? && self.binary_form(&first, "^")? {
            self.set_car(&first_cell, &self.builder.call("(", &[first])?)?;
        }
        Ok(expression.clone())
    }
    fn derive(&self, expression: &Value, variable: &Value) -> SexpResult<Value> {
        // Match the original per-derivative kernel lifetime; do not retain its
        // native traversal storage between independent derivative calls.
        Kernel::new(self.access, super::derivative_symbol).derive(expression, variable)
    }
    fn duplicate(&self, expression: &Value) -> SexpResult<Value> {
        super::duplicate_builder_value(self.access, expression)
    }
    fn derive_copy(&self, expression: &Value, variable: &Value) -> SexpResult<Value> {
        let copied = self.duplicate(expression)?;
        self.derive(&copied, variable)
    }

    fn name(&self, names: &Value, index: usize) -> SexpResult<Value> {
        let name = super::derivative_string_element(self.access, names, index as _)?;
        self.builder.formal_symbol(&name)
    }
    fn zero(&self, expression: &Value) -> SexpResult<bool> {
        super::derivative_is_zero(self.access, expression)
    }
    fn finish(
        &self,
        count: usize,
        variables: usize,
        hessian: bool,
        names: &Value,
        function: &Value,
    ) -> SexpResult<Value> {
        let mut cell = cdr(&self.expressions)?;
        for index in 0..count {
            self.checkpoint()?;
            let symbol = self.variable(
                i32::try_from(index + 1).map_err(|_| invalid("too many derivative expressions"))?,
            )?;
            let remainder = cdr(&cell)?;
            if self.count(&symbol, &remainder, None)? < 2 {
                let value = car(&cell)?;
                self.set_cdr(&cell, &self.replace(&symbol, &value, &remainder, None)?)?;
                self.set_car(&cell, &self.access.domain().missing())?;
            } else {
                let value = self.parentheses(&car(&cell)?, None)?;
                self.set_car(&cell, &self.builder.call("<-", &[symbol, value])?)?;
            }
            cell = cdr(&cell)?;
        }
        let value = self.parentheses(&car(&cell)?, None)?;
        self.set_car(
            &cell,
            &self
                .builder
                .call("<-", &[self.builder.symbol(".value")?, value])?,
        )?;
        cell = cdr(&cell)?;
        self.set_car(&cell, &self.builder.initialize(names, false)?)?;
        cell = cdr(&cell)?;
        if hessian {
            self.set_car(&cell, &self.builder.initialize(names, true)?)?;
            cell = cdr(&cell)?;
        }
        for i in 0..variables {
            let name = super::derivative_string_element(self.access, names, i as _)?;
            let value = self.parentheses(&car(&cell)?, None)?;
            self.set_car(&cell, &self.builder.assign_gradient(&name, &value)?)?;
            cell = cdr(&cell)?;
            if hessian {
                for j in i..variables {
                    if !self.same(&car(&cell)?, &self.access.domain().missing())? {
                        let first = super::derivative_string_element(self.access, names, i as _)?;
                        let second = if i == j {
                            None
                        } else {
                            Some(super::derivative_string_element(
                                self.access,
                                names,
                                j as _,
                            )?)
                        };
                        let value = self.parentheses(&car(&cell)?, None)?;
                        self.set_car(
                            &cell,
                            &self
                                .builder
                                .assign_hessian(&first, second.as_ref(), &value)?,
                        )?;
                    }
                    cell = cdr(&cell)?;
                }
            }
        }
        self.set_car(&cell, &self.builder.attach(false)?)?;
        cell = cdr(&cell)?;
        if hessian {
            self.set_car(&cell, &self.builder.attach(true)?)?;
            cell = cdr(&cell)?;
        }
        self.set_car(&cell, &self.builder.symbol(".value")?)?;
        let mut previous = self.expressions.clone();
        let mut current = cdr(&previous)?;
        let mut seen = ListPath::new(self.access, &current)?;
        while !current.is_nil() {
            self.checkpoint()?;
            let next = cdr(&current)?;
            seen.advance(self.access, &next)?;
            if self.same(&car(&current)?, &self.access.domain().missing())? {
                self.set_cdr(&previous, &next)?;
            } else {
                previous = current;
            }
            current = next;
        }
        if function.typeof_() == SEXPTYPE::LGLSXP
            && super::derivative_function_flag(self.access, function)?
        {
            return self.builder.closure(names, &self.expressions);
        }
        if function.typeof_() == SEXPTYPE::CLOSXP {
            let formals = function.try_formals()?.into_owned()?;
            let environment = function.try_cloenv()?.into_owned()?;
            return self
                .builder
                .closure_with_parts(&formals, &self.expressions, &environment);
        }
        let domain = self.access.domain();
        let result = self
            .access
            .allocator(&domain)?
            .allocate(|a| Some(a.alloc_vector(SEXPTYPE::EXPRSXP, 1)))?;
        let mut result = SexpMut::try_from_checked(result)?;
        result.try_set_vector_elt(0, self.expressions.clone())?;
        Ok(result.freeze())
    }
}

pub(super) fn build(access: &RuntimeAccess, args: &Value) -> SexpResult<Value> {
    let domain = access.domain();
    domain.link(args)?;
    let mut cursor = cdr(args)?;
    let mut selected = [
        domain.nil(),
        domain.nil(),
        domain.nil(),
        domain.nil(),
        domain.nil(),
    ];
    // Select actual owning values before any expression/ALTREP provider runs.
    for value in &mut selected {
        if !cursor.is_nil() {
            *value = car(&cursor)?;
            cursor = cdr(&cursor)?;
        }
    }
    let [given, names, function, tag, hessian] = selected;
    let expression = if given.typeof_() == SEXPTYPE::EXPRSXP {
        let selected = given.try_vector_elt(0)?.into_owned()?;
        drop(given);
        selected
    } else {
        given
    };
    let count = length(access, &names)?;
    let selected_tag = super::derivative_string_element(access, &tag, 0)?;
    drop(tag);
    let tag = selected_tag;
    let tag_length = super::derivative_string_prefix(access, &tag)?;
    let hessian_flag = super::derivative_hessian_flag(access, &hessian)?;
    drop(hessian);
    let hessian = hessian_flag;
    let builder = Builder::new(access);
    let expressions = builder.call("{", &[])?;
    let program = Program {
        access,
        builder,
        expressions,
        tag,
        tag_length,
    };
    let duplicated = program.duplicate(&expression)?;
    let f_index = program.find(&duplicated, None)?;
    let mut indices = Indices::new(access, count, hessian)?;
    drop(duplicated);
    let mut k = 0;
    for i in 0..count {
        program.checkpoint()?;
        let variable = program.name(&names, i)?;
        let derived = program.derive_copy(&expression, &variable)?;
        indices.first[i] = program.find(&derived, None)?;
        drop(derived);
        if hessian {
            let first = program.derive_copy(&expression, &variable)?;
            for j in i..count {
                let variable = program.name(&names, j)?;
                let second = program.derive_copy(&first, &variable)?;
                indices.second[k] = program.find(&second, None)?;
                k += 1;
            }
        }
    }
    let mut total = 0;
    let mut tail = cdr(&program.expressions)?;
    while !tail.is_nil() {
        total += 1;
        tail = cdr(&tail)?;
    }
    if f_index != 0 {
        program.append(&program.variable(f_index)?, false)?;
    } else {
        program.append(&program.duplicate(&expression)?, false)?;
    }
    program.append(&domain.missing(), false)?;
    if hessian {
        program.append(&domain.missing(), false)?;
    }
    let mut k = 0;
    for i in 0..count {
        let variable = program.name(&names, i)?;
        if indices.first[i] != 0 {
            program.append(&program.variable(indices.first[i])?, false)?;
        } else {
            let derived = program.derive_copy(&expression, &variable)?;
            program.append(&derived, false)?;
        }
        if hessian {
            let first = {
                let copied = program.duplicate(&expression)?;
                // Preserve the legacy extra name-provider read, after the
                // duplicate, when the first derivative has no cached entry.
                let selected = if indices.first[i] == 0 {
                    program.name(&names, i)?
                } else {
                    variable.clone()
                };
                program.derive(&copied, &selected)?
            };
            for j in i..count {
                if indices.second[k] != 0 {
                    program.append(&program.variable(indices.second[k])?, false)?;
                } else {
                    let variable = program.name(&names, j)?;
                    let copied = program.duplicate(&first)?;
                    let second = program.derive(&copied, &variable)?;
                    if indices.first[i] == 0 && program.zero(&second)? {
                        program.append(&domain.missing(), false)?;
                    } else {
                        program.append(&second, false)?;
                    }
                }
                k += 1;
            }
        }
    }
    program.append(&domain.missing(), false)?;
    program.append(&domain.missing(), false)?;
    if hessian {
        program.append(&domain.missing(), false)?;
    }
    drop(indices);
    drop(expression);
    program.finish(total, count, hessian, &names, &function)
}

#[cfg(test)]
#[path = "outer/index_tests.rs"]
mod index_tests;
