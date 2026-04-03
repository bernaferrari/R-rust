use crate::env::Environment;
use crate::gc::{Gc, Root, Scope};
use crate::promise::{Promise, PromiseState};
use crate::sexp::{Sexp, Tag};
use crate::symbol::Symbol;
use alloc::vec::Vec;
use core::result;

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    UnboundVariable(Symbol),
    UndefinedFunction(Symbol),
    InvalidArgumentCount(usize, usize),
    TypeMismatch(Tag, Tag),
    ZeroLengthVariable,
    AttemptToApplyNonFunction,
    MissingArgument,
    ExtraArgument,
    UserDefined(Sexp),
    Interrupt,
    StackOverflow,
}

pub struct Evaluator {
    stack: Vec<EvalFrame>,
    depth: usize,
    max_depth: usize,
}

#[derive(Debug, Clone)]
pub struct EvalFrame {
    expression: Sexp,
    environment: Gc<Environment>,
    call_env: Option<Gc<Environment>>,
    next_op: EvalOp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalOp {
    Eval,
    EvalCar,
    Apply,
    EvalArgs,
    EvalNextArg,
    Return,
}

impl Evaluator {
    pub const DEFAULT_MAX_DEPTH: usize = 10000;

    pub fn new() -> Self {
        Evaluator {
            stack: Vec::with_capacity(1024),
            depth: 0,
            max_depth: Self::DEFAULT_MAX_DEPTH,
        }
    }

    pub fn eval(&mut self, expr: Sexp, env: Gc<Environment>) -> Result<Sexp> {
        self.depth = 0;
        self.stack.clear();

        self.stack.push(EvalFrame {
            expression: expr,
            environment: env,
            call_env: None,
            next_op: EvalOp::Eval,
        });

        self.run()
    }

    fn run(&mut self) -> Result<Sexp> {
        while let Some(frame) = self.stack.pop() {
            match frame.next_op {
                EvalOp::Eval => self.eval_step(frame)?,
                EvalOp::EvalCar => todo!(),
                EvalOp::Apply => todo!(),
                EvalOp::EvalArgs => todo!(),
                EvalOp::EvalNextArg => todo!(),
                EvalOp::Return => return Ok(frame.expression),
            }
        }

        unreachable!("evaluation stack underflow")
    }

    fn eval_step(&mut self, frame: EvalFrame) -> Result<()> {
        match frame.expression.tag() {
            Tag::Symbol => self.eval_symbol(frame.expression, frame.environment),
            Tag::Promise => self.eval_promise(frame.expression, frame.environment),
            Tag::List if frame.expression.is_null() => Ok(()),
            Tag::List => self.eval_application(frame.expression, frame.environment),
            _ => {
                self.stack.push(EvalFrame {
                    expression: frame.expression,
                    environment: frame.environment,
                    call_env: None,
                    next_op: EvalOp::Return,
                });
                Ok(())
            }
        }
    }

    fn eval_symbol(&mut self, sym: Sexp, env: Gc<Environment>) -> Result<()> {
        let symbol =
            Symbol::from_sexp(sym).ok_or_else(|| Error::TypeMismatch(Tag::Symbol, sym.tag()))?;

        match env.get(symbol) {
            Some(value) => {
                self.stack.push(EvalFrame {
                    expression: value,
                    environment: env,
                    call_env: None,
                    next_op: EvalOp::Eval,
                });
                Ok(())
            }
            None => Err(Error::UnboundVariable(symbol)),
        }
    }

    fn eval_promise(&mut self, prom: Sexp, env: Gc<Environment>) -> Result<()> {
        let promise = prom
            .as_promise()
            .ok_or_else(|| Error::TypeMismatch(Tag::Promise, prom.tag()))?;

        match promise.state() {
            PromiseState::Evaluated => {
                self.stack.push(EvalFrame {
                    expression: promise.value().unwrap().clone(),
                    environment: env,
                    call_env: None,
                    next_op: EvalOp::Eval,
                });
                Ok(())
            }
            PromiseState::Evaluating => {
                todo!("promise cycle detection")
            }
            PromiseState::Pending => {
                todo!("promise forcing")
            }
        }
    }

    fn eval_application(&mut self, expr: Sexp, env: Gc<Environment>) -> Result<()> {
        let car = expr.car();
        let cdr = expr.cdr();

        self.stack.push(EvalFrame {
            expression: cdr,
            environment: env.clone(),
            call_env: Some(env),
            next_op: EvalOp::EvalArgs,
        });

        self.stack.push(EvalFrame {
            expression: car,
            environment: env,
            call_env: None,
            next_op: EvalOp::EvalCar,
        });

        Ok(())
    }

    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    pub fn set_max_depth(&mut self, depth: usize) {
        self.max_depth = depth;
    }

    pub fn current_depth(&self) -> usize {
        self.depth
    }
}
