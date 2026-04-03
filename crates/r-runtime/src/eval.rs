use crate::sexp::Sexp;
use alloc::string::String;
use alloc::vec::Vec;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    UnboundVariable(String),
    UndefinedFunction(String),
    InvalidArgumentCount(usize, usize),
    TypeMismatch(String, String),
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

#[derive(Debug)]
pub struct EvalFrame {
    expression: Sexp,
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

    pub fn eval(&mut self, _expr: Sexp) -> Result<Sexp> {
        todo!("eval not yet implemented")
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

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}
