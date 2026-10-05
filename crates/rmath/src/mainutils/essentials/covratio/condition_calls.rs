#![forbid(unsafe_code)]
//! Owned arithmetic syntax, separate from the already captured numeric operands.
use super::{Result, Value, active, checked};
use crate::sexp::{ffi::SEXPTYPE, owner::RuntimeAccess};

pub(super) enum Phase {
    Denominator,
    Studentized { fixed_dispersion: bool },
    Product,
    RowMap,
}
struct Syntax<'a, I> {
    access: &'a RuntimeAccess,
    intern: &'a mut I,
}
impl<I: FnMut(&str) -> Result<Value>> Syntax<'_, I> {
    fn symbol(&mut self, name: &str) -> Result<Value> {
        active(self.access)?;
        let symbol = (self.intern)(name)?;
        active(self.access)?;
        if symbol.typeof_() != SEXPTYPE::SYMSXP {
            return Err("invalid arithmetic syntax symbol".into());
        }
        checked(self.access.domain().link(&symbol))?;
        Ok(symbol)
    }
    fn call(&mut self, name: &str, arguments: &[Value]) -> Result<Value> {
        let function = self.symbol(name)?;
        let domain = self.access.domain();
        let allocator = checked(self.access.allocator(&domain))?;
        let mut tail = domain.nil();
        for argument in arguments.iter().rev() {
            tail = checked(allocator.pairlist_cell(argument, &tail, &domain.nil()))?;
        }
        checked(allocator.call(&function, &tail))
    }
    fn number(&self, value: f64) -> Result<Value> {
        let domain = self.access.domain();
        let allocator = checked(self.access.allocator(&domain))?;
        checked(allocator.allocate(|arena| {
            let pointer = arena.alloc_vector(SEXPTYPE::REALSXP, 1);
            let node = arena.node_token(pointer)?;
            arena
                .heap_identity()
                .payload_lease(&node)?
                .set_real_elt(0, value)?;
            Some(pointer)
        }))
    }
    fn integer(&self, value: i32) -> Result<Value> {
        let domain = self.access.domain();
        let allocator = checked(self.access.allocator(&domain))?;
        checked(allocator.allocate(|arena| {
            let pointer = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
            let node = arena.node_token(pointer)?;
            arena
                .heap_identity()
                .payload_lease(&node)?
                .set_integer_elt(0, value)?;
            Some(pointer)
        }))
    }
    fn denominator(&mut self, fixed: bool) -> Result<Value> {
        let sigma = if fixed {
            let model = self.symbol("model")?;
            self.call("sigma", &[model])?
        } else {
            let influence = self.symbol("infl")?;
            let sigma = self.symbol("sigma")?;
            self.call("$", &[influence, sigma])?
        };
        let omh = self.symbol("omh")?;
        let root = self.call("sqrt", &[omh])?;
        self.call("*", &[sigma, root])
    }
    fn build(&mut self, phase: Phase) -> Result<Value> {
        match phase {
            Phase::RowMap => {
                let keep = self.symbol("keep")?;
                let omit = self.symbol("omit")?;
                let negative = self.call("-", &[omit])?;
                let target = self.call("[", &[keep, negative])?;
                let one = self.integer(1)?;
                let n = self.symbol("n")?;
                let sequence = self.call(":", &[one, n])?;
                self.call("<-", &[target, sequence])
            }
            Phase::Denominator => self.denominator(false),
            Phase::Studentized { fixed_dispersion } => {
                let residuals = self.symbol("res")?;
                let denominator = self.denominator(fixed_dispersion)?;
                let parenthesized = self.call("(", &[denominator])?;
                self.call("/", &[residuals, parenthesized])
            }
            Phase::Product => {
                let n = self.symbol("n")?;
                let p = self.symbol("p")?;
                let difference = self.call("-", &[n.clone(), p.clone()])?;
                let one = self.number(1.)?;
                let difference = self.call("-", &[difference, one])?;
                let difference = self.call("(", &[difference])?;
                let star = self.symbol("e.star")?;
                let two = self.number(2.)?;
                let squared = self.call("^", &[star, two])?;
                let numerator = self.call("+", &[difference, squared])?;
                let numerator = self.call("(", &[numerator])?;
                let denominator = self.call("-", &[n, p.clone()])?;
                let denominator = self.call("(", &[denominator])?;
                let quotient = self.call("/", &[numerator, denominator])?;
                let quotient = self.call("(", &[quotient])?;
                let power = self.call("^", &[quotient, p])?;
                let omh = self.symbol("omh")?;
                self.call("*", &[omh, power])
            }
        }
    }
}
/// Caller has already snapshotted the operation's owning fields and numbers.
/// Symbol/allocation callbacks may reenter, collect, or revoke the runtime.
pub(super) fn emit(
    access: &RuntimeAccess,
    phase: Phase,
    intern: &mut impl FnMut(&str) -> Result<Value>,
) -> Result<()> {
    active(access)?;
    let message = if matches!(phase, Phase::RowMap) {
        "number of items to replace is not a multiple of replacement length"
    } else {
        "longer object length is not a multiple of shorter object length"
    };
    let call = Syntax { access, intern }.build(phase)?;
    active(access)?;
    let _attribution = crate::mainutils::errors::warning_call_guard(call.as_raw());
    crate::mainutils::errors::nmath_warning_hook(message);
    active(access)
}
