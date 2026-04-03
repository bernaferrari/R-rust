use crate::env::Environment;
use crate::gc::{Gc, Trace};
use crate::sexp::Sexp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromiseState {
    Pending,
    Evaluating,
    Evaluated,
}

#[derive(Debug, Clone)]
pub struct Promise {
    state: PromiseState,
    expression: Sexp,
    environment: Gc<Environment>,
    value: Option<Sexp>,
    seen: bool,
}

unsafe impl Trace for Promise {
    fn trace(&self) {
        self.expression.trace();
        self.environment.trace();
        if let Some(val) = &self.value {
            val.trace();
        }
    }
}

impl Promise {
    pub fn new(expr: Sexp, env: Gc<Environment>) -> Self {
        Promise {
            state: PromiseState::Pending,
            expression: expr,
            environment: env,
            value: None,
            seen: false,
        }
    }

    pub fn state(&self) -> PromiseState {
        self.state
    }

    pub fn expression(&self) -> &Sexp {
        &self.expression
    }

    pub fn environment(&self) -> &Gc<Environment> {
        &self.environment
    }

    pub fn value(&self) -> Option<&Sexp> {
        self.value.as_ref()
    }

    pub fn is_forced(&self) -> bool {
        self.state == PromiseState::Evaluated
    }

    pub fn is_evaluating(&self) -> bool {
        self.state == PromiseState::Evaluating
    }

    pub fn mark_evaluating(&mut self) {
        debug_assert_eq!(self.state, PromiseState::Pending);
        self.state = PromiseState::Evaluating;
    }

    pub fn set_value(&mut self, value: Sexp) {
        debug_assert_eq!(self.state, PromiseState::Evaluating);
        self.value = Some(value);
        self.state = PromiseState::Evaluated;
    }

    pub fn seen(&self) -> bool {
        self.seen
    }

    pub fn set_seen(&mut self, seen: bool) {
        self.seen = seen;
    }
}
